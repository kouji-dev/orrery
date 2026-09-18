//! One enum serves both wire formats.

use orrery_jsonrpc::framing::{Framing, read_frame, write_frame};
use tokio::io::BufReader;

#[tokio::test]
async fn a_newline_delimited_stream_parses() {
    // MCP stdio is newline-delimited JSON, not LSP framing.
    let bytes = b"{\"a\":1}\n{\"b\":2}\n".to_vec();
    let mut reader = BufReader::new(std::io::Cursor::new(bytes));

    assert_eq!(
        read_frame(&mut reader, Framing::LineDelimited)
            .await
            .unwrap(),
        Some(br#"{"a":1}"#.to_vec())
    );
    assert_eq!(
        read_frame(&mut reader, Framing::LineDelimited)
            .await
            .unwrap(),
        Some(br#"{"b":2}"#.to_vec())
    );
    assert_eq!(
        read_frame(&mut reader, Framing::LineDelimited)
            .await
            .unwrap(),
        None,
        "a clean eof is None, not an error"
    );
}

#[tokio::test]
async fn blank_lines_are_not_messages() {
    let mut reader = BufReader::new(std::io::Cursor::new(b"\n\n{\"a\":1}\n\n".to_vec()));
    assert_eq!(
        read_frame(&mut reader, Framing::LineDelimited)
            .await
            .unwrap(),
        Some(br#"{"a":1}"#.to_vec())
    );
    assert_eq!(
        read_frame(&mut reader, Framing::LineDelimited)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn a_newline_inside_a_body_would_be_a_second_message() {
    // Not a bug, a property of the format, and the writer is what has to
    // respect it: `write_frame` refuses a body with a newline in it rather
    // than emitting a stream that cannot be read back.
    let mut out = Vec::new();
    let err = write_frame(&mut out, Framing::LineDelimited, b"{\"a\":\n1}")
        .await
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[tokio::test]
async fn the_async_reader_and_the_moved_in_sync_one_agree() {
    // `content_length.rs` came over verbatim from the ADE with its tests. This
    // asserts the async reader added beside it reads exactly what that writer
    // writes — the two must not drift.
    let mut framed = Vec::new();
    orrery_jsonrpc::framing::content_length::write_message(&mut framed, br#"{"hello":"world"}"#)
        .unwrap();

    let mut reader = BufReader::new(std::io::Cursor::new(framed.clone()));
    assert_eq!(
        read_frame(&mut reader, Framing::ContentLength)
            .await
            .unwrap(),
        Some(br#"{"hello":"world"}"#.to_vec())
    );

    // And the other direction.
    let mut written = Vec::new();
    write_frame(
        &mut written,
        Framing::ContentLength,
        br#"{"hello":"world"}"#,
    )
    .await
    .unwrap();
    assert_eq!(written, framed);

    let mut sync = std::io::BufReader::new(std::io::Cursor::new(written));
    assert_eq!(
        orrery_jsonrpc::framing::content_length::read_message(&mut sync).unwrap(),
        Some(br#"{"hello":"world"}"#.to_vec())
    );
}

#[tokio::test]
async fn a_body_bigger_than_the_ceiling_is_refused_rather_than_allocated() {
    let mut reader = BufReader::new(std::io::Cursor::new(
        b"Content-Length: 99999999999\r\n\r\n".to_vec(),
    ));
    let err = read_frame(&mut reader, Framing::ContentLength)
        .await
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}
