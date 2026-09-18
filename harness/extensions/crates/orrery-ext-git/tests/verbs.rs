//! The five verbs, against a real repository built in a temp directory.
//!
//! # Why a real repository and not the mock broker
//!
//! The mock broker is the right harness for "was this refused" — and
//! `denied_before_gitoxide_is_opened` uses it for exactly that. It is the wrong
//! harness for "what does `status` say about a rename", because a mock that
//! answered would be a second implementation of git agreeing with the first
//! until it did not.
//!
//! The fixtures are built with the system `git` binary rather than with
//! gitoxide, on purpose: a test that built its commits with the same library it
//! is testing could not catch a library that writes and reads its own mistake
//! consistently.
//!
//! No network, and nothing outside the temp directory.

use std::path::Path;
use std::process::Command;

use orrery_ext_api::testing::load_for_test;
use orrery_ext_api::{CallCtx, NativeExtension};
use orrery_ext_git::{GitTools, MANIFEST};
use orrery_proto::Outcome;
use serde_json::{Value, json};

/// Run `git` in `dir`, and fail loudly with what it said.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repository with two commits, a modified file and an untracked one.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path();
    git(path, &["init", "--initial-branch=main"]);
    // A committer, because a machine running this may have none configured and
    // an unconfigured `git commit` fails rather than defaulting.
    git(path, &["config", "user.email", "test@example.test"]);
    git(path, &["config", "user.name", "Test Person"]);
    git(path, &["config", "commit.gpgsign", "false"]);

    std::fs::write(path.join("a.txt"), "one\ntwo\nthree\n").expect("write");
    git(path, &["add", "a.txt"]);
    git(path, &["commit", "-m", "first: add a.txt"]);

    std::fs::write(path.join("b.txt"), "beta\n").expect("write");
    git(path, &["add", "b.txt"]);
    git(path, &["commit", "-m", "second: add b.txt"]);

    // Now dirty it: one tracked file edited, one file nobody has added.
    std::fs::write(path.join("a.txt"), "one\ntwo\nTHREE\n").expect("write");
    std::fs::write(path.join("untracked.txt"), "hello\n").expect("write");
    dir
}

/// A context whose grants cover the whole temp tree.
fn ctx(tool: &str) -> (orrery_ext_api::testing::TestHarness, CallCtx) {
    let harness = load_for_test(MANIFEST, &["read"]).expect("the shipped manifest loads");
    let ctx = harness.ctx(tool);
    (harness, ctx)
}

async fn call(tool: &str, input: Value) -> Outcome {
    let (_harness, ctx) = ctx(tool);
    GitTools::new()
        .call(tool, input, &ctx)
        .await
        .expect("the harness carried the call")
}

fn value_of(outcome: &Outcome) -> &Value {
    match outcome {
        Outcome::Ok { value: Some(v), .. } => v,
        other => panic!("expected Ok with a value, got {other:?}"),
    }
}

#[tokio::test]
async fn the_shipped_manifest_is_the_same_shape_a_third_party_ships() {
    let harness = load_for_test(MANIFEST, &["read"]).expect("it parses");
    let names: Vec<String> = harness
        .manifest()
        .contributions()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(names, ["status", "log", "show", "diff", "blame"]);
    // Every declared tool exists, which is the half a scaffold gets wrong.
    let implemented: Vec<String> = GitTools::new()
        .tools()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, implemented);
    assert!(
        matches!(harness.load_outcome(), orrery_proto::LoadOutcome::Ok { .. }),
        "{:?}",
        harness.load_outcome()
    );
}

#[tokio::test]
async fn status_separates_modified_from_untracked() {
    let dir = fixture();
    let out = call("status", json!({ "repo": dir.path() })).await;
    let rows = value_of(&out).as_array().expect("an array").clone();

    let find = |name: &str| {
        rows.iter()
            .find(|r| r["path"] == name)
            .unwrap_or_else(|| panic!("no row for {name} in {rows:?}"))
            .clone()
    };
    assert_eq!(find("a.txt")["state"], "M");
    assert_eq!(find("untracked.txt")["state"], "?");
    // b.txt is committed and untouched, so it is not in the answer at all.
    assert!(rows.iter().all(|r| r["path"] != "b.txt"), "{rows:?}");
}

#[tokio::test]
async fn status_reports_a_deleted_file_as_deleted_not_modified() {
    let dir = fixture();
    std::fs::remove_file(dir.path().join("b.txt")).expect("remove");
    let out = call("status", json!({ "repo": dir.path() })).await;
    let rows = value_of(&out).as_array().expect("an array").clone();
    let b = rows
        .iter()
        .find(|r| r["path"] == "b.txt")
        .unwrap_or_else(|| panic!("no row for b.txt in {rows:?}"));
    // The model's next move differs completely between the two.
    assert_eq!(b["state"], "D");
}

#[tokio::test]
async fn log_is_newest_first_and_bounded() {
    let dir = fixture();
    let out = call("log", json!({ "repo": dir.path() })).await;
    let rows = value_of(&out).as_array().expect("an array").clone();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["subject"], "second: add b.txt");
    assert_eq!(rows[1]["subject"], "first: add a.txt");
    assert_eq!(rows[0]["author"], "Test Person");
    assert_eq!(
        rows[0]["sha"].as_str().expect("a sha").len(),
        7,
        "seven characters, as git abbreviates"
    );

    let one = call("log", json!({ "repo": dir.path(), "limit": 1 })).await;
    assert_eq!(value_of(&one).as_array().expect("an array").len(), 1);
    // A limit past the ceiling is clamped, not honoured: a log with no ceiling
    // is a context window spent on history.
    let huge = call("log", json!({ "repo": dir.path(), "limit": 10_000 })).await;
    assert_eq!(value_of(&huge).as_array().expect("an array").len(), 2);
}

#[tokio::test]
async fn show_names_the_paths_the_commit_touched() {
    let dir = fixture();
    let out = call("show", json!({ "repo": dir.path() })).await;
    let v = value_of(&out);
    assert_eq!(v["commit"]["subject"], "second: add b.txt");
    let changes = v["changes"].as_array().expect("an array");
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0]["path"], "b.txt");
    assert_eq!(changes[0]["state"], "A");
}

#[tokio::test]
async fn diff_between_two_revisions_names_what_moved() {
    let dir = fixture();
    let out = call(
        "diff",
        json!({ "repo": dir.path(), "from": "HEAD~1", "to": "HEAD" }),
    )
    .await;
    let rows = value_of(&out).as_array().expect("an array").clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], "b.txt");
    assert_eq!(rows[0]["state"], "A");
}

#[tokio::test]
async fn diff_with_only_a_target_compares_it_against_its_parent() {
    let dir = fixture();
    let bare = call("diff", json!({ "repo": dir.path() })).await;
    let paired = call(
        "diff",
        json!({ "repo": dir.path(), "from": "HEAD~1", "to": "HEAD" }),
    )
    .await;
    // `diff <rev>` means "against its parent" far more often than "against
    // nothing", and answering the second would list the whole tree.
    assert_eq!(value_of(&bare), value_of(&paired));
}

#[tokio::test]
async fn blame_attributes_every_line() {
    let dir = fixture();
    // Blame reads committed history, so blame the committed state.
    let out = call("blame", json!({ "repo": dir.path(), "path": "a.txt" })).await;
    let v = value_of(&out);
    let lines = v["lines"].as_array().expect("an array");
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0]["line"], 1, "one-based, as every editor counts");
    assert_eq!(lines[2]["line"], 3);
    for line in lines {
        assert_eq!(line["author"], "Test Person", "{line:?}");
    }
}

#[tokio::test]
async fn blame_without_a_path_says_so_rather_than_guessing() {
    let dir = fixture();
    let out = call("blame", json!({ "repo": dir.path() })).await;
    match out {
        Outcome::Failed { code, .. } => assert_eq!(code, "bad_request"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn a_directory_that_is_not_a_repository_says_which_one() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let out = call("status", json!({ "repo": dir.path() })).await;
    match out {
        Outcome::Failed { code, message } => {
            assert_eq!(code, "not_a_repository");
            assert!(message.contains("no git repository"), "{message}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn a_revision_that_names_nothing_is_not_silently_head() {
    let dir = fixture();
    let out = call("show", json!({ "repo": dir.path(), "rev": "no-such-ref" })).await;
    match out {
        Outcome::Failed { code, .. } => assert_eq!(code, "no_such_revision"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn denied_before_gitoxide_is_opened() {
    let dir = fixture();
    // A grant that covers something else entirely. gitoxide would happily read
    // this repository; the broker is what stops it.
    let harness = load_for_test(MANIFEST, &["read:/nowhere/**"]).expect("the manifest loads");
    let ctx = harness.ctx("status");
    let out = GitTools::new()
        .call("status", json!({ "repo": dir.path() }), &ctx)
        .await
        .expect("the harness carried the call");

    match out {
        Outcome::Denied { reason, .. } => assert!(reason.contains("read"), "{reason}"),
        other => panic!("expected Denied, got {other:?}"),
    }
    // And the refusal is in the ledger, which is the point of routing a
    // permission question through the broker that gitoxide cannot use.
    assert!(
        harness.recorded().iter().any(|c| matches!(
            c,
            orrery_ext_api::testing::BrokerCall::Read { allowed: false, .. }
        )),
        "{:?}",
        harness.recorded()
    );
}

#[tokio::test]
async fn an_unknown_tool_is_an_error_not_an_empty_answer() {
    let (_h, ctx) = ctx("rebase");
    let e = GitTools::new()
        .call("rebase", json!({}), &ctx)
        .await
        .expect_err("no such tool");
    assert!(e.to_string().contains("rebase"), "{e}");
}

#[tokio::test]
async fn a_cancelled_call_does_no_work() {
    let dir = fixture();
    let harness = load_for_test(MANIFEST, &["read"]).expect("the manifest loads");
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    let ctx = harness.ctx_with("status", orrery_ext_api::ToolBudget::default(), cancel);
    let out = GitTools::new()
        .call("status", json!({ "repo": dir.path() }), &ctx)
        .await
        .expect("the harness carried the call");
    assert!(matches!(out, Outcome::Cancelled { .. }), "{out:?}");
}
