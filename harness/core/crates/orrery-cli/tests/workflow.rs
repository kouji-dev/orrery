//! `orrery workflow` — plan 11, reachable from the product.
//!
//! Round 5 left `orrery-router` and `orrery-orchestrator` out of
//! `cargo tree -p orrery-cli` entirely. Every criterion in
//! `11-router-roles-orchestrator.md` was true as a **library** test and
//! unreachable as a product: the five roles loaded, appeared in the ledger and
//! bound, and nothing in the binary could route to one or run a step.
//!
//! The headline criterion is the one this file drives from the binary: **a
//! verify loop terminates on its own cap**, on a counter the orchestrator owns
//! rather than on a prompt's good behaviour.
//!
//! No network, no key: every model call is the committed fixture stream.

mod common;

use common::{args, base, jsonl, orrery, workspace};

/// A verify loop whose predicate never holds. `max_iterations` is the only
/// thing that can stop it, which is the point.
const NEVER_PASSES: &str = r#"
name = "verify"
budget = { max_turns = 100, max_tokens = 10000000, wall_clock_ms = 600000 }

[[step]]
name = "verify"
kind = "loop"
max_iterations = 3
until = { cmp = { lhs = { ref = "fix" }, op = "eq", rhs = "it will never say this" } }

[[step.body]]
name = "fix"
kind = "agent"
subagent = "executor"
input = "make the tests pass"
"#;

fn write(dir: &std::path::Path, name: &str, text: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("the workflow file is written");
    path.display().to_string()
}

/// **The phase-6 criterion, from the binary.** A loop whose gate never passes
/// stops at `max_iterations`, says so, and exits 0 — a capped loop is an
/// ordinary outcome, not a failure and not a hang.
#[test]
fn a_verify_loop_terminates_on_its_own_cap() {
    let dir = workspace();
    let file = write(dir.path(), "verify.toml", NEVER_PASSES);

    let out = orrery(&args(
        &base(dir.path(), &["text-turn.jsonl"]),
        &["--json", "workflow", "run", &file],
    ));
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let report = jsonl(&out.stdout)
        .pop()
        .expect("the run reports itself on stdout");
    assert_eq!(report["outcome"], "completed", "{report}");
    let cap = report["caps"]
        .as_array()
        .expect("caps is a list")
        .first()
        .expect("the loop that ended on its cap is named");
    assert_eq!(cap["step"], "verify");
    assert_eq!(cap["iterations"], 3, "exactly max_iterations, no more");
}

/// An invalid workflow fails at **load**, before a single model call is paid
/// for — and `check` is the verb that asks without running anything.
#[test]
fn an_invalid_workflow_fails_at_load() {
    let dir = workspace();
    // `later` runs after `early`, so `early` cannot refer to it.
    let file = write(
        dir.path(),
        "bad.toml",
        r#"
name = "bad"

[[step]]
name = "early"
kind = "agent"
subagent = "executor"
input = { ref = "later" }

[[step]]
name = "later"
kind = "agent"
subagent = "executor"
input = "anything"
"#,
    );

    let out = orrery(&args(
        &["--workspace".to_owned(), dir.path().display().to_string()],
        &["workflow", "check", &file],
    ));
    assert_eq!(out.status.code(), Some(2), "the person wrote it");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("later"), "{stderr}");
    assert!(
        out.stdout.is_empty(),
        "stdout is data, and a workflow that will not load has none"
    );
}

/// `check` needs no model at all. Asking whether a file is well-formed must
/// work in a checkout with no provider in sight, for the same reason
/// `config explain` does.
#[test]
fn check_needs_no_provider() {
    let dir = workspace();
    let file = write(dir.path(), "verify.toml", NEVER_PASSES);
    let out = orrery(&args(
        &["--workspace".to_owned(), dir.path().display().to_string()],
        &["workflow", "check", &file],
    ));
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("verify"), "it names the workflow: {stdout}");
}

/// A `tool` step makes **no model call**, which is where the cost comes out.
///
/// A provider is still named — building a session builds a kernel, and a kernel
/// has a provider whether or not anything asks it anything — and the assertion
/// is that it was never *used*: zero tokens, and the file's real contents in
/// the step's value.
#[test]
fn a_tool_only_workflow_needs_no_model() {
    let dir = workspace();
    let file = write(
        dir.path(),
        "read.toml",
        &format!(
            r#"
name = "read-it"

[[step]]
name = "contents"
kind = "tool"
ref = "builtin.read"
input = {{ path = {target:?} }}
"#,
            target = common::TARGET
        ),
    );

    let out = orrery(&args(
        &base(dir.path(), &["text-turn.jsonl"]),
        &["--json", "workflow", "run", &file],
    ));
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = jsonl(&out.stdout).pop().expect("a report");
    assert_eq!(report["outcome"], "completed", "{report}");
    assert_eq!(
        report["usage"]["input_tokens"], 0,
        "no model was called: {report}"
    );
    assert!(
        report["steps"]["contents"]
            .to_string()
            .contains("the-file-the-tool-really-read"),
        "the tool really ran: {report}"
    );
}

/// A `[[route]]` rule in the workspace's own `orrery.toml` reaches the run.
///
/// This is the router in the product rather than in its own test suite: the
/// rule denies the sub-agent rung, so the step fails with the router's words
/// and nothing is paid for.
#[test]
fn a_declared_route_rule_reaches_the_run() {
    let dir = workspace();
    std::fs::write(
        dir.path().join("orrery.toml"),
        "[permissions]\n\
         allow = [\"tool(*)\", \"read(./**)\", \"write(./**)\", \"spawn(*)\"]\n\
         \n\
         [[route]]\n\
         name = \"no-children\"\n\
         deny = \"sub-agent\"\n\
         reason = \"this workspace runs one agent\"\n",
    )
    .expect("the workspace config is written");
    let file = write(dir.path(), "verify.toml", NEVER_PASSES);

    let out = orrery(&args(
        &base(dir.path(), &["text-turn.jsonl"]),
        &["--json", "workflow", "run", &file],
    ));
    let report = jsonl(&out.stdout).pop().expect("a report");
    assert_eq!(report["outcome"], "failed", "{report}");
    assert!(
        report["message"]
            .as_str()
            .unwrap_or_default()
            .contains("this workspace runs one agent"),
        "the router's own words reach the person: {report}"
    );
}

/// **A sub-agent's work is turn rows on its own branch**, and the parent wrote
/// the join.
///
/// Plan 11's fourth invariant, from the binary: three iterations of the verify
/// loop leave three child branches beside the root, and `session show` prints
/// the parent's `BranchResult` row for each. A sub-agent collapsed into one
/// opaque tool result would show one branch and no joins.
#[test]
fn a_sub_agent_leaves_its_own_branch_behind() {
    let dir = workspace();
    let file = write(dir.path(), "verify.toml", NEVER_PASSES);
    let base = base(dir.path(), &["text-turn.jsonl"]);

    let out = orrery(&args(&base, &["workflow", "run", &file]));
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let listed = orrery(&args(&base, &["session", "list"]));
    let listed = String::from_utf8_lossy(&listed.stdout);
    let session = listed
        .lines()
        .find_map(|l| l.split_whitespace().next())
        .expect("the run opened a session");

    let shown = orrery(&args(&base, &["session", "show", session]));
    let shown = String::from_utf8_lossy(&shown.stdout);
    assert!(
        shown.contains("branches  4"),
        "the root and one branch per iteration: {shown}"
    );
    assert_eq!(
        shown.matches("completed]").count(),
        3,
        "one join row per child, written by the parent: {shown}"
    );
}
