//! The one trait, and the contract every backend owes.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_proto::{BranchId, Seq, SessionId, TokenBudget, TurnId};

use crate::algebra::{Materialised, TokenCounter};
use crate::error::SessionError;
use crate::lease::BranchLease;
use crate::turn::{
    BranchOutcome, CompactResult, NewTurn, SessionHandle, SessionSummary, StoredEvent, TurnRow,
};

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

    /// Every session in the store, **newest first**.
    ///
    /// `open` answers about a session you can already name; this is the one
    /// that answers "which sessions are there", which is what
    /// `orrery session list` asks and what nothing could ask before.
    ///
    /// Required, with no default. The suite's
    /// [`sessions_can_be_enumerated`](crate::conformance::sessions_can_be_enumerated)
    /// is what a backend owes here; a backend that cannot answer has to say so
    /// in its own body rather than inherit a refusal it never read.
    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError>;

    /// Delete a whole session: its events, its turns and its branches.
    ///
    /// **Whole sessions only.** Plan 02 open question 2 asked how retention
    /// should work and answered itself with a constraint — a retention pass may
    /// drop a session, never trim turns inside one, because a half-session is a
    /// transcript that lies. There is therefore no `delete_turn`, and there
    /// never will be: `compact` is how a branch gets shorter, and it writes
    /// rather than mutates.
    ///
    /// Deleting is idempotent up to existence: a session that is not there is
    /// [`SessionError::NoSuchSession`], so a caller can tell "gone now" from
    /// "was never here".
    ///
    /// Required, with no default: see the note on [`list_sessions`](Self::list_sessions).
    async fn delete(&self, session: SessionId) -> Result<(), SessionError>;

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

    /// The rows on one branch, in order, ignoring ancestry and ignoring
    /// watermarks.
    ///
    /// This is the **cold replay** read, and it is deliberately not
    /// `materialise`. `materialise` renders rows into the messages a *model*
    /// sees, and that rendering is lossy on purpose: a tool result becomes a
    /// user-role block with no tool name on it, a summary and a real prompt
    /// become the same shape. Re-emitting a past session as events needs the
    /// rows themselves — which call, which tool, what it cost — so `orrery
    /// replay` reads here and encodes from [`TurnKind`](crate::TurnKind).
    ///
    /// Required, with no default: see the note on [`list_sessions`](Self::list_sessions).
    async fn turns(&self, branch: BranchId) -> Result<Vec<TurnRow>, SessionError>;

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
