//! Plan 05, Task 1: the chain, and the four rules it enforces.
//!
//! The typed half of `rewrite_is_typed` is a `compile_fail` doctest on
//! [`orrery_kernel::phase`] — the repo's idiom for "this must not compile",
//! used by `BranchLease`, `CapabilityToken` and `CallCtx` — and this file is its
//! runtime half.

mod common;

use std::sync::Arc;

use common::{Passes, Rig, TestHost, fixture, registry};
use orrery_kernel::{
    ChainOutcome, ContextBuild, InterceptCtx, Interceptor, InterceptorSet, Kernel, KernelConfig,
    MatchCtx, PassId, PendingCall, RegisterError, ToolBefore, ToolInput, TurnInput, TurnOutcome,
};
use orrery_proto::{
    AgentScope, BranchId, Grant, Outcome, RuleId, SessionId, ToolRef, TurnId, Usage, UserInput,
    Verdict,
};
use orrery_tools::{CallCtx, PolicyCheck, PolicyDecision};
use tokio_util::sync::CancellationToken;

/// An interceptor at `tool.before` that rewrites the input it is given.
struct Rewriter;

impl Interceptor<ToolBefore> for Rewriter {
    fn can_deny(&self) -> bool {
        false
    }

    fn run(&self, _ctx: &InterceptCtx<'_>, payload: &ToolInput) -> Verdict<ToolInput> {
        let mut next = payload.clone();
        next.input = serde_json::json!({ "path": "rewritten.txt" });
        Verdict::Rewrite(next)
    }
}

/// One that refuses.
struct Refuser(&'static str);

impl Interceptor<ToolBefore> for Refuser {
    fn run(&self, _ctx: &InterceptCtx<'_>, _payload: &ToolInput) -> Verdict<ToolInput> {
        Verdict::Deny {
            reason: self.0.to_owned(),
        }
    }
}

/// One that answers from what it already holds — the cache case.
struct Cached;

impl Interceptor<ToolBefore> for Cached {
    fn run(&self, _ctx: &InterceptCtx<'_>, _payload: &ToolInput) -> Verdict<ToolInput> {
        Verdict::Handled {
            result: Outcome::Ok {
                surface: None,
                value: Some(serde_json::json!({ "from": "the interceptor's own data" })),
            },
        }
    }
}

/// One that writes down that it ran, and carries on.
struct Marker(&'static str, Arc<parking_lot::Mutex<Vec<&'static str>>>);

impl Interceptor<orrery_kernel::ToolResolve> for Marker {
    fn run(&self, _ctx: &InterceptCtx<'_>, _payload: &PendingCall) -> Verdict<PendingCall> {
        self.1.lock().push(self.0);
        Verdict::Continue
    }
}

/// A policy that refuses everything, so a test can ask what happens when an
/// interceptor is the *only* thing saying yes.
struct DenyAll;

impl PolicyCheck for DenyAll {
    fn check(&self, _ref: &ToolRef, _input: &serde_json::Value, _ctx: &CallCtx) -> PolicyDecision {
        PolicyDecision::Deny {
            rule: nil_rule(),
            reason: "the policy in this test refuses everything".to_owned(),
        }
    }
}

/// The rule id a refusal carries when no numbered rule is responsible.
fn nil_rule() -> RuleId {
    "00000000-0000-0000-0000-000000000000"
        .parse()
        .expect("the nil uuid is a uuid")
}

/// A kernel whose model asks for one tool and then answers.
async fn kernel_with(
    rig: &Rig,
    interceptors: InterceptorSet,
    policy: Option<Arc<dyn PolicyCheck>>,
    host: Arc<TestHost>,
) -> Kernel {
    let provider = Passes::of(vec![fixture("tool-call.jsonl"), fixture("text-turn.jsonl")]);
    let mut registry = registry(host);
    if let Some(policy) = policy {
        registry = registry.with_policy(policy);
    }
    Kernel::new(
        rig.store.clone(),
        provider,
        Arc::new(registry),
        KernelConfig::default(),
    )
    .with_interceptors(Arc::new(interceptors))
}

async fn run(rig: &Rig, kernel: &Kernel) -> TurnOutcome {
    let lease = rig.lease().await;
    kernel
        .run_turn(
            lease,
            TurnInput::new(rig.session, UserInput::text("go"), rig.scope()),
            CancellationToken::new(),
        )
        .await
        .expect("the harness carried the turn")
}

/// The runtime half of translation #2: a rewrite at `tool.before` is the input
/// the tool actually receives. The type half is the `compile_fail` doctest.
#[tokio::test]
async fn rewrite_is_typed() {
    let rig = Rig::open().await;
    let host = TestHost::echoing();
    let mut set = InterceptorSet::new();
    set.register::<ToolBefore>(Rewriter).expect("registers");

    let kernel = kernel_with(&rig, set, None, host.clone()).await;
    let outcome = run(&rig, &kernel).await;
    assert!(matches!(outcome, TurnOutcome::Completed { .. }));

    let calls = host.recorder.calls();
    assert_eq!(calls.len(), 1, "one call was dispatched");
    assert_eq!(
        calls[0].1,
        serde_json::json!({ "path": "rewritten.txt" }),
        "the tool saw the rewritten input, not the model's"
    );
}

/// A `Deny` narrows in one direction only.
#[tokio::test]
async fn deny_narrows_only() {
    // An interceptor refusing what policy would have allowed: denied, and the
    // tool never ran.
    let rig = Rig::open().await;
    let host = TestHost::echoing();
    let mut set = InterceptorSet::new();
    set.register::<ToolBefore>(Refuser("not on my watch"))
        .expect("registers");
    let kernel = kernel_with(&rig, set, None, host.clone()).await;
    run(&rig, &kernel).await;

    let rows = rig.rows().await;
    let outcome = common::first_outcome(&rows).expect("a tool result was appended");
    match outcome {
        Outcome::Denied { reason, .. } => assert_eq!(reason, "not on my watch"),
        other => panic!("an interceptor's refusal must settle as a denial: {other:?}"),
    }
    assert!(
        host.recorder.is_empty(),
        "a refused call must never reach the tool"
    );

    // An interceptor saying nothing about what policy refuses: still refused.
    let rig = Rig::open().await;
    let host = TestHost::echoing();
    let kernel = kernel_with(
        &rig,
        InterceptorSet::new(),
        Some(Arc::new(DenyAll)),
        host.clone(),
    )
    .await;
    run(&rig, &kernel).await;

    let rows = rig.rows().await;
    match common::first_outcome(&rows).expect("a tool result was appended") {
        Outcome::Denied { reason, .. } => assert!(
            reason.contains("refuses everything"),
            "the policy's refusal is the one reported: {reason}"
        ),
        other => panic!("policy's refusal must stand: {other:?}"),
    }
    assert!(
        host.recorder.is_empty(),
        "there is no path around the policy check"
    );
}

/// Registration order, observed, twice.
#[tokio::test]
async fn order_is_deterministic() {
    let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
    for round in 0..2 {
        seen.lock().clear();
        let rig = Rig::open().await;
        let mut set = InterceptorSet::new();
        set.register::<orrery_kernel::ToolResolve>(Marker("first", seen.clone()))
            .expect("registers");
        set.register::<orrery_kernel::ToolResolve>(Marker("second", seen.clone()))
            .expect("registers");
        set.register::<orrery_kernel::ToolResolve>(Marker("third", seen.clone()))
            .expect("registers");

        let kernel = kernel_with(&rig, set, None, TestHost::echoing()).await;
        run(&rig, &kernel).await;

        assert_eq!(
            *seen.lock(),
            vec!["first", "second", "third"],
            "round {round}: interceptors run in registration order"
        );
    }
}

/// A `Handled` verdict means dispatch never runs.
#[tokio::test]
async fn handled_short_circuits() {
    let rig = Rig::open().await;
    let host = TestHost::echoing();
    let mut set = InterceptorSet::new();
    set.register::<ToolBefore>(Cached).expect("registers");

    let kernel = kernel_with(&rig, set, None, host.clone()).await;
    run(&rig, &kernel).await;

    assert!(
        host.recorder.is_empty(),
        "a handled call must not reach the tool"
    );
    let rows = rig.rows().await;
    match common::first_outcome(&rows).expect("a tool result was appended") {
        Outcome::Ok { value, .. } => assert_eq!(
            value
                .as_ref()
                .and_then(|v| v.get("from"))
                .and_then(|v| v.as_str()),
            Some("the interceptor's own data")
        ),
        other => panic!("the interceptor's own answer is what settles: {other:?}"),
    }
}

/// `context.build` is rewrite-only, and says so at registration rather than
/// mid-turn.
#[test]
fn deny_at_context_build_is_refused() {
    struct Denier;
    impl Interceptor<ContextBuild> for Denier {
        fn run(
            &self,
            _ctx: &InterceptCtx<'_>,
            _payload: &orrery_kernel::ContextDraft,
        ) -> Verdict<orrery_kernel::ContextDraft> {
            Verdict::Continue
        }
    }
    struct Rewriting;
    impl Interceptor<ContextBuild> for Rewriting {
        fn can_deny(&self) -> bool {
            false
        }
        fn run(
            &self,
            _ctx: &InterceptCtx<'_>,
            _payload: &orrery_kernel::ContextDraft,
        ) -> Verdict<orrery_kernel::ContextDraft> {
            Verdict::Continue
        }
    }

    let mut set = InterceptorSet::new();
    let refused = set.register::<ContextBuild>(Denier);
    assert_eq!(
        refused,
        Err(RegisterError::DenyNotAllowed {
            phase: "context.build"
        })
    );
    assert!(
        refused.unwrap_err().to_string().contains("rewrite-only"),
        "the error has to say what to do about it"
    );
    set.register::<ContextBuild>(Rewriting)
        .expect("a rewrite-only interceptor registers at context.build");
    assert_eq!(set.len_at::<ContextBuild>(), 1);
}

/// The chain is runnable on its own, without a turn around it: `Continue` from
/// everybody leaves the payload alone.
#[test]
fn an_empty_chain_changes_nothing() {
    let set = InterceptorSet::new();
    let scope = AgentScope {
        agent: "main".to_owned(),
        branch: BranchId::new(),
        tools: vec!["*".to_owned()],
        grant: Grant::nothing(),
    };
    let usage = Usage::default();
    let turn = TurnId::new();
    let ctx = InterceptCtx {
        session: SessionId::new(),
        turn,
        pass: PassId { turn, index: 1 },
        agent: &scope,
        budget_spent: &usage,
    };
    let payload = ToolInput {
        call: orrery_proto::CallId::new(),
        r#ref: "builtin.read".parse().expect("a tool ref"),
        input: serde_json::json!({ "path": "a.txt" }),
    };
    let (out, chain) = set.run::<ToolBefore>(&ctx, &MatchCtx::for_agent("main"), payload.clone());
    assert_eq!(out, payload);
    assert_eq!(chain, ChainOutcome::Continue);
}
