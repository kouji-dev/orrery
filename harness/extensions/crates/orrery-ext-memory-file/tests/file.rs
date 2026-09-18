//! Task 7 · The reference provider.
//!
//! Nothing here reaches a network, a model or a paid API: it is a directory of
//! JSONL files in a temp dir.

use std::sync::Arc;

use orrery_ext_memory_file::FileMemory;
use orrery_memory::{
    LifecycleCtx, LifecyclePoint, MemEntry, MemError, MemScope, MemSelector, MemoryProvider,
    ScopeKind,
};
use orrery_proto::{BranchId, RunId, SessionId, TurnId};

fn witness() -> orrery_memory::LifecycleWitness {
    LifecycleCtx::at(LifecyclePoint::TurnEnd, SessionId::new(), BranchId::new()).witness()
}

#[tokio::test]
async fn passes_conformance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let provider = Arc::new(FileMemory::new(dir.path()));
    orrery_memory::conformance::run_conformance(provider).await;
}

#[tokio::test]
async fn only_declares_global_and_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let provider = FileMemory::new(dir.path());
    assert_eq!(provider.scopes(), &[ScopeKind::Global, ScopeKind::Session]);

    for scope in [
        MemScope::Branch(BranchId::new()),
        MemScope::Turn(TurnId::new()),
        MemScope::Workflow(RunId::new()),
        MemScope::Workspace("/ws".to_owned()),
        MemScope::Project("/ws/p".to_owned()),
    ] {
        let kind = scope.kind();
        let err = provider
            .write(&witness(), scope.clone(), MemEntry::new("k", "v"))
            .await
            .expect_err("a scope it does not keep was accepted");
        assert!(
            matches!(&err, MemError::UnsupportedScope { provider, scope } if provider == "memory-file" && scope == kind.name()),
            "{err:?}",
        );
        // And a `forget` is refused the same way, rather than reporting zero.
        assert!(matches!(
            provider
                .forget(&witness(), scope, MemSelector::All)
                .await
                .expect_err("forget accepted an undeclared scope"),
            MemError::UnsupportedScope { .. },
        ));
    }
}

/// Substring plus recency, which is the whole of the retrieval strategy and is
/// documented as such.
#[tokio::test]
async fn retrieval_is_substring_then_recency() {
    let dir = tempfile::tempdir().expect("tempdir");
    let provider = FileMemory::new(dir.path());
    let session = SessionId::new();
    let scope = MemScope::Session(session);

    for (k, v, at) in [
        ("old", "the loader is slow", 1_000),
        ("new", "the loader was fixed", 2_000),
        ("other", "nothing to do with it", 3_000),
    ] {
        provider
            .write(&witness(), scope.clone(), MemEntry::new(k, v).at(at))
            .await
            .expect("write");
    }

    let found = provider
        .recall(orrery_memory::RecallQuery {
            scopes: vec![scope.clone()],
            query: "loader".to_owned(),
            budget: orrery_proto::TokenBudget {
                max: 10_000,
                reserve: 0,
            },
        })
        .await
        .expect("recall");
    assert_eq!(
        found.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
        vec!["new", "old"],
        "newest first, and the non-matching entry is not there",
    );
}

/// It survives being reopened: the point of a file-backed store.
#[tokio::test]
async fn it_is_on_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let provider = FileMemory::new(dir.path());
        provider
            .write(
                &witness(),
                MemScope::Global,
                MemEntry::new("pref", "prefers tabs"),
            )
            .await
            .expect("write");
    }
    let reopened = FileMemory::new(dir.path());
    let found = reopened
        .recall(orrery_memory::RecallQuery {
            scopes: vec![MemScope::Global],
            query: "tabs".to_owned(),
            budget: orrery_proto::TokenBudget {
                max: 10_000,
                reserve: 0,
            },
        })
        .await
        .expect("recall");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, "prefers tabs");
    assert!(dir.path().join("memory").join("global.jsonl").exists());
}
