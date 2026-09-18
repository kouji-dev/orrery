//! What memory did, in a form the host forwards to the audit stream.
//!
//! A local ledger rather than an `orrery-audit` dependency, for the reason in
//! [`crate::perm`]: this crate is published and `orrery-audit` is not. The host
//! drains it and writes each entry as an `AuditEvent` — a refusal becomes a
//! `capability.decision`, a write or a clip becomes a `content.ref`. Nothing
//! here carries a memory body: keys and counts only.

use serde::{Deserialize, Serialize};

/// One thing memory did.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum MemEvent {
    /// A provider contributed to a context.
    Recalled {
        /// Which provider.
        provider: String,
        /// How many scopes it was asked about.
        scopes: usize,
        /// How many entries survived the clamp.
        entries: usize,
        /// What they cost.
        tokens: u64,
    },
    /// The clamp dropped something. **This is why the clamp is not silent.**
    Clipped {
        /// Which provider was chatty.
        provider: String,
        /// How many entries went.
        dropped: usize,
        /// What it was allowed.
        allowance: u64,
        /// What it actually got.
        used: u64,
    },
    /// An entry was written.
    Wrote {
        /// Which provider.
        provider: String,
        /// Which scope, as `branch:<uuid>`.
        scope: String,
        /// The key. **Not** the text.
        key: String,
    },
    /// Entries were forgotten.
    Forgot {
        /// Which provider.
        provider: String,
        /// Which scope.
        scope: String,
        /// How many went.
        removed: u64,
    },
    /// The kernel refused before the store was reached.
    Refused {
        /// What was asked for, in rule-grammar form — `mem.write(global)`.
        request: String,
        /// Which scope, in full.
        scope: String,
        /// Why.
        reason: String,
    },
    /// A provider failed. Memory is never load-bearing, so this is a record,
    /// not an outcome.
    Failed {
        /// Which provider.
        provider: String,
        /// What went wrong.
        message: String,
    },
}
