//! `MemoryKernel` behind the kernel's `MemoryRecall` hole.
//!
//! Plan 12 built the memory half — scopes, lifetimes, visibility, the clamp —
//! and plan 05 left a typed hole for it. Neither crate may name the other:
//! `orrery-memory` is published and `orrery-kernel` is not, and a dependency
//! the other way would make the kernel know about stores. The facade is the
//! one place allowed to name both, which is where the two meet.
//!
//! What crosses the join is one call per turn. `MemoryKernel::recall` returns
//! one [`Recall`](orrery_memory::Recall) per provider that contributed, and the
//! kernel writes one `TurnKind::Recalled` row for each — so a replay says which
//! store said what, which is the whole reason the rows are per provider.
//!
//! Implementation plan: `harness/docs/plans/12-memory.md`

use std::sync::Arc;

use async_trait::async_trait;
use orrery_kernel::MemoryRecall;
use orrery_memory::{Actor, MemoryKernel};
use orrery_proto::{TokenBudget, UserInput};
use orrery_session::RecalledEntry;

/// The kernel's memory, backed by a [`MemoryKernel`].
///
/// The [`Actor`] is built from the branch the turn runs on, never from anything
/// an extension said about itself — that is what makes the visibility rule
/// structural rather than advisory.
pub struct KernelMemory {
    memory: Arc<MemoryKernel>,
    actor: Actor,
}

impl std::fmt::Debug for KernelMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KernelMemory")
            .field("memory", &self.memory)
            .finish_non_exhaustive()
    }
}

impl KernelMemory {
    /// Read memory as this actor.
    #[must_use]
    pub const fn new(memory: Arc<MemoryKernel>, actor: Actor) -> Self {
        Self { memory, actor }
    }

    /// The memory kernel underneath, for the ledger the host drains into audit.
    #[must_use]
    pub fn inner(&self) -> &Arc<MemoryKernel> {
        &self.memory
    }
}

#[async_trait]
impl MemoryRecall for KernelMemory {
    /// One name for a thing that may have several providers behind it. It is
    /// only used when [`recall`](MemoryRecall::recall) is: the turn calls
    /// [`recall_rows`](MemoryRecall::recall_rows), which names each provider.
    fn name(&self) -> &str {
        "memory"
    }

    async fn recall(&self, input: &UserInput, _budget: TokenBudget) -> Vec<RecalledEntry> {
        self.memory
            .recall(&self.actor, &input.text)
            .await
            .into_iter()
            .flat_map(|recall| recall.entries)
            .collect()
    }

    async fn recall_rows(
        &self,
        input: &UserInput,
        _budget: TokenBudget,
    ) -> Vec<(String, Vec<RecalledEntry>)> {
        // The allowance is the memory kernel's own, set from the profile when
        // it was built: this is the clamp plan 12 owns, and re-deriving a
        // budget here would be a second opinion about the same window.
        self.memory
            .recall(&self.actor, &input.text)
            .await
            .into_iter()
            .map(|recall| (recall.provider, recall.entries))
            .collect()
    }
}
