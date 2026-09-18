//! Length-prefixed framing, and the one place the wire format is chosen.
//!
//! # How the format is negotiated
//!
//! On a byte-stream connection (pipe, UDS, later TCP) the **first frame is a
//! [`Hello`], and it is always JSON.** Everything after it is in the format the
//! `Hello` named. Not a header — a named pipe has none — and not a query
//! parameter, which would put the answer in a URL that only one of the three
//! listeners has. HTTP is the exception that proves it: there the format is the
//! `Accept` header, because that is what an off-the-shelf AG-UI client already
//! sends.
//!
//! Frames are `u32` big-endian length, then that many bytes. No trailer, no
//! escape: a reader that knows the length never has to scan.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::TransportError;

/// The largest frame either side will allocate for: 16 MiB.
///
/// A length prefix is an instruction to allocate, so it is also an instruction
/// a hostile peer would like to give.
pub const MAX_FRAME_BYTES: u64 = 16 * 1024 * 1024;

/// How the bytes after the handshake are encoded.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireFormat {
    /// Readable, and what every client can already parse.
    #[default]
    Json,
    /// Compact. `ciborium`; MessagePack is impossible here because our tagged
    /// enums are internally tagged and MessagePack has no map-in-map tag form
    /// serde can round-trip.
    Cbor,
}

impl std::str::FromStr for WireFormat {
    type Err = TransportError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "json" => Ok(WireFormat::Json),
            "cbor" => Ok(WireFormat::Cbor),
            other => Err(TransportError::UnknownFormat(other.to_owned())),
        }
    }
}

/// The first frame on a byte-stream connection. Always JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    /// What the rest of this connection is encoded in.
    pub format: WireFormat,
    /// The AG-UI protocol version the client was built against.
    pub protocol: String,
    /// The session to attach to, when the client already knows which.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Replay from just after this. Absent means from the start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
}

impl Default for Hello {
    fn default() -> Self {
        Self {
            format: WireFormat::Json,
            protocol: orrery_agui::AGUI_PROTOCOL_VERSION.to_owned(),
            session: None,
            since: None,
        }
    }
}

/// Encodes and decodes frames in one format.
#[derive(Copy, Clone, Debug, Default)]
pub struct Codec {
    format: WireFormat,
}

impl Codec {
    /// A codec for one format.
    #[must_use]
    pub fn new(format: WireFormat) -> Self {
        Self { format }
    }

    /// The format this codec speaks.
    #[must_use]
    pub fn format(&self) -> WireFormat {
        self.format
    }

    /// Encode one value to a frame body.
    ///
    /// # Errors
    ///
    /// [`TransportError::Codec`] when the value will not serialise.
    pub fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>, TransportError> {
        match self.format {
            WireFormat::Json => {
                serde_json::to_vec(value).map_err(|e| TransportError::Codec(e.to_string()))
            }
            WireFormat::Cbor => {
                let mut out = Vec::new();
                ciborium::into_writer(value, &mut out)
                    .map_err(|e| TransportError::Codec(e.to_string()))?;
                Ok(out)
            }
        }
    }

    /// Decode one frame body.
    ///
    /// # Errors
    ///
    /// [`TransportError::Codec`] when the bytes are not that value.
    pub fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, TransportError> {
        match self.format {
            WireFormat::Json => {
                serde_json::from_slice(bytes).map_err(|e| TransportError::Codec(e.to_string()))
            }
            WireFormat::Cbor => {
                ciborium::from_reader(bytes).map_err(|e| TransportError::Codec(e.to_string()))
            }
        }
    }

    /// Write one length-prefixed frame.
    ///
    /// # Errors
    ///
    /// [`TransportError::Codec`] or [`TransportError::Io`].
    pub async fn write<W, T>(&self, w: &mut W, value: &T) -> Result<(), TransportError>
    where
        W: AsyncWrite + Unpin,
        T: Serialize,
    {
        let body = self.encode(value)?;
        write_body(w, &body).await
    }

    /// Read one length-prefixed frame.
    ///
    /// # Errors
    ///
    /// [`TransportError::Codec`], [`TransportError::FrameTooLarge`] or
    /// [`TransportError::Io`].
    pub async fn read<R, T>(&self, r: &mut R) -> Result<T, TransportError>
    where
        R: AsyncRead + Unpin,
        T: DeserializeOwned,
    {
        let body = read_body(r).await?;
        self.decode(&body)
    }
}

/// Write a frame body with its length prefix.
///
/// # Errors
///
/// [`TransportError::Io`].
pub async fn write_body<W: AsyncWrite + Unpin>(
    w: &mut W,
    body: &[u8],
) -> Result<(), TransportError> {
    let len = u32::try_from(body.len()).map_err(|_| TransportError::FrameTooLarge {
        len: body.len() as u64,
        max: MAX_FRAME_BYTES,
    })?;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(body).await?;
    w.flush().await?;
    Ok(())
}

/// Read one frame body, length prefix first.
///
/// # Errors
///
/// [`TransportError::FrameTooLarge`] or [`TransportError::Io`].
pub async fn read_body<R: AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>, TransportError> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let len = u64::from(u32::from_be_bytes(len));
    if len > MAX_FRAME_BYTES {
        return Err(TransportError::FrameTooLarge {
            len,
            max: MAX_FRAME_BYTES,
        });
    }
    let mut body = vec![0u8; usize::try_from(len).unwrap_or(0)];
    r.read_exact(&mut body).await?;
    Ok(body)
}
