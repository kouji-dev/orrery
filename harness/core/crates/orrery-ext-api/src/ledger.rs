//! The load ledger: what loaded, what degraded, what failed, what was skipped.
//!
//! [`LoadOutcome`] itself is a wire type and lives in `orrery-proto`. What lives
//! here is the collection of them, because it is queried (`query { of: "ledger" }`),
//! rendered by every client and written to the audit stream at session start —
//! and because "was anything degraded?" is a question three crates ask.

use orrery_proto::{Contribution, ExtId, LoadOutcome};
use parking_lot::RwLock;
use std::sync::Arc;

/// Every load decision of one session, in the order they were made.
///
/// Cheap to clone and shared: the host writes, clients read.
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    entries: Arc<RwLock<Vec<LoadOutcome>>>,
}

impl Ledger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one load decision.
    pub fn record(&self, outcome: LoadOutcome) {
        self.entries.write().push(outcome);
    }

    /// Everything, in order.
    #[must_use]
    pub fn all(&self) -> Vec<LoadOutcome> {
        self.entries.read().clone()
    }

    /// The decision about one extension, if there is one.
    ///
    /// The **last** one: an extension can be loaded, unloaded and loaded again,
    /// and what a person wants to know is where it stands now.
    #[must_use]
    pub fn of(&self, ext: &ExtId) -> Option<LoadOutcome> {
        self.entries
            .read()
            .iter()
            .rev()
            .find(|o| o.ext() == ext)
            .cloned()
    }

    /// Everything that did not fully load, which is what a person wants first.
    #[must_use]
    pub fn problems(&self) -> Vec<LoadOutcome> {
        self.entries
            .read()
            .iter()
            .filter(|o| !matches!(o, LoadOutcome::Ok { .. }))
            .cloned()
            .collect()
    }

    /// Everything contributed by everything that loaded.
    #[must_use]
    pub fn contributions(&self) -> Vec<(ExtId, Contribution)> {
        self.entries
            .read()
            .iter()
            .flat_map(|o| {
                o.contributions()
                    .iter()
                    .map(|c| (o.ext().clone(), c.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// How many decisions have been recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    /// Whether nothing has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }
}
