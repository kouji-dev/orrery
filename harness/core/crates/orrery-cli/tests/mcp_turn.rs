//! Plan 13 on the **run path**: a real turn calls an MCP tool, and the same
//! turn carries a discovered `SKILL.md`.
//!
//! # Why this file is separate from `tests/mcp.rs`
//!
//! `tests/mcp.rs` drives `orrery mcp list | tools` — the **inspection**
//! commands. Those were green while the product was broken: `orrery mcp tools
//! fixture` performed a real handshake and listed `mcp.fixture.echo`, and a
//! turn calling that name answered "no-such-tool: there is no tool called
//! `mcp.fixture.echo` in this agent's tool set". `orrery-cli` depended on
//! `orrery-mcp` and `orrery-skills`; `orrery-harness`, which builds what a turn
//! runs, depended on neither. An inspection command that sees a subsystem the
//! run path cannot is the one defect this round exists to stop, so the proof
//! has to be a **turn**, not a listing.
//!
//! # The server is real, and so is the protocol
//!
//! `mcp_fixture_server` is `orrery-mcp`'s own committed spec-conformant server,
//! built as an example of this crate so a test here can spawn one (cargo does
//! not build a dependency's binaries). It speaks newline-delimited JSON-RPC
//! 2.0 over stdio and knows nothing about Orrery. **No network, no key**: the
//! model is a `.jsonl` stream this file writes.

mod common;

use std::path::{Path, PathBuf};

use common::{args, jsonl, orrery_in};

/// The fixture MCP server, built beside this test binary.
fn server_exe() -> PathBuf {
    let mut dir = std::env::current_exe().expect("a test binary knows where it is");
    dir.pop();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join("examples")
        .join(format!("mcp_fixture_server{}", std::env::consts::EXE_SUFFIX))
}

/// A home declaring that server, with an optional skill beside it.
fn home(skill: Option<(&str, &str)>) -> tempfile::TempDir {
    let exe = server_exe();
    assert!(
        exe.exists(),
        "the mcp_fixture_server example must be built beside the test: {}",
        exe.display()
    );
    let dir = tempfile::tempdir().expect("a temporary home");
    std::fs::create_dir_all(dir.path().join(".orrery")).expect("the user directory");
    std::fs::write(
        dir.path().join(".orrery/config.toml"),
        // Forward slashes: a Windows path in TOML is full of escapes otherwise,
        // and every Windows API here takes either.
        format!(
            "[mcp_servers.fixture]\ncommand = \"{}\"\n",
            exe.display().to_string().replace('\\', "/")
        ),
    )
    .expect("the user config");
    if let Some((name, body)) = skill {
        let root = dir.path().join(".orrery/skills").join(name);
        std::fs::create_dir_all(&root).expect("the skill directory");
        std::fs::write(
            root.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {body}\n---\n\n{body}\n"),
        )
        .expect("the skill file");
    }
    dir
}

/// A workspace with a model stream in it that calls one MCP tool.
fn calling(tool: &str, input: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    let call = "0192f3a0-0000-7000-8000-0000000000b1";
    std::fs::write(
        dir.path().join("call.jsonl"),
        format!(
            "{{\"t\":\"started\",\"id\":\"msg_mcp\"}}\n\
             {{\"t\":\"tool-use-start\",\"call\":\"{call}\",\"name\":\"{tool}\"}}\n\
             {{\"t\":\"tool-use-delta\",\"call\":\"{call}\",\"json_fragment\":{}}}\n\
             {{\"t\":\"tool-use-end\",\"call\":\"{call}\"}}\n\
             {{\"t\":\"done\",\"stop\":\"tool-use\"}}\n",
            serde_json::Value::String(input.to_owned())
        ),
    )
    .expect("the model stream");
    dir
}

/// The global flags: that workspace, its own state, and two passes — the tool
/// call, then a plain answer so the turn ends.
fn flags(ws: &Path) -> Vec<String> {
    vec![
        "--workspace".to_owned(),
        ws.display().to_string(),
        "--state-dir".to_owned(),
        ws.join(".orrery").display().to_string(),
        "--provider".to_owned(),
        format!("fixture:{}", ws.join("call.jsonl").display()),
        "--provider".to_owned(),
        format!("fixture:{}", common::stream("text-turn.jsonl").display()),
    ]
}

/// Phase 7's criterion, through the binary: the turn **calls** the MCP tool and
/// gets the server's own answer back.
#[test]
fn a_real_turn_calls_a_declared_mcp_tool() {
    let home = home(None);
    let ws = calling("mcp.fixture.echo", r#"{"text":"the run path can call it"}"#);

    let out = orrery_in(
        home.path(),
        &args(&flags(ws.path()), &["--json", "run", "-p", "echo it"]),
    );
    assert!(
        out.status.success(),
        "the turn runs: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let frames = jsonl(&out.stdout);
    let result = frames
        .iter()
        .find(|f| f["type"] == "TOOL_CALL_RESULT")
        .unwrap_or_else(|| panic!("the call produced a result: {frames:#?}"));
    assert_eq!(
        result["outcome"]["t"], "ok",
        "and it is the server's answer, not `no-such-tool`: {result}"
    );
    assert!(
        result["content"]
            .as_str()
            .is_some_and(|c| c.contains("the run path can call it")),
        "the fixture server echoed: {result}"
    );
}

/// The same declaration, seen by the audit stream: an MCP server contributes
/// its tools through the ordinary extension door, as `mcp.<server>`.
///
/// The load stream is the one `orrery ext list` reads and `orrery ledger` does
/// not, so this reads the `.jsonl` the run itself wrote.
#[test]
fn the_admitted_tools_are_recorded_as_an_extension_load() {
    let home = home(None);
    let ws = calling("mcp.fixture.echo", r#"{"text":"hello"}"#);
    let out = orrery_in(home.path(), &args(&flags(ws.path()), &["run", "-p", "echo"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let audit = ws.path().join(".orrery/audit");
    let mut lines = String::new();
    for entry in std::fs::read_dir(&audit).expect("the run wrote an audit directory") {
        let path = entry.expect("a directory entry").path();
        lines.push_str(&std::fs::read_to_string(&path).expect("an audit stream"));
    }
    assert!(
        lines.contains(r#""ext":"mcp.fixture""#) && lines.contains("mcp.fixture.echo"),
        "the server's contribution is in the load stream: {lines}"
    );
}

/// A tool the server does not have is refused by name, like any other: the MCP
/// host is behind the ordinary registry, not beside it.
#[test]
fn an_undeclared_mcp_tool_is_still_no_such_tool() {
    let home = home(None);
    let ws = calling("mcp.fixture.invented", r#"{"text":"nope"}"#);

    let out = orrery_in(
        home.path(),
        &args(&flags(ws.path()), &["--json", "run", "-p", "try it"]),
    );
    assert!(out.status.success(), "the turn still completes");
    let frames = jsonl(&out.stdout);
    let result = frames
        .iter()
        .find(|f| f["type"] == "TOOL_CALL_RESULT")
        .expect("the call settled");
    assert_eq!(result["outcome"]["code"], "no-such-tool", "{result}");
}

/// The other half of plan 13: a `SKILL.md` on disk reaches the **same turn**.
///
/// `KernelConfig::skills` has rendered section 4 of the system prompt since
/// plan 05 and nothing filled it, so `orrery skills list` printed files the
/// model was never given. The kernel says what it is carrying on
/// `orrery.kernel.context`, which is the only place that knows.
#[test]
fn a_discovered_skill_reaches_the_same_turn() {
    let home = home(Some(("haiku", "Five, seven, five, and never rhyme")));
    let ws = calling("mcp.fixture.echo", r#"{"text":"with a skill in scope"}"#);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(args(&flags(ws.path()), &["--json", "run", "-p", "echo it"]))
        .env("COLUMNS", "100")
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("ProgramData", home.path().join("ProgramData"))
        .env("RUST_LOG", "orrery.kernel.context=debug")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the orrery binary runs");
    assert!(out.status.success());

    // The subscriber colours its output, so `skills=1` is not a substring of
    // the raw bytes: the escapes come out first.
    let raw = String::from_utf8_lossy(&out.stderr).to_string();
    let log = strip_ansi(&raw);
    assert!(
        log.contains("skills=1"),
        "the turn carries the discovered skill: {log}"
    );
    assert!(
        log.contains("skills("),
        "as a section of the system prompt, with its text in it: {log}"
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
