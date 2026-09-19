//! `orrery ledger` and `orrery telemetry` — the operator surface over the
//! audit stream.
//!
//! Section 8 phase 3 asks that every decision be logged. These tests run real
//! fixture turns, then read back the decisions those turns actually made — no
//! hand-written stream anywhere, because a ledger asserted against a file the
//! test wrote itself proves nothing about the harness.

mod common;

use common::{args, base, jsonl, orrery, workspace};

/// Run one turn, and hand back the session it wrote.
fn one_turn(dir: &std::path::Path, streams: &[&str]) -> String {
    let out = orrery(&args(&base(dir, streams), &["run", "-p", "read it"]));
    assert!(
        out.status.success(),
        "the fixture turn runs: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let listed = orrery(&args(&base(dir, &[]), &["--json", "session", "list"]));
    jsonl(&listed.stdout)[0]["session"]
        .as_str()
        .expect("an id")
        .to_owned()
}

/// A turn that calls a tool makes capability decisions, and they are readable.
#[test]
fn a_run_leaves_decisions_in_the_ledger() {
    let dir = workspace();
    one_turn(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);

    let out = orrery(&args(&base(dir.path(), &[]), &["ledger"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.trim().is_empty(), "the ledger is not empty");
    assert!(
        text.lines().any(|l| l.starts_with("allow")),
        "and it says which way each decision went: {text}"
    );
}

/// The same records, machine-readable, one JSON object per line.
#[test]
fn the_ledger_is_jsonl_on_request() {
    let dir = workspace();
    one_turn(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);

    let out = orrery(&args(&base(dir.path(), &[]), &["--json", "ledger"]));
    assert!(out.status.success());
    let rows = jsonl(&out.stdout);
    assert!(!rows.is_empty());
    for row in &rows {
        assert!(row["seq"].as_u64().is_some(), "every record is numbered");
        assert!(row["t"].as_str().is_some(), "and tagged: {row}");
    }
}

/// `--session` opens one run's stream, and naming a run that is not there is
/// the person's mistake rather than an empty success.
#[test]
fn a_session_filter_selects_one_run() {
    let dir = workspace();
    let first = one_turn(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);
    let second = one_turn(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);
    assert_ne!(first, second);

    let all = orrery(&args(&base(dir.path(), &[]), &["--json", "ledger"]));
    let one = orrery(&args(
        &base(dir.path(), &[]),
        &["--json", "ledger", "--session", &second],
    ));
    assert!(one.status.success());
    assert!(
        jsonl(&one.stdout).len() < jsonl(&all.stdout).len(),
        "one run's stream is shorter than two runs'"
    );

    let missing = orrery(&args(
        &base(dir.path(), &[]),
        &[
            "ledger",
            "--session",
            "00000000-0000-0000-0000-000000000000",
        ],
    ));
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        missing.stdout.is_empty(),
        "stdout is data, and there is none"
    );
}

/// `--subject` narrows to who asked. `agent` is the one every fixture turn has.
#[test]
fn a_subject_filter_narrows_the_ledger() {
    let dir = workspace();
    one_turn(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);

    let all = orrery(&args(&base(dir.path(), &[]), &["--json", "ledger"]));
    let agent = orrery(&args(
        &base(dir.path(), &[]),
        &["--json", "ledger", "--subject", "agent"],
    ));
    assert!(agent.status.success());
    assert!(!jsonl(&agent.stdout).is_empty(), "the agent made decisions");
    assert!(jsonl(&agent.stdout).len() <= jsonl(&all.stdout).len());

    let nobody = orrery(&args(
        &base(dir.path(), &[]),
        &["--json", "ledger", "--subject", "agent:nobody"],
    ));
    assert!(
        nobody.status.success(),
        "no matches is an answer, not an error"
    );
    assert!(nobody.stdout.is_empty());

    let bad = orrery(&args(
        &base(dir.path(), &[]),
        &["ledger", "--subject", "not-a-subject!"],
    ));
    assert_eq!(bad.status.code(), Some(2), "a malformed subject is usage");
}

/// `-n` keeps the tail, because an audit is read from the end.
#[test]
fn a_limit_keeps_the_latest() {
    let dir = workspace();
    one_turn(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);

    let all = orrery(&args(&base(dir.path(), &[]), &["--json", "ledger"]));
    let rows = jsonl(&all.stdout);
    assert!(rows.len() > 1, "there is something to trim");

    let tail = orrery(&args(
        &base(dir.path(), &[]),
        &["--json", "ledger", "-n", "1"],
    ));
    let trimmed = jsonl(&tail.stdout);
    assert_eq!(trimmed.len(), 1);
    assert_eq!(trimmed[0], *rows.last().expect("a last record"));
}

/// The two commands are disjoint views of the same file: a model request is
/// telemetry, a capability decision is not.
#[test]
fn telemetry_is_the_other_half_of_the_stream() {
    let dir = workspace();
    one_turn(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);

    let ledger = orrery(&args(&base(dir.path(), &[]), &["--json", "ledger"]));
    let telemetry = orrery(&args(&base(dir.path(), &[]), &["--json", "telemetry"]));
    assert!(telemetry.status.success());

    let tags = |out: &[u8]| -> Vec<String> {
        jsonl(out)
            .into_iter()
            .filter_map(|v| {
                v.get("t")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .collect()
    };
    let ledger_tags = tags(&ledger.stdout);
    let telemetry_tags = tags(&telemetry.stdout);
    assert!(
        ledger_tags.iter().all(|t| !telemetry_tags.contains(t)),
        "nothing appears in both: {ledger_tags:?} / {telemetry_tags:?}"
    );
    assert!(
        ledger_tags.iter().any(|t| t == "capability.decision"),
        "the evidence is in the ledger: {ledger_tags:?}"
    );
}

/// Nothing recorded yet is an answer, on stderr, with clean stdout.
#[test]
fn an_empty_state_dir_says_so() {
    let dir = workspace();
    let out = orrery(&args(&base(dir.path(), &[]), &["ledger"]));
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "stdout is data, and there is none");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("nothing recorded"),
        "and it says so"
    );
}

/// A tool call that never reached a tool is still a decision, and the ledger
/// says so.
///
/// The open question this round: a model naming a tool it was not offered
/// answers `no-such-tool` and the turn still exits 0. The exit code is right —
/// the model wrote the name, the failure goes back to it as a value, and the
/// next pass recovers — but the **silence** was not: a run whose only event was
/// a refused call left `audit/*.jsonl` holding two `model.request` lines and
/// `orrery ledger` answering "nothing matched". A call the harness refused to
/// dispatch is a decision the harness made, and section 8 phase 3 says every
/// decision is logged.
#[test]
fn a_tool_name_that_resolved_to_nothing_is_still_logged() {
    let dir = workspace();
    let invented = dir.path().join("invented.jsonl");
    std::fs::write(
        &invented,
        "{\"t\":\"started\",\"id\":\"msg_invented\"}\n\
         {\"t\":\"tool-use-start\",\"call\":\"0192f3a0-0000-7000-8000-0000000000c1\",\"name\":\"builtin.raed\"}\n\
         {\"t\":\"tool-use-delta\",\"call\":\"0192f3a0-0000-7000-8000-0000000000c1\",\"json_fragment\":\"{\\\"path\\\":\\\"Cargo.toml\\\"}\"}\n\
         {\"t\":\"tool-use-end\",\"call\":\"0192f3a0-0000-7000-8000-0000000000c1\"}\n\
         {\"t\":\"done\",\"stop\":\"tool-use\"}\n",
    )
    .expect("the invented-name stream");

    let mut flags = base(dir.path(), &[]);
    flags.push("--provider".to_owned());
    flags.push(format!("fixture:{}", invented.display()));
    flags.push("--provider".to_owned());
    flags.push(format!("fixture:{}", common::stream("text-turn.jsonl").display()));

    let run = orrery(&args(&flags, &["run", "-p", "call something that is not there"]));
    assert!(
        run.status.success(),
        "the turn completes — an invented name is the model's mistake, not a crash: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let out = orrery(&args(&base(dir.path(), &[]), &["--json", "ledger"]));
    let rows = jsonl(&out.stdout);
    let refused = rows
        .iter()
        .find(|r| r["t"] == "tool.call" && r["tool"] == "builtin.raed")
        .unwrap_or_else(|| panic!("the refused call is in the ledger: {rows:#?}"));
    assert_eq!(
        refused["outcome"], "failed",
        "and it is recorded as the failure it was: {refused}"
    );
}
