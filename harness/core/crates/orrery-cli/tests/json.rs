//! `orrery run` — **the phase-1 acceptance criterion**: a turn completes end
//! to end in a terminal.
//!
//! The model is two committed `.jsonl` streams: pass one asks for
//! `builtin.read`, pass two answers in prose. Nothing here reaches the network
//! and nothing needs a key.

mod common;

use common::{
    CONTENTS, FINAL_TEXT, TARGET, args, base, event_types, jsonl, orrery, orrery_with_stdin,
};

/// **The phase-1 criterion.** One turn, one real tool call, the final text on
/// stdout, exit 0.
#[test]
fn completes_a_turn_with_a_tool_call() {
    let dir = common::workspace();
    let base = base(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);
    let out = orrery(&args(&base, &["run", "-p", "what is in Cargo.toml?"]));

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "exit 0; stderr was: {stderr}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        FINAL_TEXT,
        "the final text, and only the final text, on stdout"
    );
}

/// …and the tool really read the file, through the broker, under a token the
/// policy engine minted. `--json` is where that is visible.
#[test]
fn the_tool_call_really_happened() {
    let dir = common::workspace();
    let base = base(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);
    let out = orrery(&args(&base, &["run", "-p", "read it", "--json"]));

    let types = event_types(&out.stdout);
    for expected in [
        "RUN_STARTED",
        "TEXT_MESSAGE_START",
        "TOOL_CALL_START",
        "TOOL_CALL_END",
        "TOOL_CALL_RESULT",
        "TEXT_MESSAGE_END",
        "RUN_FINISHED",
    ] {
        assert!(
            types.iter().any(|t| t == expected),
            "`{expected}` is missing from {types:?}"
        );
    }

    let frames = jsonl(&out.stdout);
    let result = frames
        .iter()
        .find(|f| f.get("type").and_then(serde_json::Value::as_str) == Some("TOOL_CALL_RESULT"))
        .expect("the tool call settled");
    let content = result
        .get("content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(
        content.contains("the-file-the-tool-really-read"),
        "the bytes in the event are the bytes on disk: {content}"
    );
    assert!(
        CONTENTS.contains("the-file-the-tool-really-read") && !TARGET.is_empty(),
        "the fixture still reads the file this test writes"
    );
}

/// Every stdout line is valid JSON and carries an AG-UI event.
#[test]
fn json_is_parseable() {
    let dir = common::workspace();
    let base = base(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);
    let out = orrery(&args(&base, &["run", "-p", "read it", "--json"]));

    let frames = jsonl(&out.stdout);
    assert!(!frames.is_empty(), "something was emitted");
    for frame in &frames {
        assert!(
            frame.get("seq").and_then(serde_json::Value::as_u64).is_some(),
            "every frame carries a seq: {frame}"
        );
        assert!(
            frame.get("type").and_then(serde_json::Value::as_str).is_some(),
            "every frame carries an event type: {frame}"
        );
    }
    let seqs: Vec<u64> = frames
        .iter()
        .filter_map(|f| f.get("seq").and_then(serde_json::Value::as_u64))
        .collect();
    let mut sorted = seqs.clone();
    sorted.sort_unstable();
    assert_eq!(seqs, sorted, "seq is monotonic: {seqs:?}");
}

/// `-vv` narrates on stderr and leaves stdout parseable
/// (`17-cli.md`, streams discipline).
#[test]
fn stderr_does_not_pollute_stdout() {
    let dir = common::workspace();
    let base = base(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);
    let out = orrery(&args(&base, &["run", "-p", "read it", "--json", "-vv"]));

    let frames = jsonl(&out.stdout);
    assert!(!frames.is_empty(), "stdout still has the events");
    assert!(
        !out.stderr.is_empty(),
        "-vv actually said something, or this test proves nothing"
    );
}

/// Bare `orrery` with stdout piped picks the json renderer (`17-cli.md` task 2),
/// and reads one prompt per line.
#[test]
fn defaults_to_json_without_a_tty() {
    let dir = common::workspace();
    let base = base(dir.path(), &["text-turn.jsonl"]);
    let out = orrery_with_stdin(&base, "what is here?\n");

    let types = event_types(&out.stdout);
    assert!(
        types.iter().any(|t| t == "RUN_STARTED"),
        "a piped `orrery` emits events: {types:?}"
    );
    assert!(
        types.iter().any(|t| t == "RUN_FINISHED"),
        "…and finishes the run: {types:?}"
    );
}

/// The tool's **input** is on the wire, not only its result.
///
/// A json renderer used to see `TOOL_CALL_START` and then `TOOL_CALL_END` with
/// nothing between them: the fixture's `tool-use-delta` fragments were parsed
/// into the call's input and dropped. A client could show that a tool ran and
/// never what it was asked to do.
#[test]
fn the_tool_input_is_reconstructible() {
    let dir = common::workspace();
    let base = base(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);
    let out = orrery(&args(&base, &["run", "-p", "read it", "--json"]));

    let frames = jsonl(&out.stdout);
    let start = frames
        .iter()
        .find(|f| f.get("type").and_then(serde_json::Value::as_str) == Some("TOOL_CALL_START"))
        .expect("the call started");
    let call = start
        .get("toolCallId")
        .and_then(serde_json::Value::as_str)
        .expect("the call has an id");

    let rebuilt: String = frames
        .iter()
        .filter(|f| f.get("type").and_then(serde_json::Value::as_str) == Some("TOOL_CALL_ARGS"))
        .filter(|f| f.get("toolCallId").and_then(serde_json::Value::as_str) == Some(call))
        .filter_map(|f| f.get("delta").and_then(serde_json::Value::as_str))
        .collect();
    assert!(
        !rebuilt.is_empty(),
        "the arguments are on the wire: {:?}",
        event_types(&out.stdout)
    );
    let input: serde_json::Value =
        serde_json::from_str(&rebuilt).expect("the fragments rebuild into the input");
    assert_eq!(
        input.get("path").and_then(serde_json::Value::as_str),
        Some(TARGET),
        "and the input is the one the tool ran with: {input}"
    );
}
