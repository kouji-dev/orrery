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
    /// The same file in the form every connection is opened with: see
    /// [`extended`]. Kept beside `path` rather than replacing it, because a
    /// per-session writer is spawned long after `open` returned and must not
    /// be the one connection that goes back to the short form.
    opened: PathBuf,
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
        // SQLite is handed the extended-length form, never the plain one: see
        // `extended`. `self.path` keeps what the caller asked for, because that
        // is the name a person recognises in an error.
        let opened = extended(&path);
        // Spawning the control writer applies the schema, which is what makes
        // the read-only connection below openable at all.
        let control = Writer::spawn(opened.clone())?;
        let reader = Reader::open(opened.clone())?;
        Ok(Self {
            path,
            opened,
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
        let writer = Writer::spawn(self.opened.clone())?;
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

/// The form SQLite is given a path in.
///
/// Windows' Win32 layer, which SQLite's default VFS calls, refuses a path
/// longer than `MAX_PATH` (260 characters) unless it arrives in the
/// extended-length `\\?\` form. The eval runner is where this bit: at a
/// 244-character workspace the state file reaches 263 characters, and
/// `orrery eval run` came back with "unable to open database file" - a
/// perfectly honest filesystem error about a database that was fine.
///
/// The conversion is the one Windows documents: fully qualify the path first
/// (the extended form takes no `.`, `..` or forward slashes), then prefix
/// `\\?\`, or `\\?\UNC\` for a UNC share. Off Windows, and for a path
/// that already carries the prefix, this is the identity.
///
/// `dunce` is the other half of the same story and deliberately not used here:
/// it *removes* the prefix for paths that do not need it, which is right for
/// comparing paths and exactly wrong for opening a long one.
#[must_use]
fn extended(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let raw = path.as_os_str().to_string_lossy().into_owned();
        if raw.starts_with(r"\\?\") || raw.starts_with(r"\\.\") {
            return path.to_path_buf();
        }
        if let Ok(absolute) = std::path::absolute(path) {
            let text = absolute.as_os_str().to_string_lossy().into_owned();
            if let Some(share) = text.strip_prefix(r"\\") {
                return PathBuf::from(format!(r"\\?\UNC\{share}"));
            }
            return PathBuf::from(format!(r"\\?\{text}"));
        }
    }
    path.to_path_buf()
}

/// Build the `session` singleton.
///
/// The direct constructor, for an embedder that has already decided it wants
/// SQLite. The loader half is [`SqliteSessions`]: registering that with
/// `orrery_host::NativeRegistry` is what puts this extension's `orrery.toml`
/// through the same parser a third-party manifest goes through, so the ledger
/// shows the `session` slot has a holder, a deny rule can name `sqlite`, and
/// `orrery ext test` can run it. Both halves are needed — the same split
/// `orrery-ext-views-default` has, because neither a session store nor a view
/// is a tool the model can call.
///
/// # Errors
///
/// [`SessionError::Backend`] when the database will not open.
pub fn build(state_dir: &Path) -> Result<std::sync::Arc<dyn SessionStore>, SessionError> {
    // The directory is made through the extended form too: `create_dir_all`
    // hits the same ceiling one segment earlier than the file does.
    std::fs::create_dir_all(extended(state_dir)).map_err(SessionError::backend)?;
    Ok(std::sync::Arc::new(SqliteSessionStore::open(
        state_dir.join("sessions.db"),
    )?))
}


/// The loader's half of this extension.
///
/// A session store is not a tool, so this contributes none. What it does is
/// make the crate an *extension* rather than a library the harness happens to
/// call: the host parses the manifest below with the same parser it holds a
/// third party to, records the `session` singleton against `sqlite`, and gives
/// policy a name to deny. The store itself comes from [`build`].
#[derive(Copy, Clone, Debug, Default)]
pub struct SqliteSessions;

/// The shipped manifest, compiled in so the binary and the source cannot drift.
pub const MANIFEST: &str = include_str!("../orrery.toml");

#[async_trait::async_trait]
impl orrery_ext_api::NativeExtension for SqliteSessions {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn manifest_path(&self) -> &str {
        "harness/extensions/crates/orrery-ext-session-sqlite/orrery.toml"
    }

    /// None. A session store is reached through the `session` singleton slot,
    /// never offered to the model and never called with an input.
    fn tools(&self) -> Vec<orrery_ext_api::ToolDef> {
        Vec::new()
    }

    async fn call(
        &self,
        tool: &str,
        _input: serde_json::Value,
        _ctx: &orrery_ext_api::CallCtx,
    ) -> Result<orrery_proto::Outcome, orrery_ext_api::HostError> {
        Err(orrery_ext_api::HostError::NoSuchTool {
            ext: orrery_proto::ExtId::new("sqlite").expect("a literal ext id"),
            tool: tool.to_owned(),
        })
    }
}
