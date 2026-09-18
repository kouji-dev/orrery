//! `Content-Length` framing: the one part of this crate a fake server cannot
//! stand in for.
//!
//! These are the cases that actually bite, and they are unit-testable over a
//! `&[u8]` precisely because the framer is pure over `BufRead`/`Write` rather
//! than over a process.

use std::io::BufReader;

use orrery_ext_lsp::framing::{read_message, write_message};

fn framed(bodies: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for body in bodies {
        write_message(&mut out, body.as_bytes()).expect("a Vec always takes bytes");
    }
    out
}

#[test]
fn a_round_trip_is_a_round_trip() {
    let bytes = framed(&[r#"{"jsonrpc":"2.0","id":1}"#]);
    assert!(
        String::from_utf8_lossy(&bytes).starts_with("Content-Length: 24\r\n\r\n"),
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let mut r = BufReader::new(&bytes[..]);
    let body = read_message(&mut r).expect("read").expect("a message");
    assert_eq!(body, br#"{"jsonrpc":"2.0","id":1}"#);
}

#[test]
fn two_messages_in_one_buffer_are_two_messages() {
    // A server writes as fast as it can and a single read returns both. A
    // framer that assumed one read is one message would lose the second.
    let bytes = framed(&[r#"{"id":1}"#, r#"{"id":2}"#]);
    let mut r = BufReader::new(&bytes[..]);
    assert_eq!(read_message(&mut r).unwrap().unwrap(), br#"{"id":1}"#);
    assert_eq!(read_message(&mut r).unwrap().unwrap(), br#"{"id":2}"#);
    assert!(read_message(&mut r).unwrap().is_none());
}

#[test]
fn a_clean_end_is_none_and_a_torn_one_is_an_error() {
    // Nothing at all: the server exited between messages, which is not a fault.
    let mut r = BufReader::new(&b""[..]);
    assert!(read_message(&mut r).unwrap().is_none());

    // Ending *inside* a header is a crashed server, and reporting it the same
    // way is how a hang gets mistaken for a clean exit.
    let mut r = BufReader::new(&b"Content-Length: 12\r\n"[..]);
    assert!(read_message(&mut r).is_err());

    // Ending inside the body, likewise.
    let mut r = BufReader::new(&b"Content-Length: 12\r\n\r\nshort"[..]);
    assert!(read_message(&mut r).is_err());
}

#[test]
fn other_headers_are_ignored_and_a_malformed_one_is_not() {
    let mut framed = Vec::new();
    framed.extend_from_slice(
        b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: 2\r\n\r\n{}",
    );
    let mut r = BufReader::new(&framed[..]);
    assert_eq!(read_message(&mut r).unwrap().unwrap(), b"{}");

    let mut r = BufReader::new(&b"this is not a header\r\n\r\n"[..]);
    assert!(read_message(&mut r).is_err());
}

#[test]
fn a_missing_or_absurd_content_length_is_refused() {
    let mut r = BufReader::new(&b"Content-Type: x\r\n\r\n{}"[..]);
    assert!(read_message(&mut r).is_err(), "no Content-Length");

    let mut r = BufReader::new(&b"Content-Length: nope\r\n\r\n"[..]);
    assert!(read_message(&mut r).is_err(), "unparseable");

    // A corrupt header must not make us allocate gigabytes on the way to
    // failing.
    let mut r = BufReader::new(&b"Content-Length: 999999999999\r\n\r\n"[..]);
    assert!(read_message(&mut r).is_err(), "over the ceiling");
}

#[test]
fn a_body_that_is_not_ascii_survives_byte_for_byte() {
    // The length is in **bytes**, not characters. Counting characters is the
    // classic way a client desynchronises on the first non-English identifier.
    let body = r#"{"message":"変数が見つかりません"}"#;
    let bytes = framed(&[body]);
    let mut r = BufReader::new(&bytes[..]);
    let read = read_message(&mut r).unwrap().unwrap();
    assert_eq!(read, body.as_bytes());
    assert!(body.len() > body.chars().count());
}
