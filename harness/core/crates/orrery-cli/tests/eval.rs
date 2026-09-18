//! `orrery eval run | compare | replay`.
//!
//! `orrery-eval` shipped complete and the binary could not reach a line of it.
//! These tests drive the **binary**, against committed fixture streams — no
//! network, no key — and read back what it wrote.

mod common;

use common::{args, base, jsonl, orrery, workspace, CONTENTS, TARGET};

/// A suite file, and the fixture workspace its one case runs in.
///
/// `file-exists` rather than something cleverer on purpose: what is under test
/// here is the wiring — suite in, isolated workspace, runner, grader, report
/// out — and a grader that can fail for an interesting reason makes a wiring
/// failure look like a grading failure.
fn a_suite(dir: &std::path::Path) -> std::path::PathBuf {
    let archive = dir.join("archive");
    std::fs::create_dir_all(&archive).expect("the archive directory");
    std::fs::write(archive.join(TARGET), CONTENTS).expect("the archived file");

    let path = dir.join("smoke.toml");
    std::fs::write(
        &path,
        format!(
            r#"
name = "smoke"

[[case]]
id = "reads-the-file"
prompt = "what is in Cargo.toml?"

[case.workspace]
kind = "fixture"
archive = {archive:?}

[case.grade]
grader = "assertion"

[case.grade.config]
checks = [{{ kind = "file-exists", path = "{target}" }}]
"#,
            archive = archive.display().to_string(),
            target = TARGET,
        ),
    )
    .expect("the suite file");
    path
}

/// Run the suite and hand back the report as JSON.
fn run_suite(dir: &std::path::Path) -> serde_json::Value {
    let suite = a_suite(dir);
    let out = orrery(&args(
        &base(dir, &["text-turn.jsonl"]),
        &[
            "--json",
            "eval",
            "run",
            &suite.display().to_string(),
        ],
    ));
    assert_eq!(
        out.status.code(),
        Some(0),
        "a passing suite exits 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows = jsonl(&out.stdout);
    assert_eq!(rows.len(), 1, "one report on stdout, and nothing else");
    rows.into_iter().next().expect("the report")
}

/// The whole of task 9: a suite goes in, a graded report comes out.
#[test]
fn a_suite_runs_and_is_graded() {
    let dir = workspace();
    let report = run_suite(dir.path());

    assert!(report["run_id"].as_str().is_some(), "the run is named");
    assert_eq!(report["suite"], "smoke");
    let results = report["results"].as_array().expect("results");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["case"], "reads-the-file");
    assert_eq!(results[0]["outcome"], "pass");
    assert_eq!(
        results[0]["cost_provenance"]["kind"], "measured-at-provider-boundary",
        "the cost is the provider's own number, and says so"
    );
}

/// A suite that does not pass exits non-zero, with or without a baseline.
#[test]
fn a_failing_case_exits_non_zero() {
    let dir = workspace();
    let path = dir.path().join("red.toml");
    std::fs::write(
        &path,
        r#"
name = "red"

[[case]]
id = "cannot-pass"
prompt = "hello"

[case.workspace]
kind = "empty"

[case.grade]
grader = "assertion"

[case.grade.config]
checks = [{ kind = "file-exists", path = "never-written.txt" }]
"#,
    )
    .expect("the suite file");

    let out = orrery(&args(
        &base(dir.path(), &["text-turn.jsonl"]),
        &["--json", "eval", "run", &path.display().to_string()],
    ));
    assert_ne!(out.status.code(), Some(0), "a red suite is not green");
    assert_eq!(jsonl(&out.stdout)[0]["results"][0]["outcome"], "fail");
}

/// `--format junit` is the CI artefact, and it is XML on stdout.
#[test]
fn junit_is_an_artefact() {
    let dir = workspace();
    let suite = a_suite(dir.path());
    let out = orrery(&args(
        &base(dir.path(), &["text-turn.jsonl"]),
        &[
            "eval",
            "run",
            &suite.display().to_string(),
            "--format",
            "junit",
        ],
    ));
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("<?xml"), "it is XML: {text}");
    assert!(text.contains("<testsuite name=\"smoke\""), "{text}");
    assert!(text.contains("reads-the-file"), "{text}");

    let bad = orrery(&args(
        &base(dir.path(), &["text-turn.jsonl"]),
        &[
            "eval",
            "run",
            &suite.display().to_string(),
            "--format",
            "yaml",
        ],
    ));
    assert_eq!(bad.status.code(), Some(2), "an unknown format is usage");
}

/// Two runs of the same suite compare, and a run id that is not there is the
/// person's mistake.
#[test]
fn two_runs_compare() {
    let dir = workspace();
    let first = run_suite(dir.path());
    let second = run_suite(dir.path());
    let a = first["run_id"].as_str().expect("a run id");
    let b = second["run_id"].as_str().expect("a run id");
    assert_ne!(a, b);

    let out = orrery(&args(
        &base(dir.path(), &[]),
        &["--json", "eval", "compare", a, b],
    ));
    assert_eq!(
        out.status.code(),
        Some(0),
        "two identical runs have no regressions: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let comparison = jsonl(&out.stdout)[0].clone();
    assert!(comparison["regressions"].as_array().expect("regressions").is_empty());
    assert!(comparison["unmatched"].as_array().expect("unmatched").is_empty());

    let missing = orrery(&args(
        &base(dir.path(), &[]),
        &["eval", "compare", a, "run-nope"],
    ));
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty(), "stdout is data, and there is none");
}

/// `eval replay` re-opens one case's transcript out of a finished run.
#[test]
fn a_case_replays_out_of_a_run() {
    let dir = workspace();
    let report = run_suite(dir.path());
    let run = report["run_id"].as_str().expect("a run id");

    let out = orrery(&args(
        &base(dir.path(), &[]),
        &[
            "--json",
            "eval",
            "replay",
            run,
            "--case",
            "reads-the-file",
        ],
    ));
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let replayed = jsonl(&out.stdout)[0].clone();
    assert_eq!(replayed["case"], "reads-the-file");
    assert!(
        replayed["messages"].as_array().expect("messages").len() >= 2,
        "the prompt and what came back: {replayed}"
    );

    let missing = orrery(&args(
        &base(dir.path(), &[]),
        &["eval", "replay", run, "--case", "no-such-case"],
    ));
    assert_eq!(missing.status.code(), Some(2));
}

/// A suite file that is not there is usage, and names what it looked for.
#[test]
fn a_missing_suite_is_usage() {
    let dir = workspace();
    let out = orrery(&args(
        &base(dir.path(), &["text-turn.jsonl"]),
        &["eval", "run", "nope"],
    ));
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("nope"), "it quotes the name back: {stderr}");
}
