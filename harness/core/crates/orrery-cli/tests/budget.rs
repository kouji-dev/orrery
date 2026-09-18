//! A ceiling written in a config file, enforced by the binary.
//!
//! Round 5, item 3. `maxUsd` was inert in every build for one structural
//! reason: `orrery-harness` did not depend on `orrery-config`, so resolved
//! config had no path into the kernel and `PriceTable` was always
//! [`empty`](orrery_kernel::PriceTable::empty). A library test could set a price
//! table by hand and watch the ceiling work; nobody running `orrery` could.
//!
//! So this test drives the **binary**, with a price table and a ceiling written
//! in a user-layer config file and nothing else, and asserts the exit code the
//! CI table gives a budget stop: 3.
//!
//! No model, no network: the passes are committed `.jsonl` fixtures.

mod common;

use std::path::Path;

use common::{CONTENTS, TARGET, stream};

/// The binary, with a sandboxed home **and** a workspace.
fn run(home: &Path, ws: &Path, streams: &[&str], rest: &[&str]) -> std::process::Output {
    let mut argv = vec![
        "--workspace".to_owned(),
        ws.display().to_string(),
        "--state-dir".to_owned(),
        ws.join(".state").display().to_string(),
    ];
    for name in streams {
        argv.push("--provider".to_owned());
        argv.push(format!("fixture:{}", stream(name).display()));
    }
    argv.extend(rest.iter().map(|s| (*s).to_owned()));

    std::process::Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(&argv)
        .env("COLUMNS", "100")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ProgramData", home.join("ProgramData"))
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the orrery binary runs")
}

/// A sandboxed home with one user-layer config file in it.
fn home_with(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary home");
    std::fs::create_dir_all(dir.path().join(".orrery")).expect("the user directory");
    std::fs::write(dir.path().join(".orrery/config.toml"), config).expect("the user config");
    dir
}

/// A workspace holding the file the `tool-call` fixture reads.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    std::fs::write(dir.path().join(TARGET), CONTENTS).expect("the target file");
    dir
}

/// The prices and the ceiling, as a person writes them.
///
/// A micro-USD per token is not a real price; it is a price that makes one pass
/// of a 976-token fixture cost more than the cap, which is what the test is
/// about. `maxMicroUsd = 1` with no `[prices]` table would prove nothing, and
/// that is exactly the inert state this test exists to rule out.
const CAPPED: &str = "\
[budget]
maxMicroUsd = 1

[prices.fixture]
inputPerMillion = 1000000
outputPerMillion = 1000000
";

/// A ceiling in a config file stops a real turn, through the binary.
#[test]
fn a_config_ceiling_stops_a_turn() {
    let home = home_with(CAPPED);
    let ws = workspace();

    let out = run(
        home.path(),
        ws.path(),
        &["tool-call.jsonl", "text-turn.jsonl"],
        &["run", "-p", "read the manifest"],
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(3),
        "a budget stop is exit 3. stderr was: {stderr}"
    );
    assert!(
        stderr.contains("Usd"),
        "and it says which ceiling: {stderr}"
    );
}

/// The same run, with no ceiling written anywhere, completes. Without this the
/// test above would pass just as well if the binary were broken.
#[test]
fn the_same_turn_without_a_ceiling_completes() {
    let home = home_with("");
    let ws = workspace();

    let out = run(
        home.path(),
        ws.path(),
        &["tool-call.jsonl", "text-turn.jsonl"],
        &["run", "-p", "read the manifest"],
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr was: {stderr}");
}

/// A ceiling with **no price table** stays inert rather than stopping every
/// turn at zero. Money the harness cannot price is money it does not pretend to
/// know about.
#[test]
fn a_ceiling_with_no_prices_is_inert() {
    let home = home_with("[budget]\nmaxMicroUsd = 1\n");
    let ws = workspace();

    let out = run(
        home.path(),
        ws.path(),
        &["tool-call.jsonl", "text-turn.jsonl"],
        &["run", "-p", "read the manifest"],
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "an unpriced model has no money ceiling. stderr was: {stderr}"
    );
}

/// And the turn-count ceiling, which needs no prices at all, comes from the
/// same file through the same path.
#[test]
fn a_turn_ceiling_comes_from_config_too() {
    let home = home_with("[budget]\nmaxTurns = 1\n");
    let ws = workspace();

    let out = run(
        home.path(),
        ws.path(),
        &["tool-call.jsonl", "text-turn.jsonl"],
        &["run", "-p", "read the manifest"],
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(3), "stderr was: {stderr}");
    assert!(stderr.contains("Turns"), "{stderr}");
}
