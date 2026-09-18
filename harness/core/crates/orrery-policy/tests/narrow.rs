//! Task 6 · `PermissionHandler`, narrowing only.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::{wide_scope, Workspace};
use orrery_policy::{
    review_narrowing, Decision, HandlerError, InertMinter, PendingCall, PermissionHandler,
    PolicyBuilder, PolicyEngine, Verdict,
};
use orrery_proto::{Layer, Subject};
use proptest::prelude::*;

/// Three tools, one per list, so a test can ask the engine for a decision of
/// any verdict without ever building one by hand.
const RULES: &str = r#"
[permissions]
deny  = ["tool(d.*)"]
ask   = ["tool(k.*)"]
allow = ["tool(a.*)"]
"#;

fn three_list_engine(ws: &Workspace) -> PolicyEngine {
    PolicyEngine::new(
        PolicyBuilder::new(ws.root())
            .layer_toml(RULES, "p.toml", Layer::Project, false)
            .unwrap()
            .build()
            .unwrap(),
    )
}

fn call_of(verdict: Verdict) -> PendingCall {
    PendingCall::tool(match verdict {
        Verdict::Deny => "d.one",
        Verdict::Ask => "k.one",
        Verdict::Allow => "a.one",
        _ => unreachable!("the three verdicts are the three lists"),
    })
}

/// A handler that always answers with a decision of a fixed verdict, built the
/// only way a decision can be built: by asking the engine.
struct Fixed {
    answer: Verdict,
    inner: PolicyEngine,
}

#[async_trait]
impl PermissionHandler for Fixed {
    async fn review(
        &self,
        _call: &PendingCall,
        subject: &Subject,
        _proposed: Decision,
    ) -> Result<Decision, HandlerError> {
        Ok(self
            .inner
            .check(&call_of(self.answer), subject, &wide_scope()))
    }
}

struct Panicking;

#[async_trait]
impl PermissionHandler for Panicking {
    async fn review(
        &self,
        _call: &PendingCall,
        _subject: &Subject,
        _proposed: Decision,
    ) -> Result<Decision, HandlerError> {
        panic!("a handler that panics fails closed");
    }
}

struct Failing;

#[async_trait]
impl PermissionHandler for Failing {
    async fn review(
        &self,
        _call: &PendingCall,
        _subject: &Subject,
        _proposed: Decision,
    ) -> Result<Decision, HandlerError> {
        Err(HandlerError::Failed("the reviewer is offline".to_owned()))
    }
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(f)
}

proptest! {
    /// For arbitrary proposed and returned decisions, the result is never more
    /// permissive than proposed.
    #[test]
    fn widening_is_dropped(
        proposed in prop::sample::select(vec![Verdict::Deny, Verdict::Ask, Verdict::Allow]),
        returned in prop::sample::select(vec![Verdict::Deny, Verdict::Ask, Verdict::Allow]),
    ) {
        let ws = Workspace::new();
        let audit = orrery_audit::memory();
        let engine = three_list_engine(&ws);
        let call = call_of(proposed);
        let start = engine.check(&call, &Subject::Agent, &wide_scope());
        prop_assert_eq!(start.verdict(), proposed);

        let handler = Fixed { answer: returned, inner: three_list_engine(&ws) };
        let out = run(review_narrowing(
            &handler,
            &call,
            &Subject::Agent,
            start,
            &InertMinter::new(),
            &(Arc::clone(&audit) as orrery_audit::Audit),
        ));

        prop_assert!(out.verdict() <= proposed, "narrowing widened");
        prop_assert_eq!(out.verdict(), proposed.min(returned));

        // And an attempt to widen is logged rather than silently swallowed.
        if returned > proposed {
            prop_assert!(
                audit.to_jsonl().contains("widen"),
                "a widening attempt must be recorded"
            );
        }
    }
}

#[test]
fn panic_fails_closed() {
    let ws = Workspace::new();
    let engine = three_list_engine(&ws);
    let call = call_of(Verdict::Allow);
    let proposed = engine.check(&call, &Subject::Agent, &wide_scope());
    assert_eq!(proposed.verdict(), Verdict::Allow);

    let out = run(review_narrowing(
        &Panicking,
        &call,
        &Subject::Agent,
        proposed,
        &InertMinter::new(),
        &orrery_audit::null(),
    ));
    assert_eq!(out.verdict(), Verdict::Deny);

    // The session survives: the engine still answers afterwards.
    assert_eq!(
        engine
            .check(&call, &Subject::Agent, &wide_scope())
            .verdict(),
        Verdict::Allow
    );
}

#[test]
fn a_handler_that_errors_also_denies() {
    let ws = Workspace::new();
    let engine = three_list_engine(&ws);
    let call = call_of(Verdict::Allow);
    let proposed = engine.check(&call, &Subject::Agent, &wide_scope());

    let out = run(review_narrowing(
        &Failing,
        &call,
        &Subject::Agent,
        proposed,
        &InertMinter::new(),
        &orrery_audit::null(),
    ));
    assert_eq!(out.verdict(), Verdict::Deny);
}

/// A handler cannot affect a managed-layer decision: a managed deny is already
/// the floor, and nothing a handler returns can move it up.
#[test]
fn never_reaches_managed() {
    let ws = Workspace::new();
    let managed = PolicyEngine::new(
        PolicyBuilder::new(ws.root())
            .layer_toml(
                "[permissions]\ndeny = [\"tool(a.*)\"]\n",
                "managed.toml",
                Layer::Managed,
                false,
            )
            .unwrap()
            .layer_toml(
                "[permissions]\nallow = [\"tool(a.*)\"]\n",
                "project.toml",
                Layer::Project,
                false,
            )
            .unwrap()
            .build()
            .unwrap(),
    );
    let call = PendingCall::tool("a.one");
    let proposed = managed.check(&call, &Subject::Agent, &wide_scope());
    assert_eq!(proposed.verdict(), Verdict::Deny);

    let handler = Fixed {
        answer: Verdict::Allow,
        inner: three_list_engine(&ws),
    };
    let out = run(review_narrowing(
        &handler,
        &call,
        &Subject::Agent,
        proposed,
        &InertMinter::new(),
        &orrery_audit::null(),
    ));
    assert_eq!(
        out.verdict(),
        Verdict::Deny,
        "a handler cannot lift a managed deny"
    );
}

/// The token in the decision a handler is shown redeems nowhere: it is a shape,
/// not the capability it is being asked to judge.
#[test]
fn the_shown_token_is_inert() {
    let ws = Workspace::new();
    let engine = three_list_engine(&ws);
    let call = call_of(Verdict::Allow);
    let real = engine.check(&call, &Subject::Agent, &wide_scope());
    let inert = InertMinter::new();
    let shown = inert.shadow(&real, &call);

    let Decision::Allow { token, .. } = shown else {
        panic!("the shape should still be an Allow");
    };
    assert_eq!(
        engine.ledger().redeem(token.nonce()),
        Err(orrery_policy::TokenError::Unknown),
        "the copy handed to a handler must not redeem against the real ledger"
    );
}
