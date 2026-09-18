//! Task 5: the SSE parser. Recorded fixtures only — no test makes a network
//! request.

use std::path::{Path, PathBuf};

use orrery_ext_provider_anthropic::sse::SseParser;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn parse_all(bytes: &[u8], chunk: usize) -> Vec<(String, String)> {
    let mut p = SseParser::new();
    let mut out = Vec::new();
    for part in bytes.chunks(chunk) {
        out.extend(
            p.push(part)
                .expect("well-formed")
                .into_iter()
                .map(|e| (e.name, e.data)),
        );
    }
    out.extend(p.finish().into_iter().map(|e| (e.name, e.data)));
    out
}

#[test]
fn parses_recorded_stream() {
    let raw = read("text-turn.sse");
    let events = parse_all(&raw, raw.len());
    let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "message_start",
            "ping",
            "content_block_start",
            "content_block_delta",
            "content_block_delta",
            "content_block_stop",
            "ping",
            "message_delta",
            "message_stop",
        ]
    );
    assert!(
        events[3].1.contains("The workspace has "),
        "{:?}",
        events[3]
    );
}

#[test]
fn handles_split_frames() {
    for name in [
        "text-turn.sse",
        "tool-call.sse",
        "max-tokens.sse",
        "overloaded.sse",
    ] {
        let raw = read(name);
        let whole = parse_all(&raw, raw.len());
        for chunk in [1usize, 7, 13, 64] {
            assert_eq!(
                parse_all(&raw, chunk),
                whole,
                "{name} in {chunk}-byte chunks"
            );
        }
    }
}

#[test]
fn ignores_ping_and_comment_lines() {
    // Comments (`:` lines) vanish; `ping` is a real named event and must not,
    // because a stream that only pings is still alive and the kernel's idle
    // timeout needs to know.
    let events = parse_all(b": hello\n: world\n\nevent: ping\ndata: {}\n\n", 3);
    assert_eq!(events, vec![("ping".to_owned(), "{}".to_owned())]);
}

#[test]
fn crlf_and_multi_line_data_and_a_missing_event_name() {
    let events = parse_all(b"data: one\r\ndata: two\r\n\r\n", 5);
    // No `event:` field means the default name, and `data:` lines join with a
    // newline — both plain SSE, both things a hand-rolled parser gets wrong.
    assert_eq!(events, vec![("message".to_owned(), "one\ntwo".to_owned())]);
}

#[test]
fn a_trailing_frame_without_a_blank_line_is_still_delivered() {
    let mut p = SseParser::new();
    assert!(
        p.push(b"event: message_stop\ndata: {}\n")
            .expect("ok")
            .is_empty()
    );
    let tail = p.finish();
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].name, "message_stop");
}

#[test]
fn invalid_utf8_is_an_error_not_a_panic() {
    let mut p = SseParser::new();
    assert!(p.push(b"data: \xff\xfe\n\n").is_err());
}
