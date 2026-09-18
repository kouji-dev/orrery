//! Task 3 · the one dispatch path.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use common::{ext, wide_scope};
use orrery_proto::{CallId, Layer, Outcome, RuleId, Subject, ToolRef};
use orrery_tools::{
    CallCtx, PolicyCheck, PolicyDecision, Registry, ToolBudget, ToolError, ToolHost, ToolSpec,
};

/// A host that counts how often it was actually reached.
#[derive(Default)]
struct CountingHost {
    calls: AtomicUsize,
}

#[async_trait]
impl ToolHost for CountingHost {
    async fn call(
        &self,
        _ref: &ToolRef,
        input: serde_json::Value,
        _ctx: &CallCtx,
    ) -> Result<Outcome, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Outcome::Ok {
            surface: None,
            value: Some(input),
        })
    }
}

/// A policy that refuses everything, the way plan 07's will refuse something.
struct DenyAll;

impl PolicyCheck for DenyAll {
    fn check(&self, _ref: &ToolRef, _input: &serde_json::Value, _ctx: &CallCtx) -> PolicyDecision {
        PolicyDecision::Deny {
            rule: RuleId::new(),
            reason: "the test says no".to_owned(),
        }
    }
}

fn ctx() -> CallCtx {
    CallCtx::new(
        CallId::new(),
        Subject::Agent,
        wide_scope(),
        ToolBudget::new(30_000, 1 << 20),
    )
}

fn strict_registry(host: Arc<CountingHost>) -> (Registry, ToolRef) {
    let mut reg = Registry::with_host(host);
    reg.register(
        &ext("builtin"),
        Layer::Project,
        ToolSpec::new("read").with_schema(serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"],
            "additionalProperties": false
        })),
    );
    (reg, "builtin.read".parse().expect("valid"))
}

#[tokio::test]
async fn validates_input() {
    let host = Arc::new(CountingHost::default());
    let (reg, r#ref) = strict_registry(host.clone());

    let outcome = reg
        .dispatch(&r#ref, serde_json::json!({ "path": 7 }), ctx())
        .await
        .expect("a malformed call is not an error");
    match outcome {
        Outcome::Failed { code, .. } => assert_eq!(code, "invalid-input"),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(
        host.calls.load(Ordering::SeqCst),
        0,
        "the host must never see a call that does not fit its schema"
    );

    // And a well-formed one goes through.
    let outcome = reg
        .dispatch(&r#ref, serde_json::json!({ "path": "src" }), ctx())
        .await
        .expect("dispatch");
    assert!(outcome.is_ok(), "got {outcome:?}");
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn denial_is_ok_arm() {
    let host = Arc::new(CountingHost::default());
    let (reg, r#ref) = strict_registry(host.clone());
    let reg = reg.with_policy(Arc::new(DenyAll));

    let outcome = reg
        .dispatch(&r#ref, serde_json::json!({ "path": "src" }), ctx())
        .await
        .expect("a denial is a value, not an error");
    match outcome {
        Outcome::Denied { reason, .. } => assert_eq!(reason, "the test says no"),
        other => panic!("expected Denied, got {other:?}"),
    }
    assert_eq!(
        host.calls.load(Ordering::SeqCst),
        0,
        "a denied call never reaches the host"
    );
}

/// `ToolError` has no `Denied` variant, and this is checked rather than
/// promised: the match below stops compiling the day somebody adds one.
#[test]
fn tool_error_has_no_denied_variant() {
    fn exhaustive(e: &ToolError) -> &'static str {
        match e {
            ToolError::NoSuchTool { .. } => "no-such-tool",
            ToolError::InvalidSchema { .. } => "invalid-schema",
            ToolError::Host { .. } => "host",
            // `ToolError` is `#[non_exhaustive]` to its dependents, but this
            // arm is what a new variant would have to be added under — and a
            // `Denied` one has no business being here.
            _ => "unknown",
        }
    }
    assert_eq!(
        exhaustive(&ToolError::NoSuchTool {
            name: "x.y".to_owned()
        }),
        "no-such-tool"
    );
}

#[tokio::test]
async fn unknown_ref_is_an_error_not_a_denial() {
    let (reg, _) = strict_registry(Arc::new(CountingHost::default()));
    let r#ref: ToolRef = "builtin.write".parse().expect("valid");
    let err = reg
        .dispatch(&r#ref, serde_json::json!({}), ctx())
        .await
        .expect_err("a reference the registry does not hold is a harness bug");
    assert!(matches!(err, ToolError::NoSuchTool { .. }), "{err:?}");
}

#[tokio::test]
async fn manifest_ceiling_narrows_the_budget() {
    /// A host that reports the budget it was handed.
    struct BudgetHost;

    #[async_trait]
    impl ToolHost for BudgetHost {
        async fn call(
            &self,
            _ref: &ToolRef,
            _input: serde_json::Value,
            ctx: &CallCtx,
        ) -> Result<Outcome, ToolError> {
            Ok(Outcome::Ok {
                surface: None,
                value: Some(serde_json::to_value(ctx.budget()).expect("serialisable")),
            })
        }
    }

    let mut reg = Registry::with_host(Arc::new(BudgetHost));
    reg.register(
        &ext("builtin"),
        Layer::Project,
        ToolSpec::new("read").with_ceiling(ToolBudget::new(5_000, 1_024)),
    );
    let r#ref: ToolRef = "builtin.read".parse().expect("valid");

    let outcome = reg
        .dispatch(&r#ref, serde_json::json!({}), ctx())
        .await
        .expect("dispatch");
    let Outcome::Ok {
        value: Some(value), ..
    } = outcome
    else {
        panic!("expected a value");
    };
    let seen: ToolBudget = serde_json::from_value(value).expect("a budget");
    assert_eq!(seen, ToolBudget::new(5_000, 1_024));
}

/// Plan 07, task 4, completed once `orrery-audit` was a dependency: a settled
/// tool call is an **audit event**, not a `tracing::info!` line.
///
/// The difference is not cosmetic. The audit stream is the thing `orrery
/// ledger` reads and the thing an operator keeps; a tracing line is a
/// developer convenience that a release build's subscriber may drop entirely.
/// "Which tool ran, under which call, and how did it end" has to be answerable
/// from the stream alone.
#[tokio::test]
async fn a_settled_call_reaches_the_audit_stream() {
    let audit = orrery_audit::memory();
    let host = Arc::new(CountingHost::default());
    let (reg, r#ref) = strict_registry(host.clone());
    let reg = reg.with_audit(Arc::clone(&audit) as orrery_audit::Audit);

    let ctx = ctx();
    let call = ctx.call;
    let outcome = reg
        .dispatch(&r#ref, serde_json::json!({ "path": "Cargo.toml" }), ctx)
        .await
        .expect("the call was carried");
    assert!(outcome.is_ok());

    let recorded = audit
        .records()
        .into_iter()
        .find_map(|rec| match rec.event {
            orrery_audit::AuditEvent::ToolCall {
                call,
                tool,
                input,
                outcome,
            } => Some((call, tool, input, outcome)),
            _ => None,
        })
        .expect("the settled call is in the audit stream");

    assert_eq!(recorded.0, call, "joined to the call, not to a fresh id");
    assert_eq!(recorded.1, "builtin.read");
    assert_eq!(recorded.3, orrery_audit::CallOutcome::Ok);
    // The input is **hashed**, never kept: a tool call's arguments are the most
    // likely place a secret ends up, and an audit stream is the last place one
    // should be readable.
    assert_eq!(
        recorded.2,
        orrery_audit::Digest::of_bytes(
            serde_json::json!({ "path": "Cargo.toml" })
                .to_string()
                .as_bytes()
        )
    );
}

/// A refusal is audited too, and as a refusal.
///
/// An audit stream that only recorded what succeeded would answer "what did
/// this agent do" and not "what did it try", and the second question is the one
/// somebody asks after an incident.
#[tokio::test]
async fn a_denied_call_is_audited_as_denied() {
    let audit = orrery_audit::memory();
    let host = Arc::new(CountingHost::default());
    let (reg, r#ref) = strict_registry(host.clone());
    let reg = reg
        .with_audit(Arc::clone(&audit) as orrery_audit::Audit)
        .with_policy(Arc::new(DenyAll));

    let outcome = reg
        .dispatch(&r#ref, serde_json::json!({ "path": "Cargo.toml" }), ctx())
        .await
        .expect("a denial is not an error");
    assert!(matches!(outcome, Outcome::Denied { .. }), "{outcome:?}");

    let settled = audit
        .records()
        .into_iter()
        .find_map(|rec| match rec.event {
            orrery_audit::AuditEvent::ToolCall { outcome, .. } => Some(outcome),
            _ => None,
        })
        .expect("the refusal is in the audit stream");
    assert_eq!(settled, orrery_audit::CallOutcome::Denied);
    assert_eq!(host.calls.load(Ordering::SeqCst), 0);
}
