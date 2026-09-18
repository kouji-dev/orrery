//! The default session backend: SQLite in WAL mode, one transaction per turn append, a writer actor per session.
//!
//! Implementation plan: `harness/docs/plans/02-session-store.md`
//!
//! # The shape
//!
//! - **One writer actor per session**, each on its own OS thread with its own
//!   write connection ([`writer`]). Writes are serialised on purpose rather
//!   than discovered as `SQLITE_BUSY` under load, and every mutation is one
//!   transaction.
//! - **A second, read-only connection** for everything else ([`read`]), driven
//!   under `spawn_blocking`. WAL is what lets the two run at once.
//! - **The lease registry lives in `orrery-session`**, not here. This crate
//!   allocates no sequence numbers of its own: it takes them from the
//!   [`BranchLease`](orrery_session::BranchLease) and only commits the branch's
//!   counter once the transaction has.
//! - **`close_branch` writes the child's row and nothing else.** The parent
//!   appends its own `BranchResult` under its own lease. See the invariant at
//!   the top of `orrery_session::lease`; that shape is the only one that does
//!   not deadlock every sub-agent call.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod convert;
pub mod read;
pub mod schema;
pub mod writer;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use dashmap::DashMap;
use orrery_proto::{BranchId, Seq, SessionId, TokenBudget, TurnId};
use orrery_session::algebra::{self, Materialised, TokenCounter};
use orrery_session::lease::{BranchLease, BranchStatus, LeaseRegistry};
use orrery_session::turn::{
    BranchOutcome, CompactResult, NewTurn, SessionHandle, SessionSummary, StoredEvent, TurnRow,
};
use orrery_session::{SessionError, SessionStore};
use tokio::sync::oneshot;

use crate::read::Reader;
use crate::writer::{Writer, wait};

/// The `session` singleton, on SQLite.
pub struct SqliteSessionStore {
    path: PathBuf,
    reader: Reader,
    leases: LeaseRegistry,
    /// One actor per session, spawned on first write.
    writers: DashMap<SessionId, Writer>,
    /// The actor that creates sessions, before any per-session actor exists.
    control: Writer,
    /// Which session a branch belongs to. A branch's session never changes, so
    /// this is a cache that can never be stale — only absent.
    branch_session: DashMap<BranchId, SessionId>,
}

impl SqliteSessionStore {
    /// Open, or create, a session database.
    ///
    /// Applies the schema and sets the pragmas before returning, so a caller
    /// that gets a store back has a usable one.
    ///
    /// # Errors
    ///
    /// [`SessionError::Backend`] when SQLite will not open the file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let path = path.as_ref().to_path_buf();
        // Spawning the control writer applies the schema, which is what makes
        // the read-only connection below openable at all.
        let control = Writer::spawn(path.clone())?;
        let reader = Reader::open(path.clone())?;
        Ok(Self {
            path,
            reader,
            leases: LeaseRegistry::new(),
            writers: DashMap::new(),
            control,
            branch_session: DashMap::new(),
        })
    }

    /// Where the database is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The read half, for a backend test that wants to look at the rows
    /// directly — `compact::original_rows_survive` does exactly that.
    #[must_use]
    pub fn reader(&self) -> &Reader {
        &self.reader
    }

    /// This session's writer, spawned on first use.
    ///
    /// Public so a test can hand the actor a deliberately bad op and prove the
    /// actor survives it — `concurrency::writer_survives_a_failed_op` does
    /// exactly that, and there is no other way to make a well-formed store
    /// produce a broken write.
    ///
    /// # Errors
    ///
    /// [`SessionError::Backend`] when a new writer will not start.
    pub fn writer_for(&self, session: SessionId) -> Result<Writer, SessionError> {
        if let Some(w) = self.writers.get(&session) {
            return Ok(w.clone());
        }
        let writer = Writer::spawn(self.path.clone())?;
        Ok(self
            .writers
            .entry(session)
            .or_insert(writer)
            .value()
            .clone())
    }

    /// Which session a branch belongs to, from the cache or from the database.
    async fn session_of(&self, branch: BranchId) -> Result<SessionId, SessionError> {
        if let Some(s) = self.branch_session.get(&branch) {
            return Ok(*s);
        }
        let row = self.reader.branch(branch).await?;
        self.branch_session.insert(branch, row.session);
        Ok(row.session)
    }

    /// Make a branch leasable, reading its real next sequence number out of the
    /// database. Idempotent, and a no-op for a branch already in the registry —
    /// which matters, because that branch may be leased right now.
    async fn ensure_registered(&self, branch: BranchId) -> Result<(), SessionError> {
        if self.leases.knows(branch) {
            return Ok(());
        }
        let row = self.reader.branch(branch).await?;
        let next = self.reader.next_seq(branch).await?;
        self.branch_session.insert(branch, row.session);
        self.leases.register(branch, next, row.status);
        Ok(())
    }
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create(&self, workspace: &str, profile: &str) -> Result<SessionId, SessionError> {
        let session = SessionId::new();
        let root = BranchId::new();
        let (reply, rx) = oneshot::channel();
        self.control.send(writer::WriteOp::CreateSession {
            session,
            root,
            workspace: workspace.to_owned(),
            profile: profile.to_owned(),
            reply,
        })?;
        wait(rx).await?;

        self.branch_session.insert(root, session);
        self.leases.register(root, Seq(1), BranchStatus::Open);
        Ok(session)
    }

    async fn open(&self, session: SessionId) -> Result<SessionHandle, SessionError> {
        let handle = self.reader.session(session).await?;
        for branch in &handle.branches {
            self.ensure_registered(*branch).await?;
        }
        Ok(handle)
    }

    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError> {
        self.reader.sessions().await
    }

    async fn lease(&self, branch: BranchId) -> Result<BranchLease, SessionError> {
        self.ensure_registered(branch).await?;
        self.leases.lease(branch)
    }

    /// Drop the session's rows, then drop the in-memory bookkeeping that
    /// pointed at them.
    ///
    /// Order matters and is deliberate: the branches are read **before** the
    /// delete, because afterwards there is no row left to ask. The writer actor
    /// for this session is dropped too — keeping it would leave a thread and an
    /// open connection belonging to a session that no longer exists.
    async fn delete(&self, session: SessionId) -> Result<(), SessionError> {
        let branches = self.reader.session(session).await?.branches;

        let (reply, rx) = oneshot::channel();
        self.writer_for(session)?
            .send(writer::WriteOp::DeleteSession { session, reply })?;
        wait(rx).await?;

        for branch in branches {
            self.leases.forget(branch);
            self.branch_session.remove(&branch);
        }
        self.writers.remove(&session);
        Ok(())
    }

    async fn append(&self, lease: &BranchLease, turn: NewTurn) -> Result<TurnId, SessionError> {
        lease.ensure_open()?;
        let branch = lease.branch();
        let session = self.session_of(branch).await?;
        let seq = lease.next_seq();
        let id = TurnId::new();

        let (reply, rx) = oneshot::channel();
        self.writer_for(session)?.send(writer::WriteOp::Append {
            session,
            branch,
            seq,
            id,
            kind: turn.kind,
            reply,
        })?;
        let id = wait(rx).await?;

        // Only now. A failed write must leave the branch exactly where it was,
        // rather than burning a sequence number and opening a gap.
        lease.commit_seq(seq);
        Ok(id)
    }

    async fn branch(&self, from: TurnId, label: &str) -> Result<BranchId, SessionError> {
        let parent = self.reader.turn_branch(from).await?;
        let session = self.session_of(parent).await?;
        let child = BranchId::new();

        let (reply, rx) = oneshot::channel();
        self.writer_for(session)?.send(writer::WriteOp::Branch {
            child,
            session,
            parent,
            forked_at: from,
            label: label.to_owned(),
            reply,
        })?;
        wait(rx).await?;

        self.branch_session.insert(child, session);
        self.leases.register(child, Seq(1), BranchStatus::Open);
        Ok(child)
    }

    async fn close_branch(
        &self,
        lease: BranchLease,
        outcome: BranchOutcome,
    ) -> Result<(), SessionError> {
        let branch = lease.branch();
        let session = self.session_of(branch).await?;

        let (reply, rx) = oneshot::channel();
        self.writer_for(session)?
            .send(writer::WriteOp::CloseBranch {
                branch,
                state: outcome.tag().to_owned(),
                reply,
            })?;
        wait(rx).await?;

        lease.mark_closed();
        // Dropping the lease releases the CHILD's branch, and only the child's.
        // Nothing here reaches for the parent: the parent is mid-turn holding
        // its own lease, and waiting on it would deadlock every sub-agent call.
        drop(lease);
        Ok(())
    }

    async fn materialise(
        &self,
        branch: BranchId,
        budget: TokenBudget,
        counter: &dyn TokenCounter,
    ) -> Result<Materialised, SessionError> {
        let (rows, watermark) = self.reader.ancestry(branch).await?;
        Ok(algebra::materialise(&rows, watermark, budget, counter))
    }

    async fn compact(
        &self,
        lease: &BranchLease,
        upto: Seq,
        summary: NewTurn,
    ) -> Result<CompactResult, SessionError> {
        lease.ensure_open()?;
        let branch = lease.branch();
        let session = self.session_of(branch).await?;
        let seq = lease.next_seq();
        let id = TurnId::new();

        let (reply, rx) = oneshot::channel();
        self.writer_for(session)?.send(writer::WriteOp::Compact {
            session,
            branch,
            seq,
            id,
            kind: summary.kind,
            upto,
            reply,
        })?;
        let covered = wait(rx).await?;
        lease.commit_seq(seq);

        Ok(CompactResult {
            summary: id,
            watermark: upto,
            covered,
        })
    }

    async fn turns(&self, branch: BranchId) -> Result<Vec<TurnRow>, SessionError> {
        self.reader.turns_on(branch).await
    }

    async fn events_since(
        &self,
        session: SessionId,
        since: Option<Seq>,
    ) -> Result<Vec<StoredEvent>, SessionError> {
        self.reader.events_since(session, since).await
    }
}

/// Build the `session` singleton.
///
/// TODO(plan-06): this is the direct constructor. Once the extension loader
/// exists, `orrery-host` reads [`orrery.toml`](../orrery.toml) and calls this
/// through the same path a third-party extension takes — the ledger shows it, a
/// deny rule disables it, `orrery ext test` runs it.
///
/// # Errors
///
/// [`SessionError::Backend`] when the database will not open.
pub fn build(state_dir: &Path) -> Result<std::sync::Arc<dyn SessionStore>, SessionError> {
    std::fs::create_dir_all(state_dir).map_err(SessionError::backend)?;
    Ok(std::sync::Arc::new(SqliteSessionStore::open(
        state_dir.join("sessions.db"),
    )?))
}
