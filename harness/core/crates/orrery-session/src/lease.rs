//! One turn at a time per branch — and **the parent performs the join**.
//!
//! # The invariant (Task 8, and the one deadlock in the design)
//!
//! A child branch **never appends to its parent.**
//!
//! A sub-agent runs on a child branch while its parent still holds the parent
//! branch's lease — the parent is mid-turn; that is why it spawned a child at
//! all. If closing the child wrote the join row onto the parent, closing would
//! have to take the parent's lease, which the parent is holding and will not
//! release until the child returns. That is a deadlock, and it is not a rare
//! interleaving: it is *every* sub-agent call.
//!
//! So the shape is:
//!
//! - [`SessionStore::branch`](crate::SessionStore::branch) takes **no lease**.
//!   Forking is a read of the parent, not a write to it.
//! - [`SessionStore::close_branch`](crate::SessionStore::close_branch)
//!   **consumes** the child's lease and writes only the child's own rows. It
//!   returns; it does not notify.
//! - The parent, still holding its own lease, appends the
//!   [`BranchResult`](crate::turn::TurnKind::BranchResult) itself.
//!
//! The lease type enforces the other half of this: [`BranchLease`] has private
//! fields, no `Clone`, no `Default` and no public constructor, so the only way
//! to get one is [`SessionStore::lease`](crate::SessionStore::lease). An
//! `append` that skipped the lock cannot be written, not merely discouraged.
//!
//! ```compile_fail
//! # use orrery_session::BranchLease;
//! # use orrery_proto::BranchId;
//! // There is no public constructor, and the fields are private.
//! let forged = BranchLease { branch: BranchId::new() };
//! ```
//!
//! ```compile_fail
//! # use orrery_session::BranchLease;
//! fn clone_it(lease: &BranchLease) -> BranchLease {
//!     lease.clone() // `BranchLease` is deliberately not `Clone`.
//! }
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use dashmap::DashMap;
use orrery_proto::{BranchId, Seq};
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::error::SessionError;

/// Whether a branch may still be appended to.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BranchStatus {
    /// Accepting turns.
    Open,
    /// Closed by [`close_branch`](crate::SessionStore::close_branch).
    Closed,
}

/// What the registry holds behind the lock: the branch's next sequence number
/// and whether it is still open.
///
/// The fields are atomics rather than plain values because
/// [`SessionStore::append`](crate::SessionStore::append) takes `&BranchLease`,
/// not `&mut` — the exclusion is already provided by the owned guard, so the
/// atomics only need to satisfy the borrow checker, not to synchronise.
#[derive(Debug)]
pub struct BranchState {
    next_seq: AtomicU64,
    closed: AtomicBool,
}

impl BranchState {
    /// A fresh, open branch whose first turn will be `Seq(1)`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_seq: AtomicU64::new(1),
            closed: AtomicBool::new(false),
        }
    }

    /// A branch reopened from storage, whose next turn is `next_seq`.
    #[must_use]
    pub fn resumed(next_seq: Seq, status: BranchStatus) -> Self {
        Self {
            next_seq: AtomicU64::new(next_seq.0),
            closed: AtomicBool::new(status == BranchStatus::Closed),
        }
    }
}

impl Default for BranchState {
    fn default() -> Self {
        Self::new()
    }
}

/// The right to append to one branch.
///
/// No `Clone`, no `Default`, no public constructor and no public fields. It is
/// released when it is dropped.
#[derive(Debug)]
pub struct BranchLease {
    // Private. This is the whole point of the type.
    guard: OwnedMutexGuard<BranchState>,
    branch: BranchId,
}

impl BranchLease {
    /// Which branch this lease is for.
    #[must_use]
    pub fn branch(&self) -> BranchId {
        self.branch
    }

    /// The sequence number the next append will take. Peeking does not
    /// consume it: a failed write must not burn a number and leave a gap.
    #[must_use]
    pub fn next_seq(&self) -> Seq {
        Seq(self.guard.next_seq.load(Ordering::Relaxed))
    }

    /// Record that `seq` was durably written. Called by a backend **after** the
    /// transaction commits, never before, so a failed op leaves the branch
    /// exactly where it was.
    pub fn commit_seq(&self, seq: Seq) {
        self.guard.next_seq.store(seq.0 + 1, Ordering::Relaxed);
    }

    /// Whether the branch is still open.
    #[must_use]
    pub fn status(&self) -> BranchStatus {
        if self.guard.closed.load(Ordering::Relaxed) {
            BranchStatus::Closed
        } else {
            BranchStatus::Open
        }
    }

    /// Refuse an append to a closed branch.
    ///
    /// # Errors
    ///
    /// [`SessionError::BranchClosed`] when the branch has been closed.
    pub fn ensure_open(&self) -> Result<(), SessionError> {
        match self.status() {
            BranchStatus::Open => Ok(()),
            BranchStatus::Closed => Err(SessionError::BranchClosed {
                branch: self.branch,
            }),
        }
    }

    /// Mark the branch closed. Called by
    /// [`close_branch`](crate::SessionStore::close_branch), which consumes the
    /// lease straight afterwards.
    pub fn mark_closed(&self) {
        self.guard.closed.store(true, Ordering::Relaxed);
    }
}

/// Who hands out leases.
///
/// Lives in the **trait** crate, not in a backend, so that every backend
/// inherits the one-turn-per-branch rule rather than reimplementing it — and so
/// that two backends cannot disagree about what "busy" means.
#[derive(Debug, Default)]
pub struct LeaseRegistry {
    branches: DashMap<BranchId, Arc<Mutex<BranchState>>>,
}

impl LeaseRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Make a branch leasable, if it is not already known.
    ///
    /// Idempotent: reopening a session registers every branch it finds, and a
    /// branch already in the map keeps the state it has, because that state may
    /// be leased right now.
    pub fn register(&self, branch: BranchId, next_seq: Seq, status: BranchStatus) {
        self.branches
            .entry(branch)
            .or_insert_with(|| Arc::new(Mutex::new(BranchState::resumed(next_seq, status))));
    }

    /// Whether this branch has been registered.
    #[must_use]
    pub fn knows(&self, branch: BranchId) -> bool {
        self.branches.contains_key(&branch)
    }

    /// Take the lease, or refuse.
    ///
    /// # Errors
    ///
    /// [`SessionError::NoSuchBranch`] when the branch was never registered, and
    /// [`SessionError::BranchBusy`] when somebody else is holding it. Never
    /// blocks.
    pub fn lease(&self, branch: BranchId) -> Result<BranchLease, SessionError> {
        let state = self
            .branches
            .get(&branch)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or(SessionError::NoSuchBranch { branch })?;

        // try_lock, never lock. A busy branch is a refusal, not a queue.
        let guard = state
            .try_lock_owned()
            .map_err(|_| SessionError::BranchBusy { branch })?;

        Ok(BranchLease { guard, branch })
    }

    /// Forget a branch. Only for a session being dropped from memory; the rows
    /// outlive the registry.
    pub fn forget(&self, branch: BranchId) {
        self.branches.remove(&branch);
    }
}
