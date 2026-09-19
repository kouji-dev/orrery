//! The six tools a default turn offers, through the door a third party gets.
//!
//! Every test here loads the bundle with
//! [`orrery_ext_api::testing::load_for_test`] and drives it against
//! [`MockBroker`](orrery_ext_api::testing::MockBroker) — the same harness
//! `orrery ext test` runs, and the one plan 18 points a community author at.
//! So this file is the worked example as well as the suite: nothing in it
//! names a kernel crate, and `deps-check` rule 3 is satisfied by construction.
//!
//! The tests of the *broker* — a ceiling that bounds peak memory, a write that
//! reverts on cancel, a child whose grandchild dies with it — live in
//! `orrery-harness/tests/builtin.rs`, because a broker is what this crate may
//! not name. These are the tests of the *tools*: what each one asks the broker
//! for, and what it does with every answer the broker can give.

use orrery_ext_api::testing::{BrokerCall, TestHarness, load_for_test};
use orrery_ext_api::{NativeExtension, ToolBudget};
use orrery_ext_tools_builtin::{BuiltinTools, MANIFEST};
use orrery_proto::Outcome;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// The workspace every test pretends to run in.
const WS: &str = "/ws";

/// A bundle loaded with the grants its own manifest asks for, resolved to the
/// test's workspace.
fn rig() -> TestHarness {
    load_for_test(MANIFEST, &["read:/ws/**", "write:/ws/**", "spawn:*"])
        .expect("the bundle's own orrery.toml parses with the parser a third party is held to")
}

/// A bundle that may read but not write, and may not spawn.
fn read_only() -> TestHarness {
    load_for_test(MANIFEST, &["read:/ws/**"])
        .expect("the bundle's own orrery.toml parses with the parser a third party is held to")
}

/// The text an outcome shows.
fn text_of(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Ok { value, .. } => value
            .as_ref()
            .and_then(|v| v.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        Outcome::Failed { message, .. } => message.clone(),
        other => panic!("expected something with text in it, got {other:?}"),
    }
}

/// Call one tool.
async fn call(harness: &TestHarness, tool: &str, input: Value) -> Outcome {
    BuiltinTools::new()
        .call(tool, input, &harness.ctx(tool))
        .await
        .expect("the harness carries the call; a refusal is a value, not an error")
}

/// Call one tool under a ceiling and a cancel token of the test's choosing.
async fn call_with(
    harness: &TestHarness,
    tool: &str,
    input: Value,
    budget: ToolBudget,
    cancel: CancellationToken,
) -> Outcome {
    BuiltinTools::new()
        .call(tool, input, &harness.ctx_with(tool, budget, cancel))
        .await
        .expect("the harness carries the call; a refusal is a value, not an error")
}

// --- the manifest and the tool table agree --------------------------------

#[test]
fn every_promised_tool_is_offered() {
    let harness = rig();
    let mut promised: Vec<String> = harness.manifest().provides.tools.to_vec();
    promised.sort();
    let mut offered: Vec<String> = BuiltinTools::new()
        .tools()
        .into_iter()
        .map(|t| t.name.clone())
        .collect();
    offered.sort();
    assert_eq!(
        offered, promised,
        "the manifest promises exactly what the bundle offers"
    );
}

// --- read -----------------------------------------------------------------

#[tokio::test]
async fn read_returns_the_file_through_the_broker() {
    let harness = rig().with_file(format!("{WS}/a.txt"), "hello");
    let outcome = call(&harness, "read", json!({ "path": format!("{WS}/a.txt") })).await;
    assert_eq!(text_of(&outcome), "hello");
    assert!(
        matches!(
            harness.recorded().as_slice(),
            [BrokerCall::Read { allowed: true, .. }]
        ),
        "one read, through the broker: {:?}",
        harness.recorded()
    );
}

#[tokio::test]
async fn read_stops_at_the_ceiling_rather_than_reading_to_the_end() {
    let harness = rig().with_file(format!("{WS}/big.txt"), "x".repeat(4096));
    let outcome = call_with(
        &harness,
        "read",
        json!({ "path": format!("{WS}/big.txt") }),
        ToolBudget::new(30_000, 64),
        CancellationToken::new(),
    )
    .await;
    match outcome {
        Outcome::Truncated {
            bytes_emitted,
            limit,
            ..
        } => {
            assert_eq!(limit, 64, "the ceiling is the call's, not the file's");
            assert_eq!(bytes_emitted, 64);
        }
        other => panic!("a file past the ceiling comes back truncated: {other:?}"),
    }
    // And the ask itself was bounded: the rest never entered memory.
    match harness.recorded().as_slice() {
        [BrokerCall::Read { limit, .. }] => assert_eq!(*limit, 64),
        other => panic!("one bounded read: {other:?}"),
    }
}

#[tokio::test]
async fn read_denied_is_a_denial_not_an_error() {
    let harness = read_only().with_file("/elsewhere/secret", "shh");
    let outcome = call(&harness, "read", json!({ "path": "/elsewhere/secret" })).await;
    assert!(
        matches!(outcome, Outcome::Denied { .. }),
        "a path outside the grants settles denied: {outcome:?}"
    );
    assert!(
        matches!(
            harness.recorded().as_slice(),
            [BrokerCall::Read { allowed: false, .. }]
        ),
        "the ask is recorded as refused, not swallowed: {:?}",
        harness.recorded()
    );
}

#[tokio::test]
async fn read_without_a_path_says_which_argument_is_missing() {
    let harness = rig();
    let outcome = call(&harness, "read", json!({})).await;
    match outcome {
        Outcome::Failed { code, message } => {
            assert_eq!(code, "invalid-input");
            assert!(message.contains("path"), "it names the argument: {message}");
        }
        other => panic!("bad arguments are a value the model can fix: {other:?}"),
    }
    assert!(
        harness.recorded().is_empty(),
        "nothing reached the broker: {:?}",
        harness.recorded()
    );
}

// --- write and edit: all-or-nothing ---------------------------------------

#[tokio::test]
async fn write_goes_out_atomically() {
    let harness = rig();
    let outcome = call(
        &harness,
        "write",
        json!({ "path": format!("{WS}/new.txt"), "content": "body" }),
    )
    .await;
    assert!(text_of(&outcome).contains("4 bytes"));
    match harness.recorded().as_slice() {
        [
            BrokerCall::Write {
                bytes,
                atomic,
                allowed: true,
                ..
            },
        ] => {
            assert_eq!(*bytes, 4);
            assert!(*atomic, "a `write` is all-or-nothing");
        }
        other => panic!("one atomic write: {other:?}"),
    }
}

#[tokio::test]
async fn write_denied_is_a_denial_not_an_error() {
    let harness = read_only();
    let outcome = call(
        &harness,
        "write",
        json!({ "path": format!("{WS}/new.txt"), "content": "body" }),
    )
    .await;
    assert!(
        matches!(outcome, Outcome::Denied { .. }),
        "no write grant, so denied: {outcome:?}"
    );
}

#[tokio::test]
async fn edit_replaces_one_run_of_text() {
    let harness = rig().with_file(format!("{WS}/a.txt"), "one two one");
    let outcome = call(
        &harness,
        "edit",
        json!({ "path": format!("{WS}/a.txt"), "old_text": "two", "new_text": "three" }),
    )
    .await;
    assert!(text_of(&outcome).contains("1 occurrence"));
    match harness.recorded().as_slice() {
        [BrokerCall::Read { .. }, BrokerCall::Write { atomic, .. }] => {
            assert!(*atomic, "an `edit` is all-or-nothing");
        }
        other => panic!("read then write, nothing else: {other:?}"),
    }
}

/// The revert an atomic edit promises, in the shape a mock can show it: when
/// the edit cannot be made safely, **no write is attempted at all**, so there
/// is nothing to roll back and the file is exactly as it was.
#[tokio::test]
async fn an_ambiguous_edit_writes_nothing() {
    let harness = rig().with_file(format!("{WS}/a.txt"), "one two one");
    let outcome = call(
        &harness,
        "edit",
        json!({ "path": format!("{WS}/a.txt"), "old_text": "one", "new_text": "1" }),
    )
    .await;
    let text = text_of(&outcome);
    assert!(
        text.contains("appears 2 times"),
        "it says how many and what to do: {text}"
    );
    assert!(
        !harness
            .recorded()
            .iter()
            .any(|c| matches!(c, BrokerCall::Write { .. })),
        "a refused edit never reaches the write: {:?}",
        harness.recorded()
    );
}

/// The other half of the same promise: a file longer than the ceiling is not
/// rewritten from the slice that was read, which would delete the rest of it.
#[tokio::test]
async fn an_edit_of_a_file_past_the_ceiling_writes_nothing() {
    let harness = rig().with_file(
        format!("{WS}/big.txt"),
        format!("needle{}", "x".repeat(4096)),
    );
    let outcome = call_with(
        &harness,
        "edit",
        json!({ "path": format!("{WS}/big.txt"), "old_text": "needle", "new_text": "pin" }),
        ToolBudget::new(30_000, 64),
        CancellationToken::new(),
    )
    .await;
    let text = text_of(&outcome);
    assert!(
        text.contains("longer than this call's ceiling"),
        "it says why: {text}"
    );
    assert!(
        !harness
            .recorded()
            .iter()
            .any(|c| matches!(c, BrokerCall::Write { .. })),
        "nothing was written: {:?}",
        harness.recorded()
    );
}

// --- bash -----------------------------------------------------------------

/// The shell the bundle asks the broker to start on this platform.
fn shell() -> &'static str {
    if cfg!(windows) { "cmd" } else { "/bin/sh" }
}

#[tokio::test]
async fn bash_runs_the_command_through_the_broker() {
    let harness = rig().with_spawn_response(shell(), "hi\n");
    let outcome = call(&harness, "bash", json!({ "command": "echo hi" })).await;
    assert_eq!(text_of(&outcome), "hi\n");
    match harness.recorded().as_slice() {
        [
            BrokerCall::Spawn {
                program,
                args,
                allowed: true,
            },
        ] => {
            assert_eq!(program, shell(), "the tool never spawns; the broker does");
            assert!(
                args.iter().any(|a| a == "echo hi"),
                "the command is an argument, not a shell the tool built: {args:?}"
            );
        }
        other => panic!("one spawn: {other:?}"),
    }
}

#[tokio::test]
async fn bash_denied_is_a_denial_not_an_error() {
    let harness = read_only();
    let outcome = call(&harness, "bash", json!({ "command": "echo hi" })).await;
    assert!(
        matches!(outcome, Outcome::Denied { .. }),
        "no spawn grant, so denied: {outcome:?}"
    );
    assert!(
        matches!(
            harness.recorded().as_slice(),
            [BrokerCall::Spawn { allowed: false, .. }]
        ),
        "the ask is recorded as refused: {:?}",
        harness.recorded()
    );
}

// --- grep and glob --------------------------------------------------------

#[tokio::test]
async fn grep_reports_matches_it_may_read() {
    let harness = rig()
        .with_file(format!("{WS}/a.txt"), "needle here\nnothing\n")
        .with_file(format!("{WS}/b.txt"), "nothing at all\n");
    let outcome = call(&harness, "grep", json!({ "pattern": "needle", "path": WS })).await;
    let text = text_of(&outcome);
    assert!(text.contains("a.txt:1: needle here"), "{text}");
    assert!(!text.contains("b.txt"), "{text}");
}

/// A name is information too: discovery goes through the broker, so a file the
/// grants do not cover is not listed and therefore not searched.
#[tokio::test]
async fn grep_never_names_what_the_grants_hide() {
    let harness = load_for_test(MANIFEST, &["read:/ws/src/**"])
        .expect("the manifest parses")
        .with_file(format!("{WS}/src/a.txt"), "needle here\n")
        .with_file(format!("{WS}/secret.txt"), "needle here too\n");
    let outcome = call(&harness, "grep", json!({ "pattern": "needle", "path": WS })).await;
    let text = text_of(&outcome);
    assert!(text.contains("src/a.txt"), "the readable match: {text}");
    assert!(!text.contains("secret.txt"), "not even named: {text}");
}

#[tokio::test]
async fn grep_bounds_each_file_and_says_so() {
    let harness = rig().with_file(
        format!("{WS}/big.txt"),
        format!("needle\n{}", "x".repeat(4096)),
    );
    let outcome = call_with(
        &harness,
        "grep",
        json!({ "pattern": "needle", "path": WS }),
        ToolBudget::new(30_000, 64),
        CancellationToken::new(),
    )
    .await;
    let text = text_of(&outcome);
    assert!(text.contains("needle"), "{text}");
    assert!(
        text.contains("only the first 64 bytes"),
        "a bounded search says what it did not read: {text}"
    );
}

#[tokio::test]
async fn glob_lists_only_what_matches() {
    let harness = rig()
        .with_file(format!("{WS}/a.rs"), "")
        .with_file(format!("{WS}/b.txt"), "");
    let outcome = call(&harness, "glob", json!({ "pattern": "*.rs", "path": WS })).await;
    let text = text_of(&outcome);
    assert_eq!(text, "a.rs", "relative to the root, forward slashes: {text}");
}

#[tokio::test]
async fn a_pattern_that_is_not_a_glob_says_so() {
    let harness = rig();
    let outcome = call(&harness, "glob", json!({ "pattern": "[", "path": WS })).await;
    match outcome {
        Outcome::Failed { code, message } => {
            assert_eq!(code, "invalid-input");
            assert!(message.contains("is not a glob"), "{message}");
        }
        other => panic!("a malformed glob is a value the model can fix: {other:?}"),
    }
}

// --- the two rules that hold for every tool -------------------------------

/// The arguments that make one tool do its ordinary work against `path`.
fn args_for(tool: &str, path: &str) -> Value {
    match tool {
        "read" => json!({ "path": path }),
        "write" => json!({ "path": path, "content": "body" }),
        "edit" => json!({ "path": path, "old_text": "a", "new_text": "b" }),
        "bash" => json!({ "command": "echo hi" }),
        "grep" => json!({ "pattern": "a", "path": path }),
        "glob" => json!({ "pattern": "*", "path": path }),
        other => panic!(
            "`{other}` is a new tool with no entry here — add one, so the two \
             rules below cover it too"
        ),
    }
}

/// Cancellation reaches **every** tool, not the ones somebody remembered.
///
/// Driven off `tools()` rather than a list written here, so a seventh tool that
/// ignores `ctx.cancel` fails this test the day it is added. That is the point:
/// the ceiling and the denial are enforced by the broker, but noticing that the
/// turn was cancelled is the tool's own job — `PolicyBroker` checks the token
/// on `write` and on nothing else — and until this test nothing checked it.
#[tokio::test]
async fn a_cancelled_call_stops_in_every_tool() {
    for tool in BuiltinTools::new().tools() {
        let harness = rig()
            .with_file(format!("{WS}/a.txt"), "a")
            .with_spawn_response(shell(), "hi");
        let cancel = CancellationToken::new();
        cancel.cancel();
        let outcome = call_with(
            &harness,
            &tool.name,
            args_for(&tool.name, &format!("{WS}/a.txt")),
            ToolBudget::default(),
            cancel,
        )
        .await;
        assert!(
            matches!(outcome, Outcome::Cancelled { .. }),
            "`{}` must settle cancelled when the call was cancelled, got {outcome:?}",
            tool.name
        );
        assert!(
            harness.recorded().is_empty(),
            "`{}` did work after it was cancelled: {:?}",
            tool.name,
            harness.recorded()
        );
    }
}

/// `..` never buys a path the grants do not cover.
///
/// The tools normalise a path before handing it to the broker, so
/// `/ws/../outside` is asked about as `/outside` and refused. Without the
/// normalisation a grant of `read:/ws/**` matches the *string*
/// `/ws/../outside` — `**` spans separators — and the escape is free against
/// any broker that does not normalise for itself.
///
/// Driven off `tools()`, so a seventh tool that takes a path is covered the day
/// it is added.
#[tokio::test]
async fn dot_dot_never_escapes_the_grants_in_any_tool() {
    for tool in BuiltinTools::new().tools() {
        let takes_a_path = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.get("path"))
            .is_some();
        if !takes_a_path {
            continue;
        }
        // A tool that *names* one file must refuse outright. A tool that
        // walks — `grep`, `glob` — answers "nothing", because a listing omits
        // rather than refuses: a name is information too.
        let names_one_file = tool
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|r| r.iter().any(|v| v.as_str() == Some("path")));

        let harness = rig().with_file("/outside/secret", "shh");
        let outcome = call(
            &harness,
            &tool.name,
            args_for(&tool.name, &format!("{WS}/../outside/secret")),
        )
        .await;
        if names_one_file {
            assert!(
                !matches!(outcome, Outcome::Ok { .. }),
                "`{}` let `..` out of the workspace: {outcome:?}",
                tool.name
            );
        }
        let shown = format!("{outcome:?}");
        assert!(
            !shown.contains("shh") && !shown.contains("outside/secret"),
            "`{}` showed something from outside the workspace: {shown}",
            tool.name
        );
        for recorded in harness.recorded() {
            let (path, allowed) = match &recorded {
                BrokerCall::Read { path, allowed, .. }
                | BrokerCall::List { path, allowed, .. }
                | BrokerCall::Write { path, allowed, .. } => (path.display().to_string(), *allowed),
                other => panic!("`{}` asked for {other:?}", tool.name),
            };
            assert!(
                !path.contains(".."),
                "`{}` handed the broker an unnormalised `{path}`",
                tool.name
            );
            assert!(
                !allowed,
                "`{}` was allowed `{path}`, outside the grants",
                tool.name
            );
        }
    }
}

/// And the normalisation does not cost a legitimate path.
#[tokio::test]
async fn a_dot_dot_that_stays_inside_still_reads() {
    let harness = rig().with_file(format!("{WS}/a.txt"), "hello");
    let outcome = call(
        &harness,
        "read",
        json!({ "path": format!("{WS}/sub/../a.txt") }),
    )
    .await;
    assert_eq!(
        text_of(&outcome),
        "hello",
        "`sub/../a.txt` is `a.txt`, and it is inside the workspace"
    );
}
