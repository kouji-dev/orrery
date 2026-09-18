//! Task 11 · degrade, end to end. **This is the phase-3 criterion.**

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::Fixture;
use orrery_audit::AuditEvent;
use orrery_broker::{EngineGate, ToolNeeds, install};
use orrery_policy::PendingCall;
use orrery_proto::{Aspect, CallId, ExtId, Layer, LoadOutcome, Outcome, Subject, ToolRef};
use orrery_tools::{CallCtx, Registry, ToolBudget, ToolError, ToolHost};

/// An extension host that answers for real, so "its other tools work" is a call
/// that happened rather than a registry lookup that succeeded.
#[derive(Debug, Default)]
struct EchoHost;

#[async_trait]
impl ToolHost for EchoHost {
    async fn call(
        &self,
        r#ref: &ToolRef,
        input: serde_json::Value,
        _ctx: &CallCtx,
    ) -> Result<Outcome, ToolError> {
        Ok(Outcome::Ok {
            surface: None,
            value: Some(serde_json::json!({ "tool": r#ref.to_string(), "echo": input })),
        })
    }
}

/// `buildgraph` wants `spawn(bazel *)` for one tool and nothing for the others.
/// The operator denies `spawn`. The install must survive it.
const RULES: &str = r#"
[permissions]
allow = ["tool(*)", "read(./**)", "spawn(*)"]

[permissions."ext:buildgraph"]
allow = ["tool(*)", "read(./**)"]
deny  = ["spawn(*)"]
"#;

fn budget() -> ToolBudget {
    ToolBudget::new(30_000, 1 << 20)
}

#[tokio::test]
async fn denied_spawn_degrades() {
    let fx = Fixture::with_rules(RULES);
    let engine = Arc::new(fx.engine);
    let audit = Arc::clone(&fx.audit) as orrery_audit::Audit;
    let ext = ExtId::new("buildgraph").expect("a valid id");

    let mut registry = Registry::with_host(Arc::new(EchoHost))
        .with_policy(Arc::new(EngineGate::new(Arc::clone(&engine))))
        .with_audit(Arc::clone(&audit));

    let outcome = install(
        &mut registry,
        &engine,
        &audit,
        &ext,
        Layer::Project,
        &[
            ToolNeeds::needing("build", Aspect::Spawn, "bazel build //..."),
            ToolNeeds::needing("query", Aspect::Read, "./BUILD"),
            ToolNeeds::plain("explain"),
        ],
        &Fixture::scope(),
    );

    // 1 · the install succeeded.
    let LoadOutcome::Degraded {
        ext: named,
        contributions,
        problems,
        ..
    } = &outcome
    else {
        panic!("a denied capability must degrade, not fail: {outcome:?}");
    };
    assert_eq!(named, &ext);

    // 2 · the spawn-needing tool is disabled, and the ledger says why.
    assert!(
        !contributions.iter().any(|c| c.name == "build"),
        "the tool that needed `spawn` must not be offered"
    );
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("build"), "{}", problems[0]);
    assert!(problems[0].contains("spawn"), "{}", problems[0]);
    assert!(
        registry
            .entry(&"buildgraph.build".parse().unwrap())
            .is_none(),
        "a disabled tool must not be in the name table either"
    );

    // 3 · its other tools work — really dispatched, not merely registered.
    for name in ["query", "explain"] {
        assert!(
            contributions.iter().any(|c| c.name == name),
            "`{name}` should have loaded"
        );
        let r#ref: ToolRef = format!("buildgraph.{name}").parse().unwrap();
        let ctx = CallCtx::new(
            CallId::new(),
            Subject::Ext(ext.clone()),
            Fixture::scope(),
            budget(),
        );
        let out = registry
            .dispatch(&r#ref, serde_json::json!({ "q": "deps" }), ctx)
            .await
            .expect("the harness could carry the call");
        assert!(
            matches!(out, Outcome::Ok { .. }),
            "`{name}` should still work: {out:?}"
        );
    }

    // 4 · the audit holds the decision, with the rule that produced it.
    let records = fx.audit.records();
    let decision = records
        .iter()
        .find_map(|r| match &r.event {
            AuditEvent::CapabilityDecision {
                subject,
                request,
                verdict,
                rule_text,
                layer,
                ..
            } if request.starts_with("spawn(") => {
                Some((subject.clone(), *verdict, rule_text.clone(), *layer))
            }
            _ => None,
        })
        .expect("the spawn decision is in the audit");
    assert_eq!(decision.0, Subject::Ext(ext.clone()));
    assert_eq!(decision.1, orrery_audit::Verdict::Deny);
    assert_eq!(
        decision.2.as_deref(),
        Some("spawn(*)"),
        "the audit must name the rule that produced the decision"
    );
    assert_eq!(decision.3, Some(Layer::Project));

    // ...and the load itself is recorded as degraded, with the problem.
    let load = records
        .iter()
        .find_map(|r| match &r.event {
            AuditEvent::ExtensionLoad {
                status, problems, ..
            } => Some((status.clone(), problems.clone())),
            _ => None,
        })
        .expect("the load is in the audit");
    assert_eq!(load.0, "degraded");
    assert_eq!(load.1.len(), 1);
}

/// And the enforcement is not the pattern: even if the tool had loaded, the
/// broker would refuse, because the engine never mints a token for a denied
/// spawn.
#[tokio::test]
async fn a_denied_spawn_never_gets_a_token() {
    let fx = Fixture::with_rules(RULES);
    let ext = Subject::Ext(ExtId::new("buildgraph").unwrap());
    let call = PendingCall::spawn("bazel build //...");
    let decision = fx.engine.check(&call, &ext, &Fixture::scope());
    assert!(
        matches!(decision, orrery_policy::Decision::Deny { .. }),
        "{decision:?}"
    );
    // There is no token, so there is nothing to hand the broker. That is the
    // whole design: the refusal is a missing capability, not a checked flag.
}

/// Nothing denied: the same install loads clean.
#[tokio::test]
async fn an_extension_that_is_granted_everything_loads_ok() {
    let fx = Fixture::new();
    let engine = Arc::new(fx.engine);
    let audit = Arc::clone(&fx.audit) as orrery_audit::Audit;
    let ext = ExtId::new("buildgraph").unwrap();
    let mut registry = Registry::with_host(Arc::new(EchoHost))
        .with_policy(Arc::new(EngineGate::new(Arc::clone(&engine))));

    let outcome = install(
        &mut registry,
        &engine,
        &audit,
        &ext,
        Layer::Project,
        &[ToolNeeds::needing(
            "build",
            Aspect::Spawn,
            "bazel build //...",
        )],
        &Fixture::scope(),
    );
    assert!(matches!(outcome, LoadOutcome::Ok { .. }), "{outcome:?}");
}
