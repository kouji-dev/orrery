//! `[permissions]` is enforced, and `permissions explain` tells the truth.
//!
//! The one invariant every test here asserts is an **equivalence**, not two
//! separate facts: for one call, under one set of layers, what
//! `orrery permissions explain` says and what `orrery run` does are the same
//! answer. A permission system that reports a denial it does not enforce is
//! worse than none — it tells an operator they are protected when they are not
//! — and the only way to catch that is to ask both halves of the binary about
//! the same call and compare.
//!
//! Every run here is sandboxed: `HOME`, `USERPROFILE` and `ProgramData` point
//! inside a temporary directory, the workspace is a temporary directory, and
//! the model is a committed fixture stream. **No network request is possible.**

mod common;

use std::path::Path;

use common::{args, base, jsonl, orrery_in, quiet, stream};

/// The call the fixture turn actually makes: `builtin.read` on `Cargo.toml`,
/// which `common::workspace` writes. Spelled as a call in the rule grammar,
/// which is what `permissions explain` takes.
const CALL: &str = "read(./Cargo.toml)";

/// A home with a user layer, and nothing else in it.
fn home(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary home");
    std::fs::create_dir_all(dir.path().join(".orrery")).expect("the user directory");
    std::fs::write(dir.path().join(".orrery/config.toml"), config).expect("the user config");
    dir
}

/// Write a managed layer inside the sandboxed `ProgramData`.
fn managed(home: &Path, config: &str) {
    let dir = home.join("ProgramData/Orrery");
    std::fs::create_dir_all(&dir).expect("the managed directory");
    std::fs::write(dir.join("managed.toml"), config).expect("the managed layer");
}

/// Write the workspace's own layer, `<root>/.orrery/config.toml`.
fn workspace_layer(ws: &Path, config: &str) {
    std::fs::create_dir_all(ws.join(".orrery")).expect("the workspace directory");
    std::fs::write(ws.join(".orrery/config.toml"), config).expect("the workspace layer");
}

/// What `permissions explain` says about [`CALL`], as the verdict word.
fn explain(home: &Path, ws: &Path, profile: &[&str]) -> String {
    let mut rest = vec!["--json"];
    rest.extend_from_slice(profile);
    rest.extend_from_slice(&["permissions", "explain", CALL]);
    let out = orrery_in(home, &args(&quiet(ws), &rest));
    assert!(
        out.status.success(),
        "explaining is not a failure: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    jsonl(&out.stdout)[0]["verdict"]
        .as_str()
        .expect("a verdict")
        .to_owned()
}

/// Run the fixture turn that reads `Cargo.toml`, and hand back the exit code.
fn run(home: &Path, ws: &Path, profile: &[&str]) -> i32 {
    let mut argv = base(ws, &["tool-call.jsonl", "text-turn.jsonl"]);
    argv.extend(profile.iter().map(|s| (*s).to_owned()));
    argv.extend(["run".to_owned(), "-p".to_owned(), "read it".to_owned()]);
    let out = orrery_in(home, &argv);
    out.status.code().expect("the binary exits")
}

/// Every decision the run left in the ledger, as printed.
fn ledger(home: &Path, ws: &Path) -> String {
    let out = orrery_in(home, &args(&base(ws, &[]), &["ledger"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// **The invariant.** `explain` and `run` answer the same question the same
/// way: a call `explain` calls `deny` is one `run` refuses (exit 4, per
/// `17-cli.md`'s table), and one it calls `allow` is one `run` performs.
fn they_agree(home: &Path, ws: &Path, profile: &[&str], expected: &str) {
    let verdict = explain(home, ws, profile);
    assert_eq!(verdict, expected, "the layers say what the test set up");
    let code = run(home, ws, profile);
    let text = ledger(home, ws);
    match verdict.as_str() {
        "deny" => {
            assert_eq!(
                code, 4,
                "`explain` said deny, so the run must be denied; the ledger was:\n{text}"
            );
            assert!(
                text.lines().any(|l| l.starts_with("deny")),
                "and the denial is written down: {text}"
            );
        }
        "allow" => {
            assert_eq!(
                code, 0,
                "`explain` said allow, so the run must go through; the ledger was:\n{text}"
            );
            assert!(
                text.lines().any(|l| l.starts_with("allow")),
                "and the decision is written down: {text}"
            );
        }
        other => panic!("unexpected verdict `{other}`"),
    }
}

/// The user layer denies a read. The same binary must refuse it.
#[test]
fn a_user_deny_is_enforced() {
    let home = home(
        "[permissions]\n\
         allow = [\"tool(*)\", \"write(./**)\", \"spawn(*)\"]\n\
         deny = [\"read(./**)\"]\n",
    );
    let ws = common::workspace();
    they_agree(home.path(), ws.path(), &[], "deny");
}

/// The other half of the equivalence: the same shape of layer, allowing, and a
/// run that goes through. Without this the test above would pass on a harness
/// that denied everything.
#[test]
fn a_user_allow_is_honoured() {
    let home = home(
        "[permissions]\n\
         allow = [\"tool(*)\", \"read(./**)\", \"write(./**)\", \"spawn(*)\"]\n",
    );
    let ws = common::workspace();
    they_agree(home.path(), ws.path(), &[], "allow");
}

/// A workspace that nobody configured still works: with no `[permissions]` in
/// any layer the built-in default is in force — read, write and spawn inside
/// the workspace — and `explain` answers from that same default.
#[test]
fn no_layer_at_all_is_the_default_in_both_halves() {
    let home = home("");
    let ws = common::workspace();
    they_agree(home.path(), ws.path(), &[], "allow");
}

/// The workspace's own layer denies it, which needs trust — granted from the
/// user layer, the only place a trust claim is believed.
#[test]
fn a_workspace_layer_deny_is_enforced() {
    let home = home("trust.auto = true\n");
    let ws = common::workspace();
    workspace_layer(
        ws.path(),
        "[permissions]\n\
         allow = [\"tool(*)\", \"write(./**)\", \"spawn(*)\"]\n\
         deny = [\"read(./**)\"]\n",
    );
    they_agree(home.path(), ws.path(), &[], "deny");
}

/// Deny is a union and a managed deny is final: the user layer allowing the
/// read does not relax what the managed layer refused, in either half.
#[test]
fn a_managed_deny_beats_a_user_allow_in_both_halves() {
    let home = home(
        "[permissions]\n\
         allow = [\"tool(*)\", \"read(./**)\", \"write(./**)\", \"spawn(*)\"]\n",
    );
    managed(home.path(), "[permissions]\ndeny = [\"read(./**)\"]\n");
    let ws = common::workspace();
    they_agree(home.path(), ws.path(), &[], "deny");
}

/// A profile's shorthand is a rule like any other, and `--profile` is not
/// decoration: `read = false` under `[profile.locked]` denies the read in the
/// explanation and in the run.
#[test]
fn a_profile_shorthand_is_enforced() {
    let home = home(
        "[profile.locked]\n\
         permissions = { read = false }\n",
    );
    let ws = common::workspace();
    they_agree(home.path(), ws.path(), &["--profile", "locked"], "deny");
}

/// And a profile that allows is a profile that runs, from the same file: two
/// profiles, one binary, measurably different answers.
#[test]
fn a_profile_that_allows_still_runs() {
    let home = home(
        "[profile.open]\n\
         permissions = { read = true }\n",
    );
    let ws = common::workspace();
    they_agree(home.path(), ws.path(), &["--profile", "open"], "allow");
}

/// Every example `permissions explain --help` shows parses as a call. Help text
/// that documents what the parser rejects is a defect in its own right.
#[test]
fn every_example_in_the_help_parses() {
    let home = home("");
    let ws = common::workspace();
    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["permissions", "explain", "--help"]),
    );
    assert!(out.status.success(), "`--help` succeeds");
    let text = String::from_utf8_lossy(&out.stdout);

    let examples: Vec<String> = text
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|s| s.contains('(') && s.ends_with(')'))
        .map(str::to_owned)
        .collect();
    assert!(
        !examples.is_empty(),
        "the help shows at least one example: {text}"
    );
    for example in &examples {
        let out = orrery_in(
            home.path(),
            &args(&quiet(ws.path()), &["permissions", "explain", example]),
        );
        assert!(
            out.status.success(),
            "`{example}` is documented, so it must parse: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Belt and braces: the stream this file drives is the committed one, so a
/// rename fails here rather than silently skipping the run half.
#[test]
fn the_fixture_stream_is_there() {
    assert!(stream("tool-call.jsonl").is_file());
}
