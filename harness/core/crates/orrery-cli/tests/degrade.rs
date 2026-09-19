//! **Section 8, phase 3, through the binary.** An extension whose declared
//! `requires` are refused by policy loads *Degraded*: the tool that needed the
//! refused capability is absent from the set the model is offered, every other
//! tool it brought still works, the reason is in the load ledger, and the
//! session runs to the end.
//!
//! Why this file exists at all: the criterion had an implementation
//! (`orrery_broker::install`) whose only caller was its own test, while the run
//! path handed every extension `Capability::all` for `spawn` and so could never
//! be missing it. A driven acceptance run proved the gap — `ext list` said
//! `ok`, the turn dispatched the spawn-needing tool, and the ledger recorded
//! `allow`. Nothing but the binary can hold that ground, so this asks the
//! binary.
//!
//! The rules go in the **user layer**, which is believed without a trust
//! decision, so what these tests measure is the policy gate and not the trust
//! gate.
//!
//! No model, no network, no key: the provider is a committed `.jsonl`.

mod common;

use std::path::Path;
use std::process::Output;

use common::{args, base, home_with, orrery_in, workspace};

/// A user layer that refuses `spawn` to everything.
///
/// `tool(*)`, `read` and `write` stay allowed, so whatever is left out of the
/// offered set is left out for the one reason under test.
const DENIES_SPAWN: &str = "[permissions]
allow = [\"tool(*)\", \"read(./**)\", \"write(./**)\"]
deny = [\"spawn(*)\"]
";

/// The everyday default, spelled out, as the control.
const ALLOWS_SPAWN: &str = "[permissions]
allow = [\"tool(*)\", \"read(./**)\", \"write(./**)\", \"spawn(*)\"]
";

/// Run the binary in a home whose user layer carries `rules`.
fn run(home: &Path, ws: &Path, rest: &[&str]) -> Output {
    orrery_in(home, &args(&base(ws, &[]), rest))
}

/// The `orrery.kernel.context` line of one real turn: what the model was told
/// it had.
fn offered(home: &Path, ws: &Path) -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(args(&base(ws, &["text-turn.jsonl"]), &["run", "-p", "hello"]))
        .env("COLUMNS", "400")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ProgramData", home.join("ProgramData"))
        .env("RUST_LOG", "orrery.kernel.context=debug")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the orrery binary runs");
    assert!(
        out.status.success(),
        "a degraded extension must not take the session down: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = strip_ansi(&String::from_utf8_lossy(&out.stderr));
    let line = log
        .lines()
        .find(|l| l.contains("the system prompt this pass carries"))
        .unwrap_or_else(|| panic!("the turn says what it carried: {log}"))
        .to_owned();
    line.split("offered=")
        .nth(1)
        .unwrap_or_else(|| panic!("the line names what was offered: {line}"))
        .to_owned()
}

/// The control: nothing is denied, and the spawn-needing tool is offered.
#[test]
fn a_workspace_that_allows_spawn_offers_the_spawning_tool() {
    let home = home_with(ALLOWS_SPAWN);
    let ws = workspace();
    let offered = offered(home.path(), ws.path());
    assert!(
        offered.contains("bash"),
        "with `spawn` allowed, the tool that needs it is offered: {offered}"
    );
}

/// The criterion: `spawn` denied, so `builtin.bash` is not offered — and the
/// rest of the bundle is.
#[test]
fn a_denied_spawn_leaves_out_only_the_tool_that_needed_it() {
    let home = home_with(DENIES_SPAWN);
    let ws = workspace();
    let offered = offered(home.path(), ws.path());

    assert!(
        !offered.contains("bash"),
        "a tool needing a refused capability must not be offered: {offered}"
    );
    for kept in ["read", "write", "grep", "glob"] {
        assert!(
            offered.contains(kept),
            "`{kept}` needs nothing that was refused, so it stays: {offered}"
        );
    }
}

/// ...and the load ledger says which extension degraded and why.
#[test]
fn the_load_ledger_holds_the_reason() {
    let home = home_with(DENIES_SPAWN);
    let ws = workspace();
    // One turn, so there is a session to read a ledger for.
    let turn = orrery_in(
        home.path(),
        &args(
            &base(ws.path(), &["text-turn.jsonl"]),
            &["run", "-p", "hello"],
        ),
    );
    assert!(
        turn.status.success(),
        "the session survives the degrade: {}",
        String::from_utf8_lossy(&turn.stderr)
    );

    let out = run(home.path(), ws.path(), &["ledger", "--stream", "load"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("builtin degraded"),
        "the load stream records the degrade: {text}"
    );
    assert!(
        text.contains("bash") && text.contains("spawn"),
        "and names the tool and the capability it lost: {text}"
    );
}

/// `ext list` must not answer `ok` for a bundle policy has cut down. This is
/// the exact sentence the acceptance run caught being false.
#[test]
fn ext_list_does_not_say_ok_for_a_degraded_bundle() {
    let home = home_with(DENIES_SPAWN);
    let ws = workspace();
    let out = run(home.path(), ws.path(), &["ext", "list"]);
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.starts_with("builtin "))
        .unwrap_or_else(|| panic!("the builtin bundle is listed: {text}"));
    assert!(
        line.contains("degraded"),
        "a bundle whose `spawn` is refused is not `ok`: {text}"
    );
    assert!(
        text.contains("spawn"),
        "and says which capability it lost: {text}"
    );
}

/// Drop every ANSI escape sequence, so an assertion is about the words.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.next() != Some('[') {
            continue;
        }
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() {
                break;
            }
        }
    }
    out
}
