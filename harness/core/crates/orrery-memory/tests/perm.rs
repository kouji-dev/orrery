//! Task 8 · `mem.read` and `mem.write` are per-scope capabilities, checked at
//! the kernel boundary against the real rule grammar.

mod common;

use std::sync::Arc;

use common::{budget, entry, lifecycle, open_scope, texts};
use orrery_memory::testing::InMemoryProvider;
use orrery_memory::{Actor, MemError, MemPermissions, MemScope, MemoryKernel};
use orrery_policy::{Decision, PolicyBuilder, PolicyEngine};
use orrery_proto::{AgentScope, Aspect, BranchId, Layer, SessionId, Subject};

/// The real engine, behind this crate's own trait.
///
/// `orrery-policy` is `publish = false` and `orrery-memory` is published, so
/// the kernel takes a trait and the host — or, here, the test — supplies the
/// engine. The rules below are exactly what plan 12 writes in its Permissions
/// section.
struct PolicyPermissions(PolicyEngine);

impl MemPermissions for PolicyPermissions {
    fn check(
        &self,
        subject: &Subject,
        agent: &AgentScope,
        aspect: Aspect,
        scope: &MemScope,
    ) -> Result<(), String> {
        let call = orrery_policy::PendingCall::new(aspect, scope.kind().name());
        match self.0.check(&call, subject, agent) {
            Decision::Allow { .. } => Ok(()),
            Decision::Ask { .. } => Err(format!("`{}` needs consent", call.match_text())),
            Decision::Deny { reason, .. } => Err(reason),
            // `Decision` is `#[non_exhaustive]`; anything new is not an allow.
            _ => Err(format!("`{}` was not allowed", call.match_text())),
        }
    }
}

const RULES: &str = r#"
[permissions]
allow = ["mem.read(*)", "mem.write(*)"]

[permissions."agent:critic"]
deny  = ["mem.write(global)", "mem.write(workspace)"]
allow = ["mem.read(*)", "mem.write(session)", "mem.write(branch)", "mem.write(turn)"]
"#;

fn engine() -> PolicyPermissions {
    let rules = PolicyBuilder::new(".")
        .layer_toml(RULES, "orrery.toml", Layer::Project, false)
        .expect("parse")
        .build()
        .expect("build");
    PolicyPermissions(PolicyEngine::new(rules).with_consent(orrery_policy::ConsentMode::Never))
}

/// The critic runs at session scope — wide enough that the *width* rule would
/// let it write `global`. What stops it is the permission rule.
fn critic(session: SessionId, branch: BranchId) -> Actor {
    let mut actor = Actor::root(
        Subject::SubAgent("critic".to_owned()),
        open_scope("critic", branch),
        session,
        branch,
    );
    actor.turn = None;
    actor
}

#[tokio::test]
async fn mem_write_global_can_be_denied() {
    let provider = Arc::new(InMemoryProvider::anything("test"));
    let kernel = MemoryKernel::new()
        .with_provider(provider.clone())
        .with_permissions(Arc::new(engine()))
        .with_allowance(budget(10_000));
    let session = SessionId::new();
    let branch = BranchId::new();
    let _s = kernel.lifetimes().enter(MemScope::Session(session));
    let actor = critic(session, branch);
    let w = lifecycle(session, branch);

    let err = kernel
        .write(
            &w.witness(),
            &actor,
            MemScope::Global,
            entry("smuggled", "hunter2"),
        )
        .await
        .expect_err("a denied global write went through");
    assert!(matches!(err, MemError::Denied { .. }), "{err:?}");
    assert_eq!(provider.len(), 0, "the store was reached anyway");

    // Audited, with the aspect and the scope named.
    let refusals: Vec<String> = kernel
        .ledger()
        .iter()
        .filter_map(|e| match e {
            orrery_memory::MemEvent::Refused { request, .. } => Some(request.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(refusals, vec!["mem.write(global)".to_owned()]);

    // Session writes still work.
    kernel
        .write(
            &w.witness(),
            &actor,
            MemScope::Session(session),
            entry("note", "the loader is the bottleneck"),
        )
        .await
        .expect("session write");
    assert_eq!(provider.len(), 1);
    assert_eq!(
        texts(&kernel.recall(&actor, "loader").await),
        vec!["the loader is the bottleneck".to_owned()],
    );
}
