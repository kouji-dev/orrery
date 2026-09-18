//! The reusable conformance suite: what §4.14 means by "checks a
//! `MemoryProvider` against the scope lifetimes".
//!
//! Every provider runs the same tests. It lives in `src/` rather than in
//! `tests/` for the obvious reason: a `tests/` binary cannot be linked by
//! another crate, and the whole point is that `orrery-ext-memory-file` runs
//! exactly the suite the in-test provider runs.
//!
//! ```no_run
//! # use std::sync::Arc;
//! # async fn go(p: Arc<dyn orrery_memory::MemoryProvider>) {
//! orrery_memory::conformance::run_conformance(p).await;
//! # }
//! ```
//!
//! Each case is `pub` so a provider can run one on its own while it is being
//! built. They panic rather than returning a `Result`, because they are tests.
//! Every case cleans up after itself, so a file-backed provider can run the
//! suite against a real state directory without leaving anything behind.

use std::sync::Arc;

use orrery_proto::{
    AgentScope, BranchId, Consent, Grant, RunId, SessionId, Subject, TokenBudget, TurnId,
};

use crate::error::MemError;
use crate::ledger::MemEvent;
use crate::provider::{MemEntry, MemSelector, MemoryProvider, RecallQuery};
use crate::scope::{MemScope, ScopeKind};
use crate::visibility::Actor;
use crate::witness::{LifecycleCtx, LifecyclePoint, LifecycleWitness};
use crate::{MemoryKernel, Recall};

/// Anything a provider must satisfy. Panics on the first failure.
pub async fn run_conformance(provider: Arc<dyn MemoryProvider>) {
    declared_scopes_round_trip(provider.as_ref()).await;
    undeclared_scopes_are_refused(provider.as_ref()).await;
    scopes_do_not_leak_into_each_other(provider.as_ref()).await;
    forget_selects(provider.as_ref()).await;
    ephemeral_scopes_die_with_their_object(Arc::clone(&provider)).await;
    the_clamp_cuts_a_chatty_answer(Arc::clone(&provider)).await;
    a_sibling_cannot_read(Arc::clone(&provider)).await;
    nobody_writes_wider_than_they_run(Arc::clone(&provider)).await;
}

/// A fixture, and every scope handle derived from it.
struct Fixture {
    session: SessionId,
    branch: BranchId,
    run: RunId,
    turn: TurnId,
    workspace: String,
    project: String,
}

impl Fixture {
    fn new() -> Self {
        let session = SessionId::new();
        let workspace = format!("/conformance/{session}");
        Self {
            session,
            branch: BranchId::new(),
            run: RunId::new(),
            turn: TurnId::new(),
            project: format!("{workspace}/project"),
            workspace,
        }
    }

    fn scope(&self, kind: ScopeKind) -> MemScope {
        match kind {
            ScopeKind::Global => MemScope::Global,
            ScopeKind::Workspace => MemScope::Workspace(self.workspace.clone()),
            ScopeKind::Project => MemScope::Project(self.project.clone()),
            ScopeKind::Session => MemScope::Session(self.session),
            ScopeKind::Workflow => MemScope::Workflow(self.run),
            ScopeKind::Branch => MemScope::Branch(self.branch),
            ScopeKind::Turn => MemScope::Turn(self.turn),
        }
    }

    fn actor(&self) -> Actor {
        Actor::root(
            Subject::Agent,
            agent("agent", self.branch),
            self.session,
            self.branch,
        )
        .in_workspace(&self.workspace)
        .in_project(&self.project)
        .in_workflow(self.run)
        .in_turn(self.turn)
    }
}

fn agent(name: &str, branch: BranchId) -> AgentScope {
    AgentScope {
        agent: name.to_owned(),
        branch,
        tools: Vec::new(),
        grant: Grant {
            capabilities: Vec::new(),
            consent: Consent::Always,
        },
    }
}

fn witness() -> LifecycleWitness {
    LifecycleCtx::at(LifecyclePoint::TurnEnd, SessionId::new(), BranchId::new()).witness()
}

fn budget(max: u64) -> TokenBudget {
    TokenBudget { max, reserve: 0 }
}

fn texts(recalls: &[Recall]) -> Vec<String> {
    recalls
        .iter()
        .flat_map(|r| r.entries.iter().map(|e| e.text.clone()))
        .collect()
}

async fn recall_all(p: &dyn MemoryProvider, scopes: Vec<MemScope>, q: &str) -> Vec<MemEntry> {
    p.recall(RecallQuery {
        scopes,
        query: q.to_owned(),
        budget: budget(u64::MAX / 2),
    })
    .await
    .expect("recall")
}

/// Write, recall, forget — in every scope the provider says it keeps.
pub async fn declared_scopes_round_trip(p: &dyn MemoryProvider) {
    let fx = Fixture::new();
    for kind in p.scopes() {
        let scope = fx.scope(*kind);
        let entry = MemEntry::new(format!("k-{kind}"), format!("a note in {kind} scope"));
        p.write(&witness(), scope.clone(), entry.clone())
            .await
            .unwrap_or_else(|e| panic!("`{}` declares `{kind}` but refused a write: {e}", p.id()));

        let found = recall_all(p, vec![scope.clone()], "a note in").await;
        assert!(
            found.iter().any(|e| e.text == entry.text),
            "`{}` kept nothing in `{kind}`",
            p.id(),
        );

        let removed = p
            .forget(&witness(), scope.clone(), MemSelector::All)
            .await
            .expect("forget");
        assert!(removed >= 1, "`{}` forgot nothing in `{kind}`", p.id());
        assert!(
            recall_all(p, vec![scope], "a note in").await.is_empty(),
            "`{}` still answers from a scope it forgot",
            p.id(),
        );
    }
}

/// A scope the provider does not declare is a **declared error**, not a silent
/// success. This is the case that catches a provider pretending to be wider
/// than it is.
pub async fn undeclared_scopes_are_refused(p: &dyn MemoryProvider) {
    let fx = Fixture::new();
    for kind in ScopeKind::ALL {
        if p.scopes().contains(&kind) {
            continue;
        }
        let scope = fx.scope(kind);
        match p.write(&witness(), scope, MemEntry::new("k", "v")).await {
            Err(MemError::UnsupportedScope { .. }) => {}
            other => panic!(
                "`{}` answered `{kind}` with {other:?}; an undeclared scope must be \
                 `MemError::UnsupportedScope`",
                p.id(),
            ),
        }
    }
}

/// Two scopes of the same kind do not see each other, and a recall never
/// answers from a scope it was not asked about.
pub async fn scopes_do_not_leak_into_each_other(p: &dyn MemoryProvider) {
    let a = Fixture::new();
    let b = Fixture::new();
    for kind in p.scopes() {
        if *kind == ScopeKind::Global {
            // There is only one `global`; nothing to separate it from.
            continue;
        }
        let (sa, sb) = (a.scope(*kind), b.scope(*kind));
        p.write(&witness(), sa.clone(), MemEntry::new("a", "belongs to a"))
            .await
            .expect("write a");
        p.write(&witness(), sb.clone(), MemEntry::new("b", "belongs to b"))
            .await
            .expect("write b");

        let seen = recall_all(p, vec![sa.clone()], "belongs").await;
        assert!(
            seen.iter().all(|e| e.text == "belongs to a"),
            "`{}` leaked one `{kind}` scope into another: {seen:?}",
            p.id(),
        );

        p.forget(&witness(), sa, MemSelector::All)
            .await
            .expect("clean a");
        p.forget(&witness(), sb, MemSelector::All)
            .await
            .expect("clean b");
    }
}

/// `forget` semantics: by key, by substring, and all.
pub async fn forget_selects(p: &dyn MemoryProvider) {
    let fx = Fixture::new();
    let Some(kind) = p.scopes().first().copied() else {
        return;
    };
    let scope = fx.scope(kind);
    for (k, v) in [
        ("one", "alpha note"),
        ("two", "beta note"),
        ("three", "gamma note"),
    ] {
        p.write(&witness(), scope.clone(), MemEntry::new(k, v))
            .await
            .expect("write");
    }

    assert_eq!(
        p.forget(
            &witness(),
            scope.clone(),
            MemSelector::Key("one".to_owned())
        )
        .await
        .expect("forget by key"),
        1,
        "`{}` forgot the wrong number of entries by key",
        p.id(),
    );
    assert_eq!(
        p.forget(
            &witness(),
            scope.clone(),
            MemSelector::Contains("beta".to_owned())
        )
        .await
        .expect("forget by substring"),
        1,
        "`{}` forgot the wrong number of entries by substring",
        p.id(),
    );
    assert_eq!(
        p.forget(&witness(), scope.clone(), MemSelector::All)
            .await
            .expect("forget all"),
        1,
        "`{}` left something behind",
        p.id(),
    );
    assert!(recall_all(p, vec![scope], "note").await.is_empty());
}

/// Every ephemeral scope the provider keeps stops resolving when the kernel
/// object it names ends — **with no cleanup call anywhere**.
pub async fn ephemeral_scopes_die_with_their_object(p: Arc<dyn MemoryProvider>) {
    let kinds: Vec<ScopeKind> = p.scopes().to_vec();
    let kernel = MemoryKernel::new()
        .with_provider(Arc::clone(&p))
        .with_allowance(budget(100_000));
    let fx = Fixture::new();
    let actor = fx.actor();

    for kind in kinds {
        let scope = fx.scope(kind);
        let guard = kernel.lifetimes().enter(scope.clone());
        kernel
            .write(
                &witness(),
                &actor,
                scope.clone(),
                MemEntry::new("ephemeral", format!("a note in {kind}")),
            )
            .await
            .expect("write");
        assert!(
            texts(&kernel.recall(&actor, "a note in").await)
                .iter()
                .any(|t| t == &format!("a note in {kind}")),
            "`{}` did not answer from a live `{kind}` scope",
            p.id(),
        );

        drop(guard);

        let after = texts(&kernel.recall(&actor, "a note in").await);
        if kind.is_ephemeral() {
            assert!(
                !after.iter().any(|t| t == &format!("a note in {kind}")),
                "`{kind}` outlived the thing it names",
            );
        } else {
            assert!(
                after.iter().any(|t| t == &format!("a note in {kind}")),
                "`global` should survive its guard",
            );
        }
        // Clean up whatever the kernel did not.
        let _ = p.forget(&witness(), scope, MemSelector::All).await;
    }
    kernel.sweep(&witness()).await;
}

/// The clamp is the kernel's: a provider that answers with far more than the
/// allowance contributes the allowance, and the drop is recorded.
pub async fn the_clamp_cuts_a_chatty_answer(p: Arc<dyn MemoryProvider>) {
    let Some(kind) = p.scopes().first().copied() else {
        return;
    };
    let kernel = MemoryKernel::new()
        .with_provider(Arc::clone(&p))
        .with_allowance(budget(200));
    let fx = Fixture::new();
    let actor = fx.actor();
    let scope = fx.scope(kind);
    let _guard = kernel.lifetimes().enter(scope.clone());

    let body = "chatter ".repeat(30); // 60 tokens each, 600 in total
    for i in 0..10 {
        kernel
            .write(
                &witness(),
                &actor,
                scope.clone(),
                MemEntry::new(format!("c{i}"), &body),
            )
            .await
            .expect("write");
    }

    let recalls = kernel.recall(&actor, "chatter").await;
    let used: u64 = recalls.iter().map(|r| r.used_tokens).sum();
    let dropped: usize = recalls.iter().map(|r| r.dropped).sum();
    assert!(
        used <= 200,
        "`{}` got {used} tokens past a 200 clamp",
        p.id()
    );
    assert!(
        dropped > 0,
        "a 4000-token answer under a 200 allowance dropped nothing"
    );
    assert!(
        kernel
            .ledger()
            .iter()
            .any(|e| matches!(e, MemEvent::Clipped { .. })),
        "the clamp dropped entries and did not say so",
    );

    let _ = p.forget(&witness(), scope, MemSelector::All).await;
}

/// Visibility, rule 2: never across siblings. Skipped for a provider that does
/// not keep `branch` scope at all — there is nothing to leak.
pub async fn a_sibling_cannot_read(p: Arc<dyn MemoryProvider>) {
    if !p.scopes().contains(&ScopeKind::Branch) {
        return;
    }
    let kernel = MemoryKernel::new()
        .with_provider(Arc::clone(&p))
        .with_allowance(budget(100_000));
    let session = SessionId::new();
    let parent = BranchId::new();
    let left = BranchId::new();
    let right = BranchId::new();
    let _l = kernel.lifetimes().enter(MemScope::Branch(left));
    let _r = kernel.lifetimes().enter(MemScope::Branch(right));

    let a = Actor::sub_agent(
        Subject::SubAgent("left".to_owned()),
        agent("left", left),
        session,
        [parent, left],
    );
    let b = Actor::sub_agent(
        Subject::SubAgent("right".to_owned()),
        agent("right", right),
        session,
        [parent, right],
    );

    kernel
        .write(
            &witness(),
            &a,
            MemScope::Branch(left),
            MemEntry::new("secret", "the api key is hunter2"),
        )
        .await
        .expect("write");

    assert_eq!(texts(&kernel.recall(&a, "api").await).len(), 1);
    assert!(
        texts(&kernel.recall(&b, "api").await).is_empty(),
        "`{}` let a sibling read another branch",
        p.id(),
    );

    let _ = p
        .forget(&witness(), MemScope::Branch(left), MemSelector::All)
        .await;
}

/// Visibility, rule 3: never write wider than you run, and the store never sees
/// the attempt.
pub async fn nobody_writes_wider_than_they_run(p: Arc<dyn MemoryProvider>) {
    if !p.scopes().contains(&ScopeKind::Global) {
        return;
    }
    let kernel = MemoryKernel::new()
        .with_provider(Arc::clone(&p))
        .with_allowance(budget(100_000));
    let session = SessionId::new();
    let branch = BranchId::new();
    let _g = kernel.lifetimes().enter(MemScope::Branch(branch));
    let child = Actor::sub_agent(
        Subject::SubAgent("worker".to_owned()),
        agent("worker", branch),
        session,
        [branch],
    );

    let err = kernel
        .write(
            &witness(),
            &child,
            MemScope::Global,
            MemEntry::new("smuggled", "the api key is hunter2"),
        )
        .await
        .expect_err("a sub-agent wrote to global");
    assert!(matches!(err, MemError::Denied { .. }), "{err:?}");
    assert!(
        recall_all(&*p, vec![MemScope::Global], "smuggled")
            .await
            .is_empty(),
        "`{}` was reached by a write the kernel refused",
        p.id(),
    );
}
