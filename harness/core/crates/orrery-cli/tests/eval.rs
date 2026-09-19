//! `orrery eval run | compare | replay`.
//!
//! `orrery-eval` shipped complete and the binary could not reach a line of it.
//! These tests drive the **binary**, against committed fixture streams — no
//! network, no key — and read back what it wrote.

mod common;

use common::{CONTENTS, TARGET, args, base, home_with, jsonl, orrery, orrery_in, workspace};

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
        &["--json", "eval", "run", &suite.display().to_string()],
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
    assert!(
        comparison["regressions"]
            .as_array()
            .expect("regressions")
            .is_empty()
    );
    assert!(
        comparison["unmatched"]
            .as_array()
            .expect("unmatched")
            .is_empty()
    );

    let missing = orrery(&args(
        &base(dir.path(), &[]),
        &["eval", "compare", a, "run-nope"],
    ));
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        missing.stdout.is_empty(),
        "stdout is data, and there is none"
    );
}

/// `eval replay` re-opens one case's transcript out of a finished run.
#[test]
fn a_case_replays_out_of_a_run() {
    let dir = workspace();
    let report = run_suite(dir.path());
    let run = report["run_id"].as_str().expect("a run id");

    let out = orrery(&args(
        &base(dir.path(), &[]),
        &["--json", "eval", "replay", run, "--case", "reads-the-file"],
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

/// A fake competing agent CLI, written into `dir`.
///
/// **No real agent CLI is ever started and no key exists.** This writes the
/// file the case is graded on and prints the JSON shape `claude --print
/// --output-format json` prints, which is all an adapter reads. On Windows it
/// is a `.cmd`, which is also what exercises the ported shim handling:
/// `CreateProcessW` cannot start one, so `launch_prefix` has to route it
/// through `cmd.exe /c call` or nothing runs at all.
fn fake_agent(dir: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).expect("the adapter directory");
    if cfg!(windows) {
        let path = dir.join("fake-agent.cmd");
        std::fs::write(
            &path,
            "@echo off\r\n\
             echo written> out.txt\r\n\
             echo {\"type\":\"result\",\"usage\":{\"input_tokens\":1200,\"output_tokens\":340},\"total_cost_usd\":0.0042}\r\n",
        )
        .expect("the fake agent");
        path
    } else {
        let path = dir.join("fake-agent");
        std::fs::write(
            &path,
            "#!/bin/sh\n\
             echo written > out.txt\n\
             echo '{\"type\":\"result\",\"usage\":{\"input_tokens\":1200,\"output_tokens\":340},\"total_cost_usd\":0.0042}'\n",
        )
        .expect("the fake agent");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        path
    }
}

/// A suite with two of our profiles' worth of cases and one competitor.
fn a_cross_suite(dir: &std::path::Path) -> std::path::PathBuf {
    let agent = fake_agent(&dir.join("bin"));
    let path = dir.join("cross.toml");
    std::fs::write(
        &path,
        format!(
            r#"
name = "cross"

[adapter.fake-agent]
command = "{agent} --print"
parse = "json"

[[case]]
id = "writes-a-file"
prompt = "write out.txt"

[case.workspace]
kind = "empty"

[case.grade]
grader = "assertion"

[case.grade.config]
checks = [{{ kind = "file-exists", path = "out.txt" }}]
"#,
            agent = agent.display().to_string().replace('\\', "\\\\"),
        ),
    )
    .expect("the suite file");
    path
}

/// The two profiles of ours the cross suite is run over. They differ in nothing
/// but their name in this build, which the command says out loud: the fixture
/// provider is the only selectable one, so a second profile is a second label.
const TWO_PROFILES: &str = "[profile.fast]\nmodel = \"small\"\n\n[profile.careful]\nmodel = \"big\"\n";

/// Run the cross suite over two of our profiles and the declared competitor.
fn run_cross(home: &std::path::Path, dir: &std::path::Path) -> serde_json::Value {
    let suite = a_cross_suite(dir);
    let out = orrery_in(
        home,
        &args(
            &base(dir, &["text-turn.jsonl"]),
            &[
                "--json",
                "eval",
                "run",
                &suite.display().to_string(),
                "--profile",
                "fast",
                "--profile",
                "careful",
            ],
        ),
    );
    // Our two profiles run the fixture stream and write nothing, so they fail
    // the check the competitor passes. That is the suite doing its job; what is
    // under test is that all three ran and are labelled.
    jsonl(&out.stdout)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no report: {}", String::from_utf8_lossy(&out.stderr)))
}

/// The half of phase 9 that was built and unreachable: a competing harness in
/// the same report as ours, driven from the binary by a suite that names it.
#[test]
fn a_suite_puts_a_competing_harness_beside_ours() {
    let home = home_with(TWO_PROFILES);
    let dir = workspace();
    let report = run_cross(home.path(), dir.path());

    let results = report["results"].as_array().expect("results");
    assert_eq!(results.len(), 3, "two profiles and one competitor: {report}");

    let profiles: Vec<&str> = results
        .iter()
        .map(|r| r["profile"].as_str().expect("a profile"))
        .collect();
    assert!(profiles.contains(&"fast"), "{profiles:?}");
    assert!(profiles.contains(&"careful"), "{profiles:?}");
    assert!(profiles.contains(&"fake-agent"), "{profiles:?}");

    let theirs = results
        .iter()
        .find(|r| r["profile"] == "fake-agent")
        .expect("the competitor's result");
    assert_eq!(
        theirs["cost_provenance"]["kind"], "reported-by-tool",
        "their number is theirs, and says so: {theirs}"
    );
    assert_eq!(theirs["cost_provenance"]["tool"], "fake-agent");
    assert_eq!(theirs["cost"]["input_tokens"], 1200);
    assert_eq!(theirs["cost"]["micro_usd"], 4200);
    assert_eq!(
        theirs["outcome"], "pass",
        "the competitor wrote the file the same grader looks for"
    );

    for profile in ["fast", "careful"] {
        let mine = results
            .iter()
            .find(|r| r["profile"] == profile)
            .unwrap_or_else(|| panic!("no result for {profile}"));
        assert_eq!(
            mine["cost_provenance"]["kind"], "measured-at-provider-boundary",
            "ours is read at the provider boundary: {mine}"
        );
    }
}

/// The text report, which is what a person actually reads, keeps the two kinds
/// of number apart in words.
#[test]
fn the_text_report_labels_whose_number_each_one_is() {
    let home = home_with(TWO_PROFILES);
    let dir = workspace();
    let suite = a_cross_suite(dir.path());
    let out = orrery_in(
        home.path(),
        &args(
            &base(dir.path(), &["text-turn.jsonl"]),
            &[
                "eval",
                "run",
                &suite.display().to_string(),
                "--profile",
                "fast",
                "--format",
                "text",
            ],
        ),
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("reported by fake-agent"), "{text}");
    assert!(text.contains("measured at the provider boundary"), "{text}");
}

/// And two such runs compare, from the binary, with the competitor in both.
#[test]
fn two_cross_runs_compare() {
    let home = home_with(TWO_PROFILES);
    let dir = workspace();
    let a = run_cross(home.path(), dir.path())["run_id"]
        .as_str()
        .expect("a run id")
        .to_owned();
    let b = run_cross(home.path(), dir.path())["run_id"]
        .as_str()
        .expect("a run id")
        .to_owned();
    assert_ne!(a, b);

    let out = orrery_in(
        home.path(),
        &args(
            &base(dir.path(), &[]),
            &["--json", "eval", "compare", &a, &b],
        ),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "two identical cross runs have no regressions: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let comparison = jsonl(&out.stdout)[0].clone();
    assert!(
        comparison["unmatched"]
            .as_array()
            .expect("unmatched")
            .is_empty(),
        "the competitor's case is matched in both runs: {comparison}"
    );
    assert!(
        comparison["regressions"]
            .as_array()
            .expect("regressions")
            .is_empty()
    );
}

/// An adapter block naming a CLI that is not installed is the person's
/// mistake, said before a case runs — not an hour into a suite.
#[test]
fn an_uninstalled_competitor_is_usage() {
    let dir = workspace();
    let path = dir.path().join("missing.toml");
    std::fs::write(
        &path,
        r#"
name = "missing"

[adapter.definitely-not-installed]
command = "definitely-not-installed --print"

[[case]]
id = "anything"
prompt = "hello"

[case.workspace]
kind = "empty"

[case.grade]
grader = "assertion"

[case.grade.config]
checks = []
"#,
    )
    .expect("the suite file");

    let out = orrery(&args(
        &base(dir.path(), &["text-turn.jsonl"]),
        &["eval", "run", &path.display().to_string()],
    ));
    assert_eq!(out.status.code(), Some(2), "an unusable suite is usage");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("definitely-not-installed"),
        "it names the adapter: {stderr}"
    );
    assert!(out.stdout.is_empty(), "stdout is data, and there is none");
}

/// A long workspace path is a filesystem fact, not a permission decision.
///
/// A driven run against a 143-character workspace came back `error`, with
/// "denied: no rule allows `read(<case-workspace>/Cargo.toml)`" on it, while
/// the same suite in a 19-character workspace scored 1.0. The eval isolator
/// adds a case directory to whatever root it is given, so the grader's read
/// crossed Windows' `MAX_PATH`; `canonicalize` handed back the `\?\` form,
/// nothing matched the compiled rule, and a length came out of the engine
/// wearing a policy verdict's clothes. This pins both halves: it runs, and it
/// scores.
///
/// 200 rather than the reported 143: the case directory's own length decides
/// where `MAX_PATH` is crossed, and it depends on the case id and the matrix
/// point's names. 200 crosses it whatever they are called — at 143 this same
/// test passes against the unfixed engine by a dozen characters.
#[test]
fn a_long_workspace_path_runs_and_is_graded() {
    let temp = tempfile::tempdir().expect("a temporary root");
    let mut dir = temp.path().to_path_buf();
    while dir.display().to_string().len() < 200 {
        dir.push("a-long-workspace-segment");
    }
    std::fs::create_dir_all(&dir).expect("a long workspace directory");
    assert!(
        dir.display().to_string().len() >= 200,
        "the fixture has to be long to be testing anything"
    );
    std::fs::write(dir.join(TARGET), CONTENTS).expect("the target file");

    let report = run_suite(&dir);
    let results = report["results"].as_array().expect("results");
    assert_eq!(
        results[0]["outcome"], "pass",
        "a 143-character workspace grades like any other: {}",
        results[0]
    );
    assert_eq!(results[0]["score"], 1.0);
}
