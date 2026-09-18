//! LSP base-protocol framing: `Content-Length: N\r\n(…)\r\n\r\n<body>`.
//!
//! Moved from `ade/src-tauri/src/lsp/transport.rs`, which is where the split-read
//! and two-messages-in-one-read cases were already worked out.
//!
//! # Why it is here and not in `orrery-jsonrpc`
//!
//! Plan 06 puts framing in `orrery-jsonrpc`, and that is the right home — but
//! `orrery-jsonrpc` is `publish = false`, and `cargo xtask deps-check` rule 2
//! forbids an extension from depending on an unpublished core crate. A
//! community author has to be able to build this crate against crates.io, so
//! the sixty lines live here. **When `orrery-jsonrpc` publishes, this module
//! should be deleted in favour of `orrery_jsonrpc::framing`**, and nothing else
//! in the crate changes: `Framed` is the only thing that touches bytes.

use std::io::{self, BufRead, Write};

/// Refuse absurd bodies: a corrupt header must not make us allocate gigabytes.
const MAX_BODY: usize = 64 * 1024 * 1024;

/// Read one message body.
///
/// `Ok(None)` at a clean end of stream — no header had started. An end of
/// stream *inside* a header or a body is an error, because those are a crashed
/// server rather than a finished one, and reporting them the same way is how a
/// hang gets mistaken for a clean exit.
///
/// # Errors
///
/// [`io::Error`] for a malformed header, a missing or unparseable
/// `Content-Length`, a body over [`MAX_BODY`], or a truncated stream.
pub fn read_message(r: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut content_length: Option<usize> = None;
    let mut line = String::new();
    let mut any = false;
    loop {
        line.clear();
        let n = r.read_line(&mut line)?;
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
            break;
        }
        let Some((name, value)) = trimmed.split_once(':') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("malformed header line {trimmed:?}"),
            ));
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            let len: usize = value.trim().parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "bad Content-Length")
            })?;
            content_length = Some(len);
        }
        // `Content-Type` and anything else is ignored, as the spec says to.
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
    r.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Write one message body with its header.
///
/// # Errors
///
/// [`io::Error`] when the sink refuses the bytes.
pub fn write_message(w: &mut impl Write, body: &[u8]) -> io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(body)?;
    w.flush()
}
