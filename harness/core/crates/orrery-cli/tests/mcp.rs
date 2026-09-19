//! `orrery mcp list | tools`.
//!
//! `orrery-mcp` was built, tested and outside the binary's dependency closure,
//! which is why section 8 phase 7 read PARTIAL. These tests are how it stops
//! being: they drive the **binary** against config the test wrote, in a
//! sandboxed home, with no network anywhere.
//!
//! **What is not here: a successful handshake against a real server.** That is
//! `orrery-mcp`'s own `client::real_server_works_unmodified`, which spawns the
//! committed spec-conformant fixture server — a binary this crate's test
//! harness does not build. What *is* here is everything the command adds on top
//! of that: the declaration is found, the layer is named, discovery starts
//! nothing, and a server that will not start fails with a sentence.

mod common;

use common::{args, home_with, jsonl, orrery_in, quiet};

/// §4.11: a server registers as extension id `mcp.<server>` and is discovered
/// without being started.
#[test]
fn list_shows_what_configuration_declares() {
    let home = home_with(
        "[mcp_servers.jira]\n\
         command = \"jira-mcp\"\n\
         args = [\"--stdio\"]\n",
    );
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["--json", "mcp", "list"]),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows = jsonl(&out.stdout);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["server"], "jira");
    assert_eq!(
        rows[0]["ext"], "mcp.jira",
        "no second namespacing scheme: it is an extension id like any other"
    );
    assert_eq!(rows[0]["layer"], "user", "the layer that declared it");
    assert_eq!(rows[0]["transport"], "stdio");
    assert_eq!(
        rows[0]["health"], "discovered",
        "discovered, and **not** connected: listing starts no process"
    );
}

/// Nothing declared is an answer, on stderr, with clean stdout.
#[test]
fn nothing_declared_says_so() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(home.path(), &args(&quiet(ws.path()), &["mcp", "list"]));
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "stdout is data, and there is none");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no MCP servers declared"),
        "and it says how to declare one"
    );
}

/// A server declared with no `command` is reported rather than silently
/// dropped: a server nobody can start is exactly what this command is for.
#[test]
fn a_server_without_a_command_is_reported() {
    let home = home_with("[mcp_servers.broken]\nargs = [\"--stdio\"]\n");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(home.path(), &args(&quiet(ws.path()), &["mcp", "list"]));
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("broken"), "it names the server: {stderr}");
    assert!(stderr.contains("command"), "and what is missing: {stderr}");
}

/// Asking about a server nobody declared is the person's mistake, and points
/// at the command that would have told them.
#[test]
fn tools_of_an_undeclared_server_is_usage() {
    let home = home_with("[mcp_servers.jira]\ncommand = \"jira-mcp\"\n");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["mcp", "tools", "github"]),
    );
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("github"), "{stderr}");
    assert!(stderr.contains("mcp list"), "{stderr}");
    assert!(out.stdout.is_empty(), "stdout is data, and there is none");
}

/// A declared server whose program is not there fails with a sentence rather
/// than a panic or a hang — the failure a person actually hits.
#[test]
fn a_server_that_will_not_start_says_so() {
    let home = home_with(
        "[mcp_servers.ghost]\n\
         command = \"orrery-no-such-program-exists\"\n",
    );
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["mcp", "tools", "ghost"]),
    );
    assert!(!out.status.success(), "it does not claim success");
    assert_ne!(out.status.code(), None, "and it does not crash");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.starts_with("orrery:"),
        "a sentence, not a backtrace: {stderr}"
    );
}
