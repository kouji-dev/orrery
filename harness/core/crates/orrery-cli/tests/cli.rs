//! The command tree is a public surface: it must not change silently.
//!
//! Every snapshot below is a committed file under `tests/snapshots/`. Regenerate
//! them deliberately with `ORRERY_UPDATE_SNAPSHOTS=1 cargo test -p orrery-cli`,
//! and read the diff before committing it.

use std::path::PathBuf;
use std::process::{Command, Output};

fn orrery(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(args)
        // clap wraps help to the terminal width; pin it so the snapshots are
        // the same on every machine.
        .env("COLUMNS", "100")
        .output()
        .expect("the orrery binary runs")
}

fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.txt"))
}

fn assert_snapshot(name: &str, actual: &str) {
    let path = snapshot_path(name);
    // clap prints argv[0], which is `orrery.exe` on Windows. Normalise it so
    // one set of snapshots covers every platform.
    let actual = actual.replace("\r\n", "\n").replace("orrery.exe", "orrery");
    if std::env::var_os("ORRERY_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual).expect("the snapshot is writable");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing snapshot {}: {e}", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(
        expected,
        actual,
        "\n--- {name} drifted from {} ---\nrerun with ORRERY_UPDATE_SNAPSHOTS=1 if this is intended\n",
        path.display()
    );
}

/// Every command and subcommand in `17-cli.md` is present, with its help text
/// pinned so the surface cannot change without a visible diff.
#[test]
fn help_snapshot() {
    let cases: &[(&str, &[&str])] = &[
        ("root", &["--help"]),
        ("run", &["run", "--help"]),
        ("serve", &["serve", "--help"]),
        ("attach", &["attach", "--help"]),
        ("replay", &["replay", "--help"]),
        ("session", &["session", "--help"]),
        ("session-list", &["session", "list", "--help"]),
        ("session-show", &["session", "show", "--help"]),
        ("session-rm", &["session", "rm", "--help"]),
        ("install", &["install", "--help"]),
        ("remove", &["remove", "--help"]),
        ("ext", &["ext", "--help"]),
        ("ext-list", &["ext", "list", "--help"]),
        ("ext-test", &["ext", "test", "--help"]),
        ("permissions", &["permissions", "--help"]),
        ("permissions-explain", &["permissions", "explain", "--help"]),
        ("config", &["config", "--help"]),
        ("config-explain", &["config", "explain", "--help"]),
        ("init", &["init", "--help"]),
        ("import", &["import", "--help"]),
        ("eval", &["eval", "--help"]),
        ("eval-run", &["eval", "run", "--help"]),
        ("eval-compare", &["eval", "compare", "--help"]),
        ("eval-replay", &["eval", "replay", "--help"]),
        ("ledger", &["ledger", "--help"]),
        ("telemetry", &["telemetry", "--help"]),
    ];
    for (name, args) in cases {
        let out = orrery(args);
        assert!(
            out.status.success(),
            "`orrery {}` should exit 0",
            args.join(" ")
        );
        assert_snapshot(name, &String::from_utf8_lossy(&out.stdout));
    }
}

/// Usage errors are exit code 2 (`17-cli.md`, exit-code table).
#[test]
fn unknown_flag_is_exit_2() {
    let out = orrery(&["--no-such-flag"]);
    assert_eq!(out.status.code(), Some(2));
}

/// Open question 1, checked rather than assumed: a typo'd subcommand is a
/// usage error, not a TUI.
///
/// `Cli` has no top-level positional argument, so clap has nothing to bind an
/// unrecognised word to and refuses it by name. Bare `orrery` being
/// interactive therefore costs nothing.
#[test]
fn a_typo_is_not_an_interactive_session() {
    let out = orrery(&["rnu", "-p", "hi"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unrecognized subcommand") && stderr.contains("rnu"),
        "and it says which word it did not know: {stderr}"
    );
}

/// A subcommand that has not landed yet exits 2 and names the plan file that
/// will implement it, so the tree is complete from phase 0 and fills in.
///
/// `run`, `serve`, `attach`, `ext test`, `session list|show`, both `explain`s,
/// `init` and `import` are no longer on this list: they are implemented, and
/// their own suites cover them.
#[test]
fn unimplemented_subcommands_name_their_plan() {
    let cases: &[(&[&str], &str)] = &[
        (&["replay", "s1"], "17-cli.md"),
        (&["eval", "run", "suite"], "16-eval-runner.md"),
        (&["ledger"], "07-policy-broker-audit.md"),
        (&["telemetry"], "07-policy-broker-audit.md"),
    ];
    for (args, plan) in cases {
        let out = orrery(args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`orrery {}` should exit 2 while unimplemented; stderr was: {stderr}",
            args.join(" ")
        );
        assert!(
            stderr.contains("not implemented in this build"),
            "`orrery {}` should say so: {stderr}",
            args.join(" ")
        );
        assert!(
            stderr.contains(plan),
            "`orrery {}` should name {plan}: {stderr}",
            args.join(" ")
        );
    }
}

/// Narration goes to stderr so `--json` stdout stays machine-readable
/// (`17-cli.md`, streams discipline).
#[test]
fn not_implemented_does_not_pollute_stdout() {
    let out = orrery(&["replay", "s1", "--json"]);
    assert!(
        out.stdout.is_empty(),
        "stdout must stay clean: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}


/// `session rm` is in the tree, takes `--yes`, and refuses for a reason that is
/// about the store rather than about this plan: there is no delete on
/// `SessionStore`, and plan 02 parked retention. It says so.
#[test]
fn session_rm_names_the_missing_piece() {
    let out = orrery(&["session", "rm", "s1", "--yes"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("02-session-store.md"), "{stderr}");
    assert!(
        stderr.contains("retention"),
        "and why, not just which file: {stderr}"
    );
}
