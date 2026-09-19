//! `orrery replay <session>` — a past session, re-emitted as AG-UI events.
//!
//! Plan 17 task 8's `replay::renders_a_past_session`. The session these tests
//! read is written by a real fixture turn — no network, no key — and the
//! assertion is against the event stream that same turn produced live.

mod common;

use common::{CONTENTS, FINAL_TEXT, args, base, event_types, jsonl, orrery, workspace};

/// Run one turn against a stream and hand back the session id and the frames.
fn a_live_turn(dir: &std::path::Path, streams: &[&str]) -> (String, Vec<String>) {
    let out = orrery(&args(
        &base(dir, streams),
        &["--json", "run", "-p", "hello"],
    ));
    assert!(
        out.status.success(),
        "the fixture turn runs: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let live = shape(&event_types(&out.stdout));

    let listed = orrery(&args(&base(dir, &[]), &["--json", "session", "list"]));
    let id = jsonl(&listed.stdout)[0]["session"]
        .as_str()
        .expect("an id")
        .to_owned();
    (id, live)
}

/// The frame kinds, with runs of the same kind collapsed.
///
/// **Cold replay re-encodes rows, not deltas.** A live turn emits one
/// `TEXT_MESSAGE_CONTENT` per chunk the provider streamed; the row that turn
/// wrote holds the finished text, so a replay emits one. That difference is the
/// design (see `cmd/replay.rs`), not a defect, and collapsing runs is how this
/// suite says so out loud: everything that is *not* chunking has to match
/// exactly.
fn shape(types: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in types {
        if out.last() != Some(t) {
            out.push(t.clone());
        }
    }
    out
}

/// The acceptance criterion: replay draws the same run the turn drew.
///
/// Not "some events": the same *kinds*, in the same order, with the same text
/// and the same tool result in them.
#[test]
fn renders_a_past_session() {
    let dir = workspace();
    let (id, live) = a_live_turn(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);

    let out = orrery(&args(&base(dir.path(), &[]), &["--json", "replay", &id]));
    assert!(
        out.status.success(),
        "replay runs: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let replayed = shape(&event_types(&out.stdout));
    assert_eq!(replayed, live, "replay re-emits the run the turn emitted");

    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains(CONTENTS.trim_end().lines().next().expect("a line")),
        "the tool result the turn really produced is in the replay: {text}"
    );
}

/// The same, for a turn that is nothing but prose.
#[test]
fn a_text_turn_replays_its_text() {
    let dir = workspace();
    let (id, live) = a_live_turn(dir.path(), &["text-turn.jsonl"]);

    let out = orrery(&args(&base(dir.path(), &[]), &["--json", "replay", &id]));
    assert!(out.status.success());
    assert_eq!(shape(&event_types(&out.stdout)), live);

    let joined: String = jsonl(&out.stdout)
        .iter()
        .filter_map(|v| v.get("delta").and_then(serde_json::Value::as_str))
        .collect();
    assert_eq!(joined, FINAL_TEXT, "word for word");
}

/// **Replay needs no model.** A past session is on disk; asking to see it must
/// not need a provider, a key or a network.
#[test]
fn replay_needs_no_provider() {
    let dir = workspace();
    let (id, _) = a_live_turn(dir.path(), &["text-turn.jsonl"]);

    let out = orrery(&args(&base(dir.path(), &[]), &["--json", "replay", &id]));
    assert!(
        out.status.success(),
        "no --provider was passed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.stdout.is_empty());
}

/// A session that is not there is the person's mistake, not a crash.
#[test]
fn an_unknown_session_is_usage() {
    let dir = workspace();
    a_live_turn(dir.path(), &["text-turn.jsonl"]);

    let out = orrery(&args(
        &base(dir.path(), &[]),
        &["replay", "00000000-0000-0000-0000-000000000000"],
    ));
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "stdout is data, and there is none");
}
