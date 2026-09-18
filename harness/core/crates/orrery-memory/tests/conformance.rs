//! Task 6 · The suite every `MemoryProvider` runs, exercised here against the
//! in-test provider so it is meaningful before the file provider exists.

use std::sync::Arc;

use orrery_memory::testing::InMemoryProvider;

#[tokio::test]
async fn in_memory_provider_passes_conformance() {
    orrery_memory::conformance::run_conformance(Arc::new(InMemoryProvider::anything("in-test")))
        .await;
}

/// A provider that declares fewer scopes than it is asked for must say so, and
/// the suite has to catch a provider that silently succeeds instead.
#[tokio::test]
async fn a_narrow_provider_passes_the_same_suite() {
    orrery_memory::conformance::run_conformance(Arc::new(InMemoryProvider::new(
        "narrow",
        &[
            orrery_memory::ScopeKind::Global,
            orrery_memory::ScopeKind::Session,
        ],
    )))
    .await;
}
