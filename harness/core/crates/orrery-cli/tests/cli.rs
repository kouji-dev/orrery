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
        ("trust", &["trust", "--help"]),
        ("trust-grant", &["trust", "grant", "--help"]),
        ("trust-revoke", &["trust", "revoke", "--help"]),
        ("trust-list", &["trust", "list", "--help"]),
        ("init", &["init", "--help"]),
        ("import", &["import", "--help"]),
        ("eval", &["eval", "--help"]),
        ("eval-run", &["eval", "run", "--help"]),
        ("eval-compare", &["eval", "compare", "--help"]),
        ("eval-replay", &["eval", "replay", "--help"]),
        ("workflow", &["workflow", "--help"]),
        ("workflow-check", &["workflow", "check", "--help"]),
        ("workflow-run", &["workflow", "run", "--help"]),
        ("mcp", &["mcp", "--help"]),
        ("mcp-list", &["mcp", "list", "--help"]),
        ("mcp-tools", &["mcp", "tools", "--help"]),
        ("skills", &["skills", "--help"]),
        ("skills-list", &["skills", "list", "--help"]),
        ("skills-show", &["skills", "show", "--help"]),
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

/// **Nothing is on this list any more.** Every subcommand in the tree is
/// implemented as of 2026-09-19: `run`, `serve`, `attach`, `replay`,
/// `session list|show|rm`, `install`, `remove`, `ext`, `permissions`, `config`,
/// `init`, `import`, `eval`, `ledger`, `telemetry`, `mcp` and `skills` each
/// have a suite of their own.
///
/// What is asserted here instead is the property the list existed to protect:
/// **every command in `--help` does something.** A command that parses and then
/// exits 2 saying "not implemented in this build" is a command that is in the
/// tree and not in the product, which is exactly the gap this round closed.
#[test]
fn no_subcommand_is_a_stub() {
    let help = orrery(&["--help"]);
    let text = String::from_utf8_lossy(&help.stdout);
    let commands: Vec<&str> = text
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .filter(|w| *w != "help")
        .collect();
    assert!(commands.len() > 10, "the tree was read: {commands:?}");

    for command in commands {
        // No arguments, so most of these are a usage error — which is the
        // point: a usage error is the *command* answering. What must never
        // appear is the stub message.
        let out = orrery(&[command]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("not implemented in this build"),
            "`orrery {command}` is still a stub: {stderr}"
        );
    }
}

/// Narration goes to stderr so `--json` stdout stays machine-readable
/// (`17-cli.md`, streams discipline).
#[test]
fn a_usage_error_does_not_pollute_stdout() {
    let out = orrery(&["eval", "run", "no-such-suite", "--json"]);
    assert!(
        out.stdout.is_empty(),
        "stdout must stay clean: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// `session rm` is in the tree, takes `--yes`, and needs it: a delete that
/// cannot be undone must not happen because a script forgot a flag.
///
/// The interesting half — that it really removes the session — is in
/// `tests/session.rs`, where there is history on disk to remove.
#[test]
fn session_rm_without_yes_refuses() {
    let out = orrery(&["session", "rm", "00000000-0000-0000-0000-000000000000"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--yes"), "it says which flag: {stderr}");
    assert!(
        stderr.contains("cannot be reconstructed") || stderr.contains("cannot be undone"),
        "and why: {stderr}"
    );
}
