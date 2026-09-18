//! The one trait, and the contract every backend owes.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_proto::{BranchId, Seq, SessionId, TokenBudget, TurnId};

use crate::algebra::{Materialised, TokenCounter};
use crate::error::SessionError;
use crate::lease::BranchLease;
use crate::turn::{BranchOutcome, CompactResult, NewTurn, SessionHandle, StoredEvent};

/// Where the turn tree lives.
///
/// `Arc<dyn SessionStore>`, one per harness, and the only singleton whose
/// failure ends the session rather than degrading it.
///
/// # The contract
///
/// - **`lease` is `try_lock`, never `lock`.** A busy branch is
///   [`SessionError::BranchBusy`], not a queue. Parallel work is parallel
///   branches.
/// - **`append` takes a `&BranchLease`** it cannot forge, and allocates the
///   next [`Seq`] from that lease. Sequence numbers on one branch are
///   one-based, contiguous and never reused.
/// - **`close_branch` writes only to the child.** It takes the lease by value,
///   consuming it, and it never touches the parent. The parent writes its own
///   [`BranchResult`](crate::turn::TurnKind::BranchResult) under its own lease.
///   This is the one deadlock in the design and the reason for that shape.
/// - **`compact` writes, never mutates.** The rows it covers stay readable
///   afterwards; only the *view* `materialise` builds changes.
#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    /// Start a session, with its root branch.
    async fn create(&self, workspace: &str, profile: &str) -> Result<SessionId, SessionError>;

    /// Look one up.
    async fn open(&self, session: SessionId) -> Result<SessionHandle, SessionError>;

    /// Acquire the right to append.
    ///
    /// `try_lock`, never `lock`: a busy branch is refused rather than queued,
    /// so a caller that wanted concurrency has to say so by forking a branch.
    async fn lease(&self, branch: BranchId) -> Result<BranchLease, SessionError>;

    /// Write a turn. The lease says which branch, and hands out the `Seq`.
    async fn append(&self, lease: &BranchLease, turn: NewTurn) -> Result<TurnId, SessionError>;

    /// Fork a new branch at a turn. Takes no lease: forking is a read of the
    /// parent, not a write to it, which is what lets a parent spawn a child
    /// while holding its own lease.
    async fn branch(&self, from: TurnId, label: &str) -> Result<BranchId, SessionError>;

    /// Close a branch. Consumes the lease; writes only the child's own rows.
    async fn close_branch(
        &self,
        lease: BranchLease,
        outcome: BranchOutcome,
    ) -> Result<(), SessionError>;

    /// Build the messages for a branch: its whole ancestry, under the highest
    /// watermark, fitted to a budget by `counter`.
    async fn materialise(
        &self,
        branch: BranchId,
        budget: TokenBudget,
        counter: &dyn TokenCounter,
    ) -> Result<Materialised, SessionError>;

    /// Write a summary turn and a watermark over everything up to `upto`.
    /// One transaction, and the covered rows survive it.
    async fn compact(
        &self,
        lease: &BranchLease,
        upto: Seq,
        summary: NewTurn,
    ) -> Result<CompactResult, SessionError>;

    /// Replay, for `session.attach(since)` and `orrery replay`.
    ///
    /// `since` is exclusive; `None` means from the start. The returned
    /// sequence numbers are contiguous — a client detects a gap by arithmetic.
    async fn events_since(
        &self,
        session: SessionId,
        since: Option<Seq>,
    ) -> Result<Vec<StoredEvent>, SessionError>;
}

/// Convenience so a backend can be handed around as one line.
pub type SharedStore = Arc<dyn SessionStore>;
