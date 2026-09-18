//! Memory ceilings, enforced by wasmtime rather than hoped for.

use orrery_tools::ToolBudget;

/// What one instance may take.
///
/// Both numbers come from the call's [`ToolBudget`], so an extension's ceiling
/// is the same number the rest of the kernel already budgets by rather than a
/// second one that could disagree.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Ceilings {
    /// The most linear memory the guest may hold, in bytes.
    ///
    /// Enforced by `Store::limiter`, so a `memory.grow` past it **fails inside
    /// the guest** — the allocator gets a null, Rust aborts, and the instance
    /// traps. The host does not have to notice afterwards.
    pub memory_bytes: usize,
    /// How long the guest may run, in milliseconds of wall clock.
    ///
    /// Enforced by epoch interruption, which traps at loop backedges and
    /// function entries. See [`crate::cancel`] for what that cannot do.
    pub wall_clock_ms: u64,
    /// How many tables and instances it may make. Small on purpose: a
    /// component that wants thousands is doing something this host does not.
    pub instances: usize,
}

impl Ceilings {
    /// 64 MiB and 30 seconds — the default for a tool nobody budgeted.
    pub const DEFAULT: Self = Self {
        memory_bytes: 64 * 1024 * 1024,
        wall_clock_ms: 30_000,
        instances: 16,
    };

    /// The ceilings a call's budget implies.
    ///
    /// `ToolBudget::memory_bytes` is `None` for in-process tools — which a wasm
    /// guest is not, so a missing number falls back to [`Self::DEFAULT`]'s
    /// rather than to "unlimited". A ceiling that defaults to absent is not a
    /// ceiling.
    #[must_use]
    pub fn from_budget(budget: &ToolBudget) -> Self {
        Self {
            memory_bytes: budget
                .memory_bytes
                .and_then(|b| usize::try_from(b).ok())
                .unwrap_or(Self::DEFAULT.memory_bytes),
            wall_clock_ms: if budget.wall_clock_ms == 0 {
                Self::DEFAULT.wall_clock_ms
            } else {
                budget.wall_clock_ms
            },
            instances: Self::DEFAULT.instances,
        }
    }

    /// The wasmtime limiter these ceilings mean.
    #[must_use]
    pub fn limiter(&self) -> wasmtime::StoreLimits {
        wasmtime::StoreLimitsBuilder::new()
            .memory_size(self.memory_bytes)
            .instances(self.instances)
            .tables(self.instances)
            // A trap, not a `None` from `memory.grow`: the guest asked for more
            // than it may have, and we would rather it stop than quietly
            // mis-handle the failure.
            .trap_on_grow_failure(true)
            .build()
    }
}

impl Default for Ceilings {
    fn default() -> Self {
        Self::DEFAULT
    }
}
