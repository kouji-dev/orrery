//! What a [`SessionStore`](crate::SessionStore) can refuse, and why.
//!
//! Every variant here **ends the turn**, and most of them end the session:
//! §4.7 makes the session store the one singleton that cannot degrade, because
//! history is the one thing that cannot be reconstructed. The error type is
//! therefore deliberately narrow — there is no "retry later", because a store
//! that cannot answer is a store that has lost the conversation.

use orrery_proto::{BranchId, Seq, SessionId, TurnId};

/// Why the store said no.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    /// A turn is already in flight on this branch.
    ///
    /// Not a queue: [`SessionStore::lease`](crate::SessionStore::lease) is a
    /// `try_lock`, so a second submit on a busy branch is refused immediately
    /// and the caller decides whether to fork a branch or wait.
    #[error("branch {branch} is busy: one turn at a time per branch")]
    BranchBusy {
        /// The branch that is already leased.
        branch: BranchId,
    },

    /// The branch has been closed by [`close_branch`](crate::SessionStore::close_branch).
    #[error("branch {branch} is closed")]
    BranchClosed {
        /// The closed branch.
        branch: BranchId,
    },

    /// No such session.
    #[error("no such session: {session}")]
    NoSuchSession {
        /// The session that was asked for.
        session: SessionId,
    },

    /// No such branch.
    #[error("no such branch: {branch}")]
    NoSuchBranch {
        /// The branch that was asked for.
        branch: BranchId,
    },

    /// No such turn.
    #[error("no such turn: {turn}")]
    NoSuchTurn {
        /// The turn that was asked for.
        turn: TurnId,
    },

    /// An append arrived with a sequence number that is not the next one.
    #[error("out of order on branch {branch}: expected seq {expected}, got {got}")]
    OutOfOrder {
        /// The branch.
        branch: BranchId,
        /// What the branch was waiting for.
        expected: Seq,
        /// What arrived.
        got: Seq,
    },

    /// The store's own invariants are broken: a duplicate `(branch, seq)`, a
    /// payload that will not deserialise, a dangling foreign key.
    ///
    /// Surfaced as a value rather than a panic, because a corrupt session is
    /// something a person has to be told about, not a crash in a background
    /// task.
    #[error("session store is corrupt: {detail}")]
    Corrupt {
        /// What was wrong.
        detail: String,
    },

    /// The backend itself failed: the disk, the connection, the writer task.
    #[error("session store backend failed: {detail}")]
    Backend {
        /// What the backend said.
        detail: String,
    },
}

impl SessionError {
    /// A backend failure from anything that renders.
    pub fn backend(detail: impl std::fmt::Display) -> Self {
        Self::Backend {
            detail: detail.to_string(),
        }
    }

    /// A corruption report from anything that renders.
    pub fn corrupt(detail: impl std::fmt::Display) -> Self {
        Self::Corrupt {
            detail: detail.to_string(),
        }
    }
}
