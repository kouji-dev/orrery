//! Plan 05, Task 8 — **the phase-1 acceptance criterion**.
//!
//! A `Harness` built from a config: the fixture provider, the sqlite store and
//! the builtin tool bundle. One turn, one tool call, and the transcript reads
//!
//! ```text
//! user → assistant(tool_use) → tool_result → assistant(text)
//! ```
//!
//! with the tool having actually touched the filesystem — through the broker,
//! under a capability token the policy engine minted.
//!
//! Nothing here reaches the network and nothing needs a key: the model is two
//! committed `.jsonl` streams and sqlite is bundled.

use std::path::{Path, PathBuf};

use orrery_audit::AuditEvent;
use orrery_harness::{Harness, ResolvedConfig};
use orrery_kernel::{KernelConfig, TurnOutcome};
use orrery_proto::{ContentBlock, Outcome, TokenBudget};
use orrery_session::CharsOverFour;
use tokio_util::sync::CancellationToken;

/// The committed streams, from this crate's directory.
fn stream(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../clients/conformance/streams")
        .join(name)
}

/// What the model asks to read, in `tool-call.jsonl`.
const TARGET: &str = "Cargo.toml";

/// What that file says in the workspace this test builds.
const CONTENTS: &str = "[package]\nname = \"the-file-the-tool-really-read\"\n";

/// Build a harness over a temporary workspace with a `Cargo.toml` in it.
fn harness(audit: orrery_audit::Audit) -> (tempfile::TempDir, Harness) {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    std::fs::write(dir.path().join(TARGET), CONTENTS).expect("the target file is written");

    let mut config = ResolvedConfig::fixture(
        dir.path(),
        // Pass one asks for `builtin.read`; pass two answers in prose.
        vec![stream("tool-call.jsonl"), stream("text-turn.jsonl")],
    );
    config.audit = audit;
    config.kernel = KernelConfig {
        model: "fixture".to_owned(),
        ..KernelConfig::default()
    };
    let harness = Harness::build(config).expect("the harness builds");
    (dir, harness)
}

/// The whole of phase 1, in one turn.
#[test]
fn one_turn_with_a_tool_call() {
    let audit = orrery_audit::memory();
    let (dir, harness) = harness(audit.clone());

    let outcome = harness
        .block_on(harness.submit("what is in Cargo.toml?", CancellationToken::new()))
        .expect("the harness carried the turn");
    assert!(
        matches!(outcome, TurnOutcome::Completed { .. }),
        "the turn completes: {outcome:?}"
    );

    // 1 · The transcript.
    let rows = harness.block_on(async {
        harness
            .store()
            .materialise(
                harness.branch(),
                TokenBudget {
                    max: u64::MAX,
                    reserve: 0,
                },
                &CharsOverFour,
            )
            .await
            .expect("the branch materialises")
    });
    let shape: Vec<String> = rows
        .messages
        .iter()
        .map(|m| {
            let kinds: Vec<&str> = m
                .content
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { .. } => "text",
                    ContentBlock::ToolUse { .. } => "tool_use",
                    ContentBlock::ToolResult { .. } => "tool_result",
                    ContentBlock::Thinking { .. } => "thinking",
                    _ => "?",
                })
                .collect();
            format!("{:?}({})", m.role, kinds.join("+"))
        })
        .collect();
    assert_eq!(
        shape,
        vec![
            "User(text)",
            "Assistant(text+tool_use)",
            "User(tool_result)",
            "Assistant(text)",
        ],
        "user → assistant(tool_use) → tool_result → assistant(text)"
    );

    // 2 · The tool really read the file.
    let settled = rows
        .messages
        .iter()
        .flat_map(|m| m.content.iter())
        .find_map(|b| match b {
            ContentBlock::ToolResult { outcome, .. } => Some(outcome.clone()),
            _ => None,
        })
        .expect("the tool call settled");
    let read_back = match &settled {
        Outcome::Ok { value, .. } => value.clone().expect("the tool returned what it read"),
        other => panic!("the default rules allow reading the workspace: {other:?}"),
    };
    assert_eq!(
        read_back.get("text").and_then(|t| t.as_str()),
        Some(CONTENTS),
        "the bytes in the transcript are the bytes on disk"
    );

    // 3 · And it went through the broker, under a token the engine minted.
    let decisions: Vec<String> = audit
        .records()
        .into_iter()
        .filter_map(|r| match r.event {
            AuditEvent::CapabilityDecision {
                request, verdict, ..
            } => Some(format!("{verdict:?} {request}")),
            _ => None,
        })
        .collect();
    assert!(
        decisions
            .iter()
            .any(|d| d.starts_with("Allow tool(builtin.read")),
        "the call was checked as a tool: {decisions:?}"
    );
    assert!(
        decisions
            .iter()
            .any(|d| d.starts_with("Allow read(") && d.to_lowercase().contains("cargo.toml")),
        "and the *effect* was checked separately, against the resolved path: {decisions:?}"
    );
    assert!(
        audit.records().iter().any(|r| matches!(
            &r.event,
            AuditEvent::ToolCall { tool, .. } if tool == "builtin.read"
        )),
        "the dispatch itself is in the audit"
    );

    drop(dir);
}

/// The default rules allow the workspace and nothing else: a read outside it is
/// refused by the **broker**, with the rule that refused it.
#[test]
fn outside_the_workspace_is_refused() {
    let audit = orrery_audit::memory();
    let (_dir, harness) = harness(audit.clone());
    let outside = if cfg!(windows) {
        "C:/Windows/System32/drivers/etc/hosts"
    } else {
        "/etc/hosts"
    };

    let registry = harness.kernel().registry().clone();
    let scope = harness.scope().clone();
    let outcome = harness.block_on(async move {
        registry
            .dispatch(
                &"builtin.read".parse().expect("a tool ref"),
                serde_json::json!({ "path": outside }),
                orrery_tools::CallCtx::new(
                    orrery_proto::CallId::new(),
                    orrery_proto::Subject::Agent,
                    scope,
                    orrery_harness::default_tool_budget(),
                ),
            )
            .await
            .expect("the harness carried the call")
    });

    match outcome {
        Outcome::Denied { reason, .. } => assert!(
            reason.contains("no rule allows") || reason.contains("denies"),
            "the refusal says why: {reason}"
        ),
        other => panic!("a read outside the workspace must be refused: {other:?}"),
    }
}

/// The ledger reports what loaded, which is what `query extensions` answers.
///
/// Amended in round 5: the default set is three bundles, not one. `builtin` is
/// the tools; `views-default` is the section 6.7 floor, without which a client
/// has nothing bound and can legitimately draw a blank screen; `agents-default`
/// is the section 4.6 roles. All three load through the same door, which is why
/// they are all in one ledger and why this test looks the entry up by name
/// rather than by position.
#[test]
fn the_first_party_bundles_are_in_the_ledger() {
    let (_dir, harness) = harness(orrery_audit::null());
    let ledger = harness.ledger().all();
    let by_name = |want: &str| {
        ledger
            .iter()
            .find(|entry| entry.ext().as_str() == want)
            .unwrap_or_else(|| panic!("`{want}` did not load: {ledger:?}"))
    };
    let names: Vec<String> = by_name("builtin")
        .contributions()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    for tool in ["read", "write", "edit", "bash", "grep", "glob"] {
        assert!(
            names.contains(&tool.to_owned()),
            "`{tool}` is missing: {names:?}"
        );
    }
    // The floor contributes views and agents rather than tools, which is what
    // makes "a view is not a tool" true of the shipped set and not only of the
    // vocabulary.
    for (ext, contribution) in [
        ("views-default", "assistant.text"),
        ("agents-default", "planner"),
    ] {
        let names: Vec<String> = by_name(ext)
            .contributions()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert!(names.contains(&contribution.to_owned()), "{ext}: {names:?}");
    }
    assert!(
        harness
            .kernel()
            .registry()
            .entry(&"builtin.read".parse().unwrap())
            .is_some(),
        "and it is registered under the one namespace"
    );
}

/// A turn on a branch somebody else is holding is refused, not queued.
#[test]
fn a_busy_branch_is_refused() {
    let (_dir, harness) = harness(orrery_audit::null());
    harness.block_on(async {
        let _held = harness
            .store()
            .lease(harness.branch())
            .await
            .expect("the first lease");
        let second = harness.store().lease(harness.branch()).await;
        assert!(
            matches!(second, Err(orrery_session::SessionError::BranchBusy { .. })),
            "one turn at a time per branch: {second:?}"
        );
    });
}

/// `block_on` and `handle` are the two halves of the bridge, and they agree.
#[test]
fn the_runtime_bridge_works_both_ways() {
    let (_dir, harness) = harness(orrery_audit::null());
    let from_block_on = harness.block_on(async { 1 + 1 });
    let handle = harness.handle();
    let from_handle = handle.block_on(async { 1 + 1 });
    assert_eq!(from_block_on, from_handle);
}

/// Every first-party extension this build has, named.
#[test]
fn the_feature_set_is_what_it_says() {
    let compiled = orrery_harness::features::compiled_in();
    assert!(compiled.contains(&"builtin"));
    assert!(compiled.contains(&"session-sqlite"));
    assert!(compiled.contains(&"provider-fixture"));
}
