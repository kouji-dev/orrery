//! Plan 12 at the kernel: what memory contributed is in the tree.
//!
//! `orrery-memory`'s own `record::recalled_is_in_the_turn` builds its `TurnRow`s
//! by hand, so it proves the *algebra* — a `Recalled` row replays as resolved
//! content — and not that anything writes one. This is the other half: a real
//! turn, through the real kernel, against the real sqlite store, with a real
//! `MemoryKernel` behind the `MemoryRecall` hole.
//!
//! No network, no key: the model is a committed `.jsonl` replayed by the fixture
//! provider, and the memory store is `orrery-memory`'s in-test provider.

mod common;

use std::sync::Arc;

use common::{Passes, Rig, TestHost, fixture, registry};
use orrery_harness::KernelMemory;
use orrery_kernel::{Kernel, KernelConfig, TurnInput};
use orrery_memory::{
    Actor, LifecycleCtx, LifecyclePoint, MemEntry, MemScope, MemoryKernel,
    testing::InMemoryProvider,
};
use orrery_proto::{Subject, TokenBudget, UserInput};
use orrery_session::TurnKind;

fn budget(max: u64) -> TokenBudget {
    TokenBudget { max, reserve: 0 }
}

fn entry(key: &str, text: &str) -> MemEntry {
    MemEntry::new(key, text)
}

/// A memory kernel with one thing in it, and the actor that may read it.
async fn stocked(rig: &Rig, id: &str, text: &str) -> (Arc<MemoryKernel>, Actor) {
    let provider = Arc::new(InMemoryProvider::anything(id));
    let memory = Arc::new(
        MemoryKernel::new()
            .with_provider(provider)
            .with_allowance(budget(10_000)),
    );
    let actor = Actor::root(Subject::Agent, rig.scope(), rig.session, rig.branch);
    let lifecycle = LifecycleCtx::at(LifecyclePoint::TurnEnd, rig.session, rig.branch);
    memory
        .write(
            &lifecycle.witness(),
            &actor,
            MemScope::Global,
            entry("pref", text),
        )
        .await
        .expect("the write lands");
    (memory, actor)
}

/// A real turn records what memory contributed, and the replay reproduces the
/// context even after the store has moved on.
#[tokio::test]
async fn a_turn_records_what_memory_contributed() {
    let rig = Rig::open().await;
    let (memory, actor) = stocked(&rig, "before", "the user prefers tabs").await;

    let kernel = Kernel::new(
        rig.store.clone(),
        Passes::repeating(fixture("text-turn.jsonl")),
        Arc::new(registry(TestHost::echoing())),
        KernelConfig::default(),
    )
    .with_memory(Arc::new(KernelMemory::new(memory.clone(), actor.clone())));

    kernel
        .run_turn(
            rig.lease().await,
            // The in-test provider matches on substring, so the question is
            // worded the way a retrieval that works would be.
            TurnInput::new(rig.session, UserInput::text("prefers"), rig.scope()),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("the turn runs");

    // The row is in the tree, as resolved content, before the question it
    // answers.
    let rows = rig.rows().await;
    let tags: Vec<&str> = rows.iter().map(|r| r.kind.tag()).collect();
    assert_eq!(
        tags.first(),
        Some(&"recalled"),
        "memory is written before the user's question: {tags:?}"
    );
    let TurnKind::Recalled { provider, entries } = &rows[0].kind else {
        panic!("a Recalled row");
    };
    assert_eq!(provider, "before", "the row names the store that answered");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text, "the user prefers tabs");

    // The context the turn had.
    let before = rig.materialised().await;
    assert!(
        before.contains("the user prefers tabs"),
        "the model saw it: {before}"
    );

    // --- the store moves on -------------------------------------------------
    let (after_store, actor2) = stocked(&rig, "after", "the user prefers spaces").await;
    let fresh = after_store.recall(&actor2, "prefers").await;
    assert_eq!(
        fresh[0].entries[0].text, "the user prefers spaces",
        "the swap is real"
    );

    // The replay does not care: the context is the one the turn had, because it
    // is content in the tree and not a query to re-run.
    let replayed = rig.materialised().await;
    assert_eq!(replayed, before, "same rows, same context");
    assert!(!replayed.contains("spaces"), "nothing re-queried: {replayed}");
}
