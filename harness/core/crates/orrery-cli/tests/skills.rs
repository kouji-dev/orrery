//! `orrery skills list | show`.
//!
//! The skills are real `SKILL.md` files the test writes into a sandboxed home,
//! in the agentskills.io shape, unchanged. Nothing here starts a model.

mod common;

use common::{args, home_with, jsonl, orrery_in, quiet};

/// Write a `SKILL.md` into a home's user layer.
fn skill(home: &std::path::Path, dir: &str, front: &str, body: &str) {
    let root = home.join(".orrery/skills").join(dir);
    std::fs::create_dir_all(&root).expect("the skill directory");
    std::fs::write(
        root.join("SKILL.md"),
        format!("---\n{front}---\n\n{body}\n"),
    )
    .expect("the skill file");
}

/// The everyday question: what is in force, and may any of it run code.
#[test]
fn list_shows_the_discovered_skills() {
    let home = home_with("");
    skill(
        home.path(),
        "commit-style",
        "name: commit-style\ndescription: How this repository writes commit messages\n",
        "Lead with the verb.",
    );
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["--json", "skills", "list"]),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows = jsonl(&out.stdout);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "commit-style");
    assert_eq!(
        rows[0]["description"], "How this repository writes commit messages",
        "the front matter is taken unchanged"
    );
    assert_eq!(rows[0]["layer"], "user");
    assert_eq!(
        rows[0]["grant"],
        serde_json::Value::Null,
        "no grant declared means its scripts cannot run, and it says null"
    );
}

/// The human column says the same thing in a word.
#[test]
fn the_grant_column_says_whether_scripts_can_run() {
    let home = home_with("");
    skill(
        home.path(),
        "safe",
        "name: safe\ndescription: Notes only\n",
        "Read me.",
    );
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(home.path(), &args(&quiet(ws.path()), &["skills", "list"]));
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("safe"), "{text}");
    assert!(
        text.contains(" - "),
        "a skill with no grant shows a dash, not a blank: {text}"
    );
}

/// `show` prints the front matter and the body an agent would be handed.
#[test]
fn show_prints_the_document() {
    let home = home_with("");
    skill(
        home.path(),
        "commit-style",
        "name: commit-style\ndescription: How this repository writes commit messages\n",
        "Lead with the verb, and never restate the diff.",
    );
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["skills", "show", "commit-style"]),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("commit-style"), "{text}");
    assert!(
        text.contains("Lead with the verb"),
        "the body is there: {text}"
    );
    assert!(
        text.contains("cannot run: no grant declared"),
        "and the one thing this design adds is stated: {text}"
    );
}

/// A skill scoped to another agent is **absent**, not listed and refused.
#[test]
fn a_scoped_skill_is_absent_from_another_agent() {
    // Scope is declared in **configuration**, never in the front matter: the
    // `SKILL.md` format is adopted unchanged, so a skill cannot widen its own
    // reach by editing itself.
    let home = home_with("[skills.reviewer-only]\nscope = [\"reviewer\"]\n");
    skill(
        home.path(),
        "reviewer-only",
        "name: reviewer-only\ndescription: For the reviewer\n",
        "Check the tests.",
    );
    let ws = tempfile::tempdir().expect("a workspace");

    let mine = orrery_in(
        home.path(),
        &args(
            &quiet(ws.path()),
            &["--json", "skills", "list", "--agent", "reviewer"],
        ),
    );
    assert!(mine.status.success());
    assert_eq!(jsonl(&mine.stdout).len(), 1, "the reviewer sees it");

    let theirs = orrery_in(
        home.path(),
        &args(
            &quiet(ws.path()),
            &["--json", "skills", "list", "--agent", "main"],
        ),
    );
    assert!(theirs.status.success());
    assert!(
        theirs.stdout.is_empty(),
        "and nobody else does: {}",
        String::from_utf8_lossy(&theirs.stdout)
    );
}

/// Nothing found is an answer; a name that is not there is a mistake.
#[test]
fn nothing_found_and_no_such_skill() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");

    let empty = orrery_in(home.path(), &args(&quiet(ws.path()), &["skills", "list"]));
    assert!(empty.status.success());
    assert!(empty.stdout.is_empty(), "stdout is data, and there is none");
    assert!(String::from_utf8_lossy(&empty.stderr).contains("no skills found"));

    let missing = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["skills", "show", "nope"]),
    );
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("nope"));
}
