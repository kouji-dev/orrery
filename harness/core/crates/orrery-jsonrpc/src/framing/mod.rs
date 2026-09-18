//! Two wire formats, one enum.
//!
//! [`content_length`] came over **verbatim** from `ade/src-tauri/src/lsp/transport.rs`,
//! tests and all: sixty-five lines of correct `Content-Length` framing with
//! split-read, two-in-one-buffer, EOF and garbage-header coverage. There was no
//! reason to write it twice and every reason not to.
//!
//! What is new is [`Framing::LineDelimited`] — MCP stdio is newline-delimited
//! JSON, not LSP framing — and the async pair below. The ADE's readers are
//! synchronous over `BufRead`, which is right for a thread per server and wrong
//! for a host that has to stream a delta, hold a guest→broker callback open and
//! cancel one call out of three.

pub mod content_length;

use std::io;

use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
};

/// Refuse absurd bodies: a corrupt header must not make us allocate GBs.
///
/// The same ceiling [`content_length`] enforces. `framing.rs`'s
/// `a_body_bigger_than_the_ceiling_is_refused_rather_than_allocated` keeps the
/// two honest.
pub const MAX_BODY: usize = 64 * 1024 * 1024;

/// How messages are separated on a byte stream.
///
/// Both halves of the protocol are the same JSON; only the separator differs.
/// One enum rather than two transports means the client, the server, the
/// pending-map and the cancellation are written once.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Framing {
    /// `Content-Length: N\r\n\r\n<body>`. LSP, and the extension protocol.
    #[default]
    ContentLength,
    /// One JSON object per line. MCP over stdio.
    LineDelimited,
}

/// Read one message body, whatever the framing.
///
/// `Ok(None)` at a clean end of stream. `Err` when the stream ends inside a
/// message, or the framing is malformed.
///
/// # Errors
///
/// [`io::Error`] from the underlying stream, or `InvalidData` for a header that
/// is not a header, a missing or unparseable length, or a body over
/// [`MAX_BODY`].
pub async fn read_frame<R>(reader: &mut R, framing: Framing) -> io::Result<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    match framing {
        Framing::ContentLength => read_content_length(reader).await,
        Framing::LineDelimited => read_line_delimited(reader).await,
    }
}

async fn read_content_length<R>(reader: &mut R) -> io::Result<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    let mut content_length: Option<usize> = None;
    let mut line = String::new();
    let mut any = false;
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            if any {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "eof inside a message header",
                ));
            }
            return Ok(None);
        }
        any = true;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                let len: usize = value.trim().parse().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "bad Content-Length")
                })?;
                content_length = Some(len);
            }
            // Content-Type and anything else is ignored.
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("malformed header line {trimmed:?}"),
            ));
        }
    }
    let len = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    if len > MAX_BODY {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message too large ({len} bytes)"),
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

async fn read_line_delimited<R>(reader: &mut R) -> io::Result<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line).await?;
        if n == 0 {
            return Ok(None);
        }
        while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
            line.pop();
        }
        // A blank line between messages is not a message. Some servers emit
        // them; none mean anything by it.
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_BODY {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("message too large ({} bytes)", line.len()),
            ));
        }
        return Ok(Some(std::mem::take(&mut line)));
    }
}

/// Write one framed message and flush.
///
/// # Errors
///
/// [`io::Error`] from the stream, or `InvalidInput` when the body cannot be
/// expressed in the chosen framing — a newline inside a line-delimited body
/// would be read back as two messages, so it is refused here rather than
/// corrupting the stream.
pub async fn write_frame<W>(writer: &mut W, framing: Framing, body: &[u8]) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    match framing {
        Framing::ContentLength => {
            writer
                .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
                .await?;
            writer.write_all(body).await?;
        }
        Framing::LineDelimited => {
            if body.contains(&b'\n') {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a line-delimited body cannot contain a newline",
                ));
            }
            writer.write_all(body).await?;
            writer.write_all(b"\n").await?;
        }
    }
    writer.flush().await
}

/// A fixed-size window over the end of a stream.
///
/// A chatty child must not grow the host without bound, and the *end* of its
/// stderr is where the reason it died lives — so the ring keeps the tail and
/// drops the head. The ADE's client had this idea; this is the same idea
/// without the OS thread.
#[derive(Debug)]
pub struct StderrRing {
    capacity: usize,
    buffer: parking_lot::Mutex<Vec<u8>>,
}

impl StderrRing {
    /// A ring that keeps the last `capacity` bytes.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buffer: parking_lot::Mutex::new(Vec::with_capacity(capacity.min(4096))),
        }
    }

    /// Add bytes, dropping whatever no longer fits from the front.
    pub fn push(&self, bytes: &[u8]) {
        let mut buffer = self.buffer.lock();
        buffer.extend_from_slice(bytes);
        if buffer.len() > self.capacity {
            let drop_to = buffer.len() - self.capacity;
            buffer.drain(..drop_to);
        }
    }

    /// How many bytes it is holding. Never more than its capacity.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.lock().len()
    }

    /// Whether it has seen nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// What it is holding, as text. Invalid UTF-8 is replaced, never refused:
    /// this is a diagnostic, and a diagnostic that can fail is no diagnostic.
    #[must_use]
    pub fn tail(&self) -> String {
        String::from_utf8_lossy(&self.buffer.lock()).into_owned()
    }
}

/// Pump a child's stderr into a ring until it closes.
pub async fn pump_stderr<R>(mut reader: R, ring: std::sync::Arc<StderrRing>)
where
    R: AsyncRead + Unpin,
{
    let mut chunk = [0u8; 4096];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => ring.push(&chunk[..n]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Framing, StderrRing, read_frame, write_frame};

    #[tokio::test]
    async fn a_split_read_still_reassembles() {
        // The sync reader's own test covers this over `BufRead`; this is the
        // async path, over a stream that hands out four bytes at a time.
        let mut framed = Vec::new();
        write_frame(
            &mut framed,
            Framing::ContentLength,
            b"hello world, this is a body",
        )
        .await
        .unwrap();

        let (mut client, mut server) = tokio::io::duplex(4);
        let writer = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            server.write_all(&framed).await.unwrap();
            server.flush().await.unwrap();
        });

        let mut reader = tokio::io::BufReader::with_capacity(4, &mut client);
        let body = read_frame(&mut reader, Framing::ContentLength)
            .await
            .unwrap();
        assert_eq!(body.unwrap(), b"hello world, this is a body");
        writer.await.unwrap();
    }

    #[test]
    fn the_ring_keeps_the_end() {
        let ring = StderrRing::new(8);
        ring.push(b"0123456789");
        assert_eq!(ring.tail(), "23456789");
        assert_eq!(ring.len(), 8);
    }
}
