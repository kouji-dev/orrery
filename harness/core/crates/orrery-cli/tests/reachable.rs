//! What the **shipped binary** can actually reach.
//!
//! Every assertion here drives `CARGO_BIN_EXE_orrery`, not a library. That is
//! the whole point of the file: this tree spent a long while with crates that
//! were green under `cargo test` and unreachable from the product — 21 of 44
//! `orrery-*` crates were not in `cargo tree -p orrery-cli` at all, and
//! `ProviderChoice` had no variant a person could name to talk to a model.
//!
//! A green test the binary cannot reach does not count as done, so these are
//! the tests that would have caught that.
//!
//! Nothing here starts a session, opens a database, reaches the network or
//! needs a key: `ext list` and `--help` are the two surfaces that answer "what
//! is in this build" without doing anything.

use std::process::{Command, Output};

fn orrery(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(args)
        .env("COLUMNS", "100")
        .output()
        .expect("the orrery binary runs")
}

fn stdout(args: &[&str]) -> String {
    let out = orrery(args);
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

/// The bundles a default build ships, by name, from the binary's own mouth.
#[test]
fn ext_list_names_every_default_bundle() {
    let listed = stdout(&["ext", "list"]);
    for ext in ["builtin", "git", "lsp"] {
        assert!(
            listed.lines().any(|l| l.starts_with(&format!("{ext}  "))),
            "`orrery ext list` never mentions `{ext}`:\n{listed}"
        );
    }
}

/// Every bundle the binary lists loads cleanly, rather than loading degraded
/// with half its tools switched off.
///
/// `degraded` is a legitimate state — it is what a missing grant looks like —
/// but a **first-party** bundle listed by its own build should not be in it:
/// that would mean the manifest asks for something the build does not grant,
/// which is a packaging mistake and not a policy decision.
#[test]
fn every_listed_bundle_loads_clean() {
    let listed = stdout(&["ext", "list"]);
    let degraded: Vec<&str> = listed
        .lines()
        .filter(|l| l.contains("  degraded  "))
        .collect();
    assert!(
        degraded.is_empty(),
        "{degraded:?}\n\nfull listing:\n{listed}"
    );
}

/// The git bundle's five verbs are reachable, named, and it is the read-only
/// set — `commit` and `branch` were declared by a scaffold that had neither.
#[test]
fn the_git_bundle_offers_the_read_only_verbs() {
    let listed = stdout(&["ext", "list"]);
    let line = listed
        .lines()
        .find(|l| l.starts_with("git  "))
        .unwrap_or_else(|| panic!("no git line:\n{listed}"));
    for verb in ["status", "log", "show", "diff", "blame"] {
        assert!(line.contains(verb), "`{verb}` missing from: {line}");
    }
    assert!(
        !line.contains("commit"),
        "a verb nothing implements: {line}"
    );
}

/// The language-server bundle's four verbs are reachable and named.
#[test]
fn the_lsp_bundle_offers_its_four_verbs() {
    let listed = stdout(&["ext", "list"]);
    let line = listed
        .lines()
        .find(|l| l.starts_with("lsp  "))
        .unwrap_or_else(|| panic!("no lsp line:\n{listed}"));
    for verb in ["hover", "definition", "references", "diagnostics"] {
        assert!(line.contains(verb), "`{verb}` missing from: {line}");
    }
    // `symbols` was declared while the crate was a scaffold and never existed.
    assert!(
        !line.contains("symbols"),
        "a verb nothing implements: {line}"
    );
}

/// `ext test` runs a first-party manifest through the same load a session uses,
/// with no model and no network — and says so.
#[test]
fn ext_test_runs_a_first_party_manifest_with_no_model_and_no_network() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../extensions/crates/orrery-ext-git/orrery.toml");
    let out = orrery(&["ext", "test", &manifest.display().to_string()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("no model, no network"), "{text}");
    assert!(text.contains("git"), "{text}");
}
