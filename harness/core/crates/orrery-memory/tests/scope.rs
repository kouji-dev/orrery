//! Task 1 · Scopes are handles on things the kernel already creates and
//! destroys, so lifetime and cleanup come for free.
//!
//! The case that earns the design is [`branch_scope_dies_with_the_branch`]:
//! **nothing in it calls `forget`**. The branch guard is dropped — which is what
//! discarding a branch *is* — and the note is gone.

mod common;

use std::sync::Arc;

use common::{budget, child_actor, entry, lifecycle, root_actor, texts};
use orrery_memory::testing::InMemoryProvider;
use orrery_memory::{MemScope, MemoryKernel};
use orrery_proto::{BranchId, SessionId, TurnId};

#[tokio::test]
async fn branch_scope_dies_with_the_branch() {
    let kernel = MemoryKernel::new()
        .with_provider(Arc::new(InMemoryProvider::anything("test")))
        .with_allowance(budget(10_000));
    let session = SessionId::new();
    let parent = BranchId::new();
    let child = BranchId::new();
    let actor = child_actor("worker", session, &[parent], child);
    let w = lifecycle(session, child);

    // The branch exists for as long as the guard does.
    let guard = kernel.lifetimes().enter(MemScope::Branch(child));
    kernel
        .write(
            &w.witness(),
            &actor,
            MemScope::Branch(child),
            entry("note", "the sub-agent's scratch note"),
        )
        .await
        .expect("write");
    assert_eq!(
        texts(&kernel.recall(&actor, "scratch").await),
        vec!["the sub-agent's scratch note".to_owned()],
    );

    // Discard the branch. That is the *only* call: no `forget`, no sweep, no
    // cleanup handler anywhere in this test.
    drop(guard);

    assert!(
        texts(&kernel.recall(&actor, "scratch").await).is_empty(),
        "a discarded branch left residue behind",
    );
}

#[tokio::test]
async fn turn_scope_dies_at_turn_end() {
    let kernel = MemoryKernel::new()
        .with_provider(Arc::new(InMemoryProvider::anything("test")))
        .with_allowance(budget(10_000));
    let session = SessionId::new();
    let branch = BranchId::new();
    let turn = TurnId::new();
    let mut actor = root_actor(session, branch);
    actor.turn = Some(turn);
    let w = lifecycle(session, branch);

    let guard = kernel.lifetimes().enter(MemScope::Turn(turn));
    kernel
        .write(
            &w.witness(),
            &actor,
            MemScope::Turn(turn),
            entry("draft", "half a thought"),
        )
        .await
        .expect("write");
    assert_eq!(texts(&kernel.recall(&actor, "thought").await).len(), 1);

    drop(guard);
    assert!(texts(&kernel.recall(&actor, "thought").await).is_empty());
}

#[tokio::test]
async fn global_survives_a_session() {
    let kernel = MemoryKernel::new()
        .with_provider(Arc::new(InMemoryProvider::anything("test")))
        .with_allowance(budget(10_000));
    let first = SessionId::new();
    let branch = BranchId::new();
    let actor = root_actor(first, branch);
    let w = lifecycle(first, branch);

    let session_guard = kernel.lifetimes().enter(MemScope::Session(first));
    kernel
        .write(
            &w.witness(),
            &actor,
            MemScope::Global,
            entry("pref", "prefers tabs"),
        )
        .await
        .expect("write");
    kernel
        .write(
            &w.witness(),
            &actor,
            MemScope::Session(first),
            entry("here", "only this session"),
        )
        .await
        .expect("write");
    drop(session_guard);

    // A second session. The global note is still there; the first session's is
    // not, and nobody cleaned it up.
    let second = SessionId::new();
    let later = root_actor(second, BranchId::new());
    let _guard = kernel.lifetimes().enter(MemScope::Session(second));
    let seen = texts(&kernel.recall(&later, "").await);
    assert!(seen.contains(&"prefers tabs".to_owned()), "{seen:?}");
    assert!(!seen.contains(&"only this session".to_owned()), "{seen:?}");
}

/// Open question 2, as a test: clearing is **lazy**. A scope that has left the
/// config set stops resolving at once; the bytes go on the next sweep.
#[tokio::test]
async fn a_retired_scope_clears_lazily() {
    let provider = Arc::new(InMemoryProvider::anything("test"));
    let kernel = MemoryKernel::new()
        .with_provider(provider.clone())
        .with_allowance(budget(10_000));
    let session = SessionId::new();
    let branch = BranchId::new();
    let mut actor = root_actor(session, branch);
    actor.project = Some("/repo".to_owned());
    let w = lifecycle(session, branch);

    let guard = kernel
        .lifetimes()
        .enter(MemScope::Project("/repo".to_owned()));
    kernel
        .write(
            &w.witness(),
            &actor,
            MemScope::Project("/repo".to_owned()),
            entry("layout", "src/ then tests/"),
        )
        .await
        .expect("write");
    drop(guard);

    // Invisible immediately...
    assert!(texts(&kernel.recall(&actor, "layout").await).is_empty());
    // ...but still on disk until somebody sweeps.
    assert_eq!(provider.len(), 1);
    let swept = kernel.sweep(&w.witness()).await;
    assert_eq!(swept, 1);
    assert_eq!(provider.len(), 0);
}
