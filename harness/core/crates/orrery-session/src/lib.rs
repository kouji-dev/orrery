//! The turn tree, the SessionStore trait, the branch lease, and the pure materialise/compact algebra.
//!
//! Implementation plan: `harness/docs/plans/02-session-store.md`
//!
//! # What this crate is for
//!
//! History is the one thing the harness cannot reconstruct. Everything else —
//! a provider, a tool, a memory store — can fail and be replaced; a lost turn
//! is lost. So the session store is the one singleton that cannot degrade
//! (§4.7), and the rules that keep it honest live **here**, in the trait crate,
//! rather than once per backend:
//!
//! - **The lease.** One turn at a time per branch. [`BranchLease`] has no
//!   public constructor, so an `append` that did not go through
//!   [`SessionStore::lease`] cannot be written.
//! - **Turns are immutable.** `compact` writes a summary and a watermark; it
//!   never mutates or deletes what it compacted.
//! - **The parent performs the join.** A child branch never appends to its
//!   parent. See the invariant at the top of [`lease`].
//! - **`materialise` is pure.** [`algebra::materialise`] is a synchronous
//!   function of rows, a watermark, a budget and a counter, so the interesting
//!   half of this crate is testable without a database.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod algebra;
pub mod conformance;
pub mod error;
pub mod lease;
#[path = "trait.rs"]
pub mod store;
pub mod turn;

pub use algebra::{CharsOverFour, Materialised, TokenCounter, materialise};
pub use error::SessionError;
pub use lease::{BranchLease, BranchState, BranchStatus, LeaseRegistry};
pub use store::SessionStore;
pub use turn::{
    BranchOutcome, CompactResult, NewTurn, RecalledEntry, SessionHandle, SessionSummary,
    StoredEvent, TurnKind, TurnRow,
};
