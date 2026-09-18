//! Task 4 · Visibility. The security-relevant one.
//!
//! Read down your own chain, never across siblings, and never write to a scope
//! wider than the one you run in — otherwise a sub-agent denied `write` puts a
//! secret in `global` for its parent to read back. Every assertion here is
//! against the **kernel**: the provider in these tests would happily do
//! whatever it was asked.

mod common;

use std::sync::Arc;

use common::{budget, child_actor, entry, lifecycle, root_actor, texts};
use orrery_memory::testing::InMemoryProvider;
use orrery_memory::{MemError, MemScope, MemoryKernel};
use orrery_proto::{BranchId, SessionId};

#[tokio::test]
async fn no_sibling_reads() {
    let kernel = MemoryKernel::new()
        .with_provider(Arc::new(InMemoryProvider::anything("test")))
        .with_allowance(budget(10_000));
    let session = SessionId::new();
    let parent = BranchId::new();
    let left = BranchId::new();
    let right = BranchId::new();

    let _root = kernel.lifetimes().enter(MemScope::Session(session));
    let _l = kernel.lifetimes().enter(MemScope::Branch(left));
    let _r = kernel.lifetimes().enter(MemScope::Branch(right));

    let a = child_actor("left", session, &[parent], left);
    let b = child_actor("right", session, &[parent], right);

    kernel
        .write(
            &lifecycle(session, left).witness(),
            &a,
            MemScope::Branch(left),
            entry("secret", "the api key is hunter2"),
        )
        .await
        .expect("write");

    assert_eq!(texts(&kernel.recall(&a, "api").await).len(), 1);
    assert!(
        texts(&kernel.recall(&b, "api").await).is_empty(),
        "a sibling read another branch's notes",
    );
}

#[tokio::test]
async fn no_writing_wider_than_you_run() {
    let provider = Arc::new(InMemoryProvider::anything("permissive"));
    let kernel = MemoryKernel::new()
        .with_provider(provider.clone())
        .with_allowance(budget(10_000));
    let session = SessionId::new();
    let parent = BranchId::new();
    let child = BranchId::new();
    let _g = kernel.lifetimes().enter(MemScope::Branch(child));
    let actor = child_actor("worker", session, &[parent], child);

    for wider in [
        MemScope::Global,
        MemScope::Workspace("/ws".to_owned()),
        MemScope::Session(session),
    ] {
        let err = kernel
            .write(
                &lifecycle(session, child).witness(),
                &actor,
                wider.clone(),
                entry("smuggled", "the api key is hunter2"),
            )
            .await
            .expect_err("a sub-agent wrote wider than it runs");
        assert!(
            matches!(err, MemError::Denied { .. }),
            "expected a denial, got {err:?}",
        );
    }

    // The provider never saw any of it. The rule is the kernel's, not a thing
    // the store is trusted to honour.
    assert_eq!(provider.len(), 0);

    // Its own branch still works.
    kernel
        .write(
            &lifecycle(session, child).witness(),
            &actor,
            MemScope::Branch(child),
            entry("mine", "a note in my own branch"),
        )
        .await
        .expect("write at its own scope");
    assert_eq!(provider.len(), 1);
}

#[tokio::test]
async fn reads_down_your_own_chain() {
    let kernel = MemoryKernel::new()
        .with_provider(Arc::new(InMemoryProvider::anything("test")))
        .with_allowance(budget(10_000));
    let session = SessionId::new();
    let parent = BranchId::new();
    let child = BranchId::new();
    let _s = kernel.lifetimes().enter(MemScope::Session(session));
    let _p = kernel.lifetimes().enter(MemScope::Branch(parent));
    let _c = kernel.lifetimes().enter(MemScope::Branch(child));

    let root = root_actor(session, parent);
    let kid = child_actor("worker", session, &[parent], child);

    kernel
        .write(
            &lifecycle(session, parent).witness(),
            &root,
            MemScope::Session(session),
            entry("plan", "we are refactoring the loader"),
        )
        .await
        .expect("write");
    kernel
        .write(
            &lifecycle(session, parent).witness(),
            &root,
            MemScope::Branch(parent),
            entry("parent-note", "the parent's own branch note"),
        )
        .await
        .expect("write");

    let seen = texts(&kernel.recall(&kid, "").await);
    assert!(
        seen.contains(&"we are refactoring the loader".to_owned()),
        "a child could not read its parent's session scope: {seen:?}",
    );
    assert!(
        seen.contains(&"the parent's own branch note".to_owned()),
        "a child could not read up its own chain: {seen:?}",
    );
}
