//! `orrery permissions explain` and `orrery config explain`.
//!
//! Both answer from the layers on disk and neither starts a model: there is no
//! `--provider` anywhere in this file, and no network request is possible.
//!
//! Every run here is sandboxed by [`common::orrery_in`] — `HOME`, `USERPROFILE`
//! and `ProgramData` point inside a temporary directory — so the assertions are
//! about the files the test wrote and not about whatever the developer keeps in
//! `~/.orrery`.

mod common;

use common::{args, home_with, jsonl, orrery_in, quiet};

/// Plan 17 task 6: the rule, the layer, the file and the line.
#[test]
fn permissions_names_rule_layer_file() {
    let home = home_with(
        "[permissions]\n\
         deny = [\"net(domain: *)\"]\n\
         allow = [\"read(./**)\"]\n",
    );
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(
            &quiet(ws.path()),
            &["permissions", "explain", "net(domain: evil.example)"],
        ),
    );
    assert!(out.status.success(), "explaining is not a failure");
    let text = String::from_utf8_lossy(&out.stdout);

    assert!(text.contains("Deny"), "the verdict is named: {text}");
    assert!(
        text.contains("net(domain: *)"),
        "the rule is quoted as written: {text}"
    );
    assert!(text.contains("User"), "the layer is named: {text}");
    assert!(
        text.contains("config.toml:2"),
        "the file and the line are named: {text}"
    );
}

/// The same answer, machine-readable, because a CI script asks this too.
#[test]
fn permissions_explain_is_json_on_request() {
    let home = home_with("[permissions]\nallow = [\"read(./**)\"]\n");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(
            &quiet(ws.path()),
            &["--json", "permissions", "explain", "read(./src/main.rs)"],
        ),
    );
    assert!(out.status.success());
    let lines = jsonl(&out.stdout);
    assert_eq!(lines.len(), 1, "one object, on one line");
    let v = &lines[0];
    assert_eq!(v["verdict"], "allow");
    assert_eq!(v["rule"]["text"], "read(./**)");
    assert_eq!(v["rule"]["layer"], "user");
    assert_eq!(v["rule"]["line"], 2);
    assert!(
        v["rule"]["file"].as_str().is_some_and(|f| f.ends_with("config.toml")),
        "{v}"
    );
}

/// A call that is not in the rule grammar is a usage error, naming the grammar.
#[test]
fn a_call_that_is_not_a_call_is_usage() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["permissions", "explain", "rm -rf /"]),
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "stdout is data, and there is none");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("read"), "the grammar is named: {err}");
}

/// Plan 10 task 7, at a shell: the winning layer, and what it beat.
#[test]
fn config_names_the_winning_layer() {
    let home = home_with("model = \"sonnet\"\n");
    std::fs::create_dir_all(home.path().join("ProgramData/Orrery")).expect("the managed directory");
    std::fs::write(
        home.path().join("ProgramData/Orrery/managed.toml"),
        "# written by IT\nmodel = \"managed-model\"\n",
    )
    .expect("the managed layer");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["config", "explain", "model"]),
    );
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("sonnet"), "the winner is printed: {text}");
    assert!(text.contains("in force"), "and said to be in force: {text}");
    assert!(
        text.contains("managed-model") && text.contains("shadowed"),
        "the loser is named too, or somebody edits a file that is not in force: {text}"
    );
}

/// A key nobody set is not an error — it is an answer.
#[test]
fn config_explain_says_when_nobody_set_it() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["--json", "config", "explain", "model"]),
    );
    assert!(out.status.success());
    let v = &jsonl(&out.stdout)[0];
    assert_eq!(v["key"], "model");
    assert_eq!(v["contributions"].as_array().map(Vec::len), Some(0));
    assert!(v["winner"].is_null());
}
