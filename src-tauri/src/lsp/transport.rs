//! LSP base-protocol framing: `Content-Length: N\r\n(...)\r\n\r\n<body>`.
//! Pure over `BufRead`/`Write` so the split-read / two-in-one / EOF cases are
//! unit-testable without a process.

use std::io::{self, BufRead, Write};

/// Refuse absurd bodies (a corrupt header must not make us allocate GBs).
const MAX_BODY: usize = 64 * 1024 * 1024;

/// Read one message body. `Ok(None)` at a clean EOF (no header started);
/// `UnexpectedEof` when the stream ends inside a header or a body.
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
    r.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Write one framed message and flush.
pub fn write_message(w: &mut impl Write, body: &[u8]) -> io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(body)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor, Read};

    /// A reader that hands out at most `chunk` bytes per `read` — simulates
    /// a pipe delivering a message in pieces.
    struct Chunked {
        inner: Cursor<Vec<u8>>,
        chunk: usize,
    }

    impl Read for Chunked {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.chunk);
            self.inner.read(&mut buf[..n])
        }
    }

    fn framed(body: &str) -> Vec<u8> {
        let mut v = Vec::new();
        write_message(&mut v, body.as_bytes()).unwrap();
        v
    }

    #[test]
    fn round_trips_one_message() {
        let bytes = framed(r#"{"a":1}"#);
        assert!(bytes.starts_with(b"Content-Length: 7\r\n\r\n"));
        let mut r = BufReader::new(Cursor::new(bytes));
        assert_eq!(read_message(&mut r).unwrap().unwrap(), br#"{"a":1}"#);
        assert!(read_message(&mut r).unwrap().is_none(), "clean eof");
    }

    #[test]
    fn split_reads_reassemble() {
        let bytes = framed("hello world, this is a body");
        for chunk in [1, 3, 7] {
            let mut r = BufReader::with_capacity(
                4,
                Chunked {
                    inner: Cursor::new(bytes.clone()),
                    chunk,
                },
            );
            assert_eq!(
                read_message(&mut r).unwrap().unwrap(),
                b"hello world, this is a body",
                "chunk {chunk}"
            );
        }
    }

    #[test]
    fn two_messages_in_one_buffer() {
        let mut bytes = framed("one");
        bytes.extend(framed("second"));
        let mut r = BufReader::new(Cursor::new(bytes));
        assert_eq!(read_message(&mut r).unwrap().unwrap(), b"one");
        assert_eq!(read_message(&mut r).unwrap().unwrap(), b"second");
        assert!(read_message(&mut r).unwrap().is_none());
    }

    #[test]
    fn extra_headers_and_case_are_tolerated() {
        let bytes = b"content-type: application/vscode-jsonrpc; charset=utf-8\r\ncontent-length: 2\r\nX-Foo: bar\r\n\r\n{}".to_vec();
        let mut r = BufReader::new(Cursor::new(bytes));
        assert_eq!(read_message(&mut r).unwrap().unwrap(), b"{}");
    }

    #[test]
    fn eof_mid_body_and_bad_headers_error() {
        let mut bytes = framed("0123456789");
        bytes.truncate(bytes.len() - 4);
        let mut r = BufReader::new(Cursor::new(bytes));
        let e = read_message(&mut r).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
        // eof inside the header block
        let mut r = BufReader::new(Cursor::new(b"Content-Length: 3\r\n".to_vec()));
        assert_eq!(read_message(&mut r).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        // no Content-Length at all
        let mut r = BufReader::new(Cursor::new(b"X: y\r\n\r\n{}".to_vec()));
        assert_eq!(read_message(&mut r).unwrap_err().kind(), io::ErrorKind::InvalidData);
        // garbage header line
        let mut r = BufReader::new(Cursor::new(b"garbage\r\n\r\n".to_vec()));
        assert_eq!(read_message(&mut r).unwrap_err().kind(), io::ErrorKind::InvalidData);
        // non-numeric length
        let mut r = BufReader::new(Cursor::new(b"Content-Length: x\r\n\r\n".to_vec()));
        assert_eq!(read_message(&mut r).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
