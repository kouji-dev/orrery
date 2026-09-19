//! The built-in permissions are a **floor**, not a value a layer replaces.
//!
//! DEFECT 9, reproduced three ways before this file existed: a config that
//! declared any permission rule at all replaced `DEFAULT_PERMISSIONS`
//! wholesale, `tool(*)` went with it, and the model was offered **no tools** —
//! while `permissions explain 'read(./Cargo.toml)'` cheerfully answered Allow.
//! The user was told their configuration was fine while the model silently had
//! nothing to work with.
//!
//! The decision, written into `harness/docs/plans/10-config-layers.md` and
//! `07-policy-broker-audit.md`: the built-in defaults are a floor **per
//! aspect**, and a layer's `allow`/`ask` for an aspect replaces that aspect's
//! floor and nothing else. A `deny` narrows the floor without removing it,
//! because a deny says what is refused, not what is permitted.
//!
//! Everything here drives the built binary. **No network request, no key.**

mod common;

use common::{args, base, home_with, orrery_in, quiet, workspace};

/// The `orrery.kernel.context` line of one real turn, from a home this test
/// built — so the user layer under it is in force.
fn context_line(home: &std::path::Path, ws: &std::path::Path) -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(args(
            &base(ws, &["text-turn.jsonl"]),
            &["run", "-p", "hello"],
        ))
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

/// The `tools=` count that line reports.
fn tool_count(line: &str) -> usize {
    line.split("tools=")
        .nth(1)
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or_else(|| panic!("the line carries a tool count: {line}"))
}

/// What that line says was offered.
fn offered(line: &str) -> String {
    line.split("offered=")
        .nth(1)
        .unwrap_or_else(|| panic!("the line names what was offered: {line}"))
        .to_owned()
}

/// (a) A **user** layer that names one permission used to empty the tool set.
#[test]
fn a_user_layer_that_only_names_read_keeps_its_tools() {
    let home = home_with("[permissions]\nallow = [\"read(./**)\"]\n");
    let ws = workspace();
    let line = context_line(home.path(), ws.path());
    assert!(
        tool_count(&line) > 0,
        "a config that says nothing about tools does not remove every tool: {line}"
    );
    assert!(
        offered(&line).contains("read"),
        "the built-in reader is still offered: {line}"
    );
}

/// (b) The same from the **workspace** layer, which needs trust to load at all.
#[test]
fn a_trusted_workspace_layer_that_only_names_read_keeps_its_tools() {
    let home = home_with("trust.auto = true\n");
    let ws = workspace();
    std::fs::create_dir_all(ws.path().join(".orrery")).expect("the workspace config directory");
    std::fs::write(
        ws.path().join(".orrery/config.toml"),
        "[permissions]\nallow = [\"read(./**)\"]\n",
    )
    .expect("the workspace config");

    let line = context_line(home.path(), ws.path());
    assert!(
        tool_count(&line) > 0,
        "a workspace config that says nothing about tools keeps them: {line}"
    );
}

/// The other half of the decision: a layer that **does** say something about
/// tools is obeyed exactly — including when what it says is "none of them".
#[test]
fn a_layer_that_names_tools_is_obeyed_exactly() {
    let ws = workspace();

    let one = home_with("[permissions]\nallow = [\"tool(builtin.read)\", \"read(./**)\"]\n");
    let line = context_line(one.path(), ws.path());
    assert_eq!(
        tool_count(&line),
        1,
        "naming one tool offers one tool, not the floor's `tool(*)`: {line}"
    );
    assert!(offered(&line).contains("read"), "{line}");

    let none = home_with("[permissions]\ndeny = [\"tool(*)\"]\n");
    let line = context_line(none.path(), ws.path());
    assert_eq!(
        tool_count(&line),
        0,
        "denying every tool denies every tool: {line}"
    );
}

/// A **deny** of one tool narrows the floor; it does not take the floor away.
///
/// The obvious wrong fix — "any mention of an aspect drops its floor" — turns a
/// managed `deny = [\"tool(shell.*)\"]` into a workspace with no tools at all.
#[test]
fn denying_one_tool_leaves_the_rest() {
    let home = home_with("[permissions]\ndeny = [\"tool(shell.exec)\"]\n");
    let ws = workspace();
    let line = context_line(home.path(), ws.path());
    assert!(
        tool_count(&line) > 0,
        "refusing one tool is not refusing every tool: {line}"
    );
    assert!(
        !offered(&line).contains("shell.exec"),
        "and the one that was refused is gone: {line}"
    );
}

/// (c) The file `orrery init` writes, driven the way a person drives it:
/// `init`, then `trust grant`, then a turn.
#[test]
fn the_file_init_writes_produces_a_working_first_turn() {
    let home = home_with("");
    let ws = workspace();

    let out = orrery_in(home.path(), &args(&quiet(ws.path()), &["init"]));
    assert!(
        out.status.success(),
        "init writes a config: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = orrery_in(home.path(), &args(&quiet(ws.path()), &["trust", "grant"]));
    assert!(
        out.status.success(),
        "the workspace can be trusted from the CLI: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let line = context_line(home.path(), ws.path());
    assert!(
        tool_count(&line) > 0,
        "the first file a new user gets is not the file that took their tools away: {line}"
    );
}

/// `permissions explain` never says Allow for a call the same binary would then
/// have no tool for. Both answers come out of one builder.
#[test]
fn explain_and_the_offered_set_answer_from_one_floor() {
    let home = home_with("[permissions]\nallow = [\"read(./**)\"]\n");
    let ws = workspace();

    let out = orrery_in(
        home.path(),
        &args(
            &quiet(ws.path()),
            &["permissions", "explain", "tool(builtin.read)"],
        ),
    );
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        said.contains("Allow"),
        "the tool floor is what explain reports too: {said}"
    );

    let line = context_line(home.path(), ws.path());
    assert!(
        offered(&line).contains("read"),
        "and the turn really offers it: {line}"
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
