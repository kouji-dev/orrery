//! The named-pipe / UDS listener: `orrery attach` on the same machine.
//!
//! `interprocess` gives one API over both, so there is no `#[cfg]` fork here and
//! no second framing story on Windows. Frames are length-prefixed
//! ([`crate::frame`]); the format is negotiated by the first frame, which is
//! always JSON.
//!
//! A disconnect is not the end of anything. The turn is the kernel's, not the
//! connection's: a client that drops mid-turn comes back with
//! `session.attach(since)` and is handed the gap.

use std::time::Duration;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, ToNsName};
use orrery_agui::Frame;
use orrery_proto::Request;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::task::JoinHandle;

use crate::error::TransportError;
use crate::frame::{Codec, Hello, WireFormat, read_body, write_body};
use crate::listener::ControlHandler;
use crate::{ClientStream, Hub};

/// What the server says back to a [`Hello`]. Always JSON, like the `Hello`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HelloAck {
    /// The format that was agreed. Never anything the client did not ask for.
    pub format: WireFormat,
    /// The protocol version this server speaks.
    pub protocol: String,
    /// The last `seq` the session has reached, so a client knows how far behind
    /// it starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    /// Set when the requested `since` was older than the replay ring and the
    /// client has to ask the session store instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_too_old: Option<u64>,
}

/// A running pipe listener.
#[derive(Debug)]
pub struct PipeServer {
    name: String,
    task: JoinHandle<()>,
}

impl PipeServer {
    /// The name clients connect to.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Stop accepting. Connections already open are dropped with it.
    pub fn shutdown(self) {
        self.task.abort();
    }
}

/// Start accepting connections on `name`.
///
/// Every connection gets its own [`ClientStream`], which means its own
/// coalescer: a client attached from a terminal that has been scrolled away
/// does not slow down the one that is being watched.
///
/// # Errors
///
/// [`TransportError::Io`] when the name is taken or cannot be created.
pub fn serve<H>(
    name: &str,
    hub: Hub,
    tick: Duration,
    control: H,
) -> Result<PipeServer, TransportError>
where
    H: ControlHandler,
{
    let ns = name
        .to_ns_name::<GenericNamespaced>()
        .map_err(TransportError::Io)?;
    let listener = ListenerOptions::new()
        .name(ns)
        .create_tokio()
        .map_err(TransportError::Io)?;
    let owned = name.to_owned();
    let control = std::sync::Arc::new(control);
    let task = tokio::spawn(async move {
        loop {
            let Ok(conn) = listener.accept().await else {
                continue;
            };
            let hub = hub.clone();
            let control = std::sync::Arc::clone(&control);
            tokio::spawn(async move {
                if let Err(err) = session(conn, hub, tick, control).await {
                    tracing::debug!(?err, "pipe connection ended");
                }
            });
        }
    });
    Ok(PipeServer { name: owned, task })
}

async fn session<S, H>(
    conn: S,
    hub: Hub,
    tick: Duration,
    control: std::sync::Arc<H>,
) -> Result<(), TransportError>
where
    S: AsyncRead + AsyncWrite + Send + 'static,
    H: ControlHandler,
{
    let (mut reader, mut writer) = tokio::io::split(conn);

    // The handshake is JSON whatever comes next, because the frame that names
    // the format cannot be in the format it names.
    let json = Codec::new(WireFormat::Json);
    let hello: Hello = json.decode(&read_body(&mut reader).await?)?;
    let codec = Codec::new(hello.format);

    // Subscribe before replaying, so nothing published in between is lost.
    let mut stream: ClientStream = hub.subscribe(tick);
    let replay = hub.attach(hello.since);
    let ack = HelloAck {
        format: hello.format,
        protocol: orrery_agui::AGUI_PROTOCOL_VERSION.to_owned(),
        last_seq: hub.last_seq(),
        replay_too_old: match &replay {
            Err(crate::ReplayError::TooOld { earliest }) => Some(*earliest),
            _ => None,
        },
    };
    write_body(&mut writer, &json.encode(&ack)?).await?;

    for frame in replay.unwrap_or_default() {
        write_body(&mut writer, &codec.encode(&frame)?).await?;
    }

    // One task reads control requests, one writes frames. A client that stops
    // reading stalls its own writer and nothing else.
    let reader_task = tokio::spawn(async move {
        loop {
            let Ok(body) = read_body(&mut reader).await else {
                return;
            };
            let Ok(request) = codec.decode::<Request>(&body) else {
                continue;
            };
            let _ = control.control(request);
        }
    });

    let write_result = async {
        while let Some(batch) = stream.next_batch().await {
            for frame in batch {
                write_body(&mut writer, &codec.encode(&frame)?).await?;
            }
        }
        Ok::<(), TransportError>(())
    }
    .await;

    reader_task.abort();
    write_result
}

/// A connected pipe client.
#[derive(Debug)]
pub struct PipeClient<S> {
    reader: tokio::io::ReadHalf<S>,
    writer: tokio::io::WriteHalf<S>,
    codec: Codec,
    ack: HelloAck,
}

/// Connect to a pipe listener.
///
/// # Errors
///
/// [`TransportError::Io`] when the name is not being served, or
/// [`TransportError::Codec`] when the handshake is not understood.
pub async fn connect(
    name: &str,
    hello: Hello,
) -> Result<PipeClient<interprocess::local_socket::tokio::Stream>, TransportError> {
    let ns = name
        .to_ns_name::<GenericNamespaced>()
        .map_err(TransportError::Io)?;
    let conn = interprocess::local_socket::tokio::Stream::connect(ns)
        .await
        .map_err(TransportError::Io)?;
    let (mut reader, mut writer) = tokio::io::split(conn);
    let json = Codec::new(WireFormat::Json);
    write_body(&mut writer, &json.encode(&hello)?).await?;
    let ack: HelloAck = json.decode(&read_body(&mut reader).await?)?;
    Ok(PipeClient {
        reader,
        writer,
        codec: Codec::new(ack.format),
        ack,
    })
}

impl<S> PipeClient<S>
where
    S: AsyncRead + AsyncWrite,
{
    /// What the server said at the handshake.
    #[must_use]
    pub fn ack(&self) -> &HelloAck {
        &self.ack
    }

    /// The next frame, or `None` when the connection closes.
    pub async fn next_frame(&mut self) -> Option<Frame> {
        let body = read_body(&mut self.reader).await.ok()?;
        self.codec.decode(&body).ok()
    }

    /// Send a control request.
    ///
    /// # Errors
    ///
    /// [`TransportError::Io`] or [`TransportError::Codec`].
    pub async fn send(&mut self, request: &Request) -> Result<(), TransportError> {
        let body = self.codec.encode(request)?;
        write_body(&mut self.writer, &body).await
    }
}
