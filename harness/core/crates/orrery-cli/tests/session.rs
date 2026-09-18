//! `orrery session list | show | rm`.
//!
//! The sessions these tests read are made by running real turns against the
//! committed fixture streams — no network, no key — so `session list` is
//! asserted against history the binary itself wrote.

mod common;

use common::{args, base, event_types, jsonl, orrery, workspace, FINAL_TEXT};

/// Run one turn, so there is a session on disk to list.
fn one_turn(dir: &std::path::Path, stream: &str) {
    let out = orrery(&args(&base(dir, &[stream]), &["run", "-p", "hello"]));
    assert!(
        out.status.success(),
        "the fixture turn runs: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Plan 17 task 7: `session list` shows what is in the store.
#[test]
fn list_shows_the_stored_sessions() {
    let dir = workspace();
    one_turn(dir.path(), "text-turn.jsonl");
    one_turn(dir.path(), "text-turn.jsonl");

    let out = orrery(&args(&base(dir.path(), &[]), &["session", "list"]));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text.lines().count(),
        2,
        "one line per session, and nothing else on stdout: {text}"
    );
    assert!(
        text.contains(&dir.path().display().to_string()),
        "the workspace is named: {text}"
    );
}

/// The same listing, machine-readable, with the fields a person scans by.
#[test]
fn list_is_jsonl_on_request() {
    let dir = workspace();
    one_turn(dir.path(), "text-turn.jsonl");

    let out = orrery(&args(&base(dir.path(), &[]), &["--json", "session", "list"]));
    assert!(out.status.success());
    let rows = jsonl(&out.stdout);
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["session"].as_str().is_some());
    assert_eq!(rows[0]["profile"], "default");
    assert!(
        rows[0]["turns"].as_u64().is_some_and(|t| t >= 2),
        "a turn is a user row and an assistant row at least: {}",
        rows[0]
    );
    assert!(rows[0]["created_at"].as_i64().is_some());
}

/// `session show` is a `materialise` away, and prints the transcript.
#[test]
fn show_prints_the_transcript() {
    let dir = workspace();
    one_turn(dir.path(), "text-turn.jsonl");

    let listed = orrery(&args(&base(dir.path(), &[]), &["--json", "session", "list"]));
    let id = jsonl(&listed.stdout)[0]["session"]
        .as_str()
        .expect("an id")
        .to_owned();

    let out = orrery(&args(&base(dir.path(), &[]), &["session", "show", &id]));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("hello"), "the prompt is in it: {text}");
    assert!(
        text.contains(FINAL_TEXT),
        "and so is what the model said: {text}"
    );
}

/// A session id that is not in the store is the person's mistake, not a crash.
#[test]
fn show_of_an_unknown_session_is_usage() {
    let dir = workspace();
    one_turn(dir.path(), "text-turn.jsonl");

    let out = orrery(&args(
        &base(dir.path(), &[]),
        &["session", "show", "00000000-0000-0000-0000-000000000000"],
    ));
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "stdout is data, and there is none");

    let malformed = orrery(&args(&base(dir.path(), &[]), &["session", "show", "nope"]));
    assert_eq!(malformed.status.code(), Some(2));
    let err = String::from_utf8_lossy(&malformed.stderr);
    assert!(err.contains("nope"), "the id is quoted back: {err}");
}

/// Nothing stored yet is an answer, not an error — and it creates no database.
#[test]
fn an_empty_store_lists_nothing() {
    let dir = workspace();
    let out = orrery(&args(&base(dir.path(), &[]), &["session", "list"]));
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "stdout is data, and there is none");
    assert!(
        !dir.path().join(".orrery/sessions.db").exists(),
        "listing nothing does not create a store"
    );
}

/// `run` is unaffected by any of this: the listing reads what a turn wrote.
#[test]
fn a_listed_session_is_the_one_the_turn_wrote() {
    let dir = workspace();
    let out = orrery(&args(
        &base(dir.path(), &["text-turn.jsonl"]),
        &["--json", "run", "-p", "hello"],
    ));
    assert!(out.status.success());
    assert!(event_types(&out.stdout).iter().any(|t| t == "RUN_STARTED"));

    let listed = orrery(&args(&base(dir.path(), &[]), &["--json", "session", "list"]));
    assert_eq!(jsonl(&listed.stdout).len(), 1);
}
