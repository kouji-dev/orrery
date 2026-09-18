//! `orrery ext` — the command plan 06 task 8 was waiting for.
//!
//! Both subcommands here run with **no model, no network and no database**:
//! they go through `orrery_ext_api::testing`, which is the same mock broker an
//! extension author writes their own tests against.

mod common;

use common::orrery;

/// `orrery ext test` passes on a fixture extension with no provider named at
/// all — no `--provider`, no key, nothing to reach (plan 06 task 8).
#[test]
fn test_runs_without_a_model() {
    let dir = tempfile::tempdir().expect("a temporary extension");
    std::fs::write(
        dir.path().join("orrery.toml"),
        "\
api = \"orrery-ext/1\"
runtime = \"native\"

[extension]
id = \"fixture-ext\"
version = \"0.1.0\"

[provides]
tools = [\"hello\"]

[requires]
read = [\"$WORKSPACE/**\"]
",
    )
    .expect("the manifest is writable");

    let out = orrery(&[
        "ext".to_owned(),
        "test".to_owned(),
        dir.path().display().to_string(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr was: {stderr}");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("fixture-ext"), "it names it: {stdout}");
    assert!(stdout.contains("hello"), "…and what it contributes: {stdout}");
    assert!(
        stdout.contains("no model, no network"),
        "…and says what it did not need: {stdout}"
    );
}

/// A manifest that will not parse is a usage error naming the file, not a panic.
#[test]
fn a_broken_manifest_is_usage() {
    let dir = tempfile::tempdir().expect("a temporary extension");
    std::fs::write(dir.path().join("orrery.toml"), "this is not toml = = =")
        .expect("the manifest is writable");
    let out = orrery(&[
        "ext".to_owned(),
        "test".to_owned(),
        dir.path().display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
}

/// `orrery ext list` reports the first-party set this build has, with what each
/// one contributes.
#[test]
fn list_shows_the_ledger() {
    let out = orrery(&["ext".to_owned(), "list".to_owned()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("builtin"), "the builtin bundle: {stdout}");
    for tool in ["read", "write", "edit", "bash", "grep", "glob"] {
        assert!(stdout.contains(tool), "`{tool}` is missing: {stdout}");
    }
    assert!(
        stdout.contains("ok"),
        "and it says how it loaded: {stdout}"
    );
}
