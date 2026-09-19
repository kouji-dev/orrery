//! The tool list a real turn offers the model, in a workspace nobody configured.
//!
//! Every fixture test in this crate was blind to this: `resolve` does not
//! consult policy, so dispatch worked, and the fixture provider replays the
//! tool calls in its stream whatever the prompt offered. A real model would
//! have been told it had no tools at all — because the offered set is filtered
//! through `PolicyEngine::explain` for `agent:<name>`, and `explain` denied
//! every subject nobody had written a rule about while `check` allowed them.
//!
//! So this asserts the offered set from the running binary, on the run path,
//! from `orrery.kernel.context` — the only place that knows what the model was
//! actually told.

mod common;

use common::{args, base, sandbox_home, workspace};

/// The `orrery.kernel.context` line of one real turn, with the colour stripped.
fn context_line(ws: &std::path::Path, stream: &str) -> String {
    let home = sandbox_home();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(args(&base(ws, &[stream]), &["run", "-p", "hello"]))
        .env("COLUMNS", "200")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ProgramData", home.join("ProgramData"))
        .env("RUST_LOG", "orrery.kernel.context=debug")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the orrery binary runs");
    assert!(
        out.status.success(),
        "the fixture turn runs: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = strip_ansi(&String::from_utf8_lossy(&out.stderr));
    log.lines()
        .find(|l| l.contains("the system prompt this pass carries"))
        .unwrap_or_else(|| panic!("the turn says what it carried: {log}"))
        .to_owned()
}

/// A workspace with no configuration at all still offers the model tools.
#[test]
fn a_default_workspace_offers_tools() {
    let ws = workspace();
    let line = context_line(ws.path(), "text-turn.jsonl");

    assert!(
        !line.contains("tools=0"),
        "a workspace nobody configured is not a workspace with no tools: {line}"
    );
    let count = line
        .split("tools=")
        .nth(1)
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or_else(|| panic!("the line carries a tool count: {line}"));
    assert!(count > 0, "the model is offered something: {line}");

    let offered = line
        .split("offered=")
        .nth(1)
        .unwrap_or_else(|| panic!("the line names what was offered: {line}"));
    assert!(
        offered.contains("read"),
        "the built-in reader is among them: {line}"
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
