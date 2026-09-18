//! The trait every runtime implements, and the one conversion the boundary
//! needs.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_ext_api::{CallCtx, ExtensionManifest, HostError, RuntimeKind, ToolDef};
use orrery_proto::{ExtId, Grant, LoadOutcome, Outcome};
use serde_json::Value;

/// One way of reaching extension code.
///
/// Four runtimes implement this and the table does not know which is which:
/// `native` is compiled in, `node` and `process` are children over JSON-RPC,
/// `wasm` is plan 14. The manifest, the dispatch path and the ledger are the
/// same for all of them, which is the whole reason `native` exists rather than
/// a privileged in-kernel path (translation #14).
#[async_trait]
pub trait ExtensionHost: Send + Sync {
    /// Which runtime this is.
    fn runtime(&self) -> RuntimeKind;

    /// Bring an extension up under a grant, and say how it went.
    ///
    /// Never returns an `Err`: "it did not load" is a reportable state, not a
    /// failure of the caller's call. A host that cannot even try answers
    /// [`LoadOutcome::Failed`] with the stage it got to.
    async fn load(&self, manifest: Arc<ExtensionManifest>, grant: Grant) -> LoadOutcome;

    /// What it actually contributes, after loading. Empty before.
    fn tools(&self, ext: &ExtId) -> Vec<ToolDef>;

    /// Which of its tools cannot work, and are therefore not offered.
    fn disabled(&self, ext: &ExtId) -> Vec<String> {
        let _ = ext;
        Vec::new()
    }

    /// Run one of its tools.
    ///
    /// A refusal is `Ok(Outcome::Denied)`; an extension that is gone is
    /// `Ok(Outcome::Unloaded)`. `Err` is for the harness itself breaking.
    async fn call(
        &self,
        ext: &ExtId,
        tool: &str,
        input: Value,
        ctx: CallCtx,
    ) -> Result<Outcome, HostError>;

    /// Take it down. Live: the session does not restart.
    async fn unload(&self, ext: &ExtId) -> Result<(), HostError>;
}

/// The registry's ceiling, as an extension sees it.
///
/// One function, because there is one boundary. See
/// [`orrery_ext_api::ToolBudget`] for why there are two types at all.
#[must_use]
pub fn ceiling_of(budget: orrery_tools::ToolBudget) -> orrery_ext_api::ToolBudget {
    orrery_ext_api::ToolBudget {
        wall_clock_ms: budget.wall_clock_ms,
        output_bytes: budget.output_bytes,
        memory_bytes: budget.memory_bytes,
    }
}

/// The same, the other way, for a ceiling a manifest declared.
#[must_use]
pub fn budget_of(ceiling: orrery_ext_api::ToolBudget) -> orrery_tools::ToolBudget {
    orrery_tools::ToolBudget {
        wall_clock_ms: ceiling.wall_clock_ms,
        output_bytes: ceiling.output_bytes,
        memory_bytes: ceiling.memory_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::{budget_of, ceiling_of};

    #[test]
    fn the_two_ceilings_are_the_same_three_numbers() {
        let registry_side = orrery_tools::ToolBudget::new(10, 20).with_memory(30);
        let round_tripped = budget_of(ceiling_of(registry_side));
        assert_eq!(round_tripped, registry_side);
    }
}
