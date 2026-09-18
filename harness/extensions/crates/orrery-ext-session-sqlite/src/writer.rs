//! The writer actor: one task per session, owning the write connection.
//!
//! SQLite takes one writer at a time. Rather than discover that as
//! `SQLITE_BUSY` under load, the write path is serialised on purpose: every
//! mutation becomes a [`WriteOp`] on an `mpsc` channel, the actor applies it in
//! **one transaction**, and answers on a `oneshot`.
//!
//! The actor runs on a dedicated OS thread, not a Tokio task. `rusqlite` blocks,
//! and a blocking commit on a runtime worker starves everything sharing that
//! worker — including the very `spawn_blocking` reads this design relies on.
//!
//! **The actor never dies on a failed op.** An error is a value sent back down
//! the `oneshot`; the loop goes round again. A single bad append must not take
//! the session's history offline, because a session store that stops answering
//! ends the session (§4.7).

use std::path::PathBuf;

use orrery_proto::{BranchId, Event, Seq, SessionId, TurnId, Usage};
use orrery_session::SessionError;
use orrery_session::turn::TurnKind;
use rusqlite::{Connection, params};
use tokio::sync::{mpsc, oneshot};

use crate::convert::{self, map_err};

/// One mutation.
///
/// Every variant carries its own `reply`, so a caller learns what happened to
/// *its* op and not merely that the actor is alive.
#[non_exhaustive]
pub enum WriteOp {
    /// Create a session and its root branch.
    CreateSession {
        /// The new session.
        session: SessionId,
        /// Its root branch.
        root: BranchId,
        /// The workspace root.
        workspace: String,
        /// The profile it was built from.
        profile: String,
        /// Where to answer.
        reply: oneshot::Sender<Result<SessionId, SessionError>>,
    },
    /// Write one turn, and the event that announces it.
    Append {
        /// Which session's event log to extend.
        session: SessionId,
        /// Which branch.
        branch: BranchId,
        /// Which sequence number — allocated by the lease, not by the actor.
        seq: Seq,
        /// The turn's id.
        id: TurnId,
        /// What the turn is.
        kind: TurnKind,
        /// Where to answer.
        reply: oneshot::Sender<Result<TurnId, SessionError>>,
    },
    /// Fork a branch.
    Branch {
        /// The new branch.
        child: BranchId,
        /// Its session.
        session: SessionId,
        /// Its parent.
        parent: BranchId,
        /// The turn it forks at.
        forked_at: TurnId,
        /// What to call it.
        label: String,
        /// Where to answer.
        reply: oneshot::Sender<Result<BranchId, SessionError>>,
    },
    /// Close a branch. Touches the **child's** row and nothing else — see the
    /// invariant at the top of `orrery_session::lease`.
    CloseBranch {
        /// The branch to close.
        branch: BranchId,
        /// How it went, as the `branches.state` discriminant.
        state: String,
        /// Where to answer.
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    /// Write a summary turn and the watermark that points at it, together.
    Compact {
        /// Which session's event log to extend.
        session: SessionId,
        /// Which branch.
        branch: BranchId,
        /// The summary turn's sequence number.
        seq: Seq,
        /// The summary turn's id.
        id: TurnId,
        /// The summary turn.
        kind: TurnKind,
        /// The watermark.
        upto: Seq,
        /// Where to answer.
        reply: oneshot::Sender<Result<u64, SessionError>>,
    },
}

/// A handle onto one session's writer.
#[derive(Clone, Debug)]
pub struct Writer {
    tx: mpsc::UnboundedSender<WriteOp>,
}

impl Writer {
    /// Start the actor on its own thread.
    ///
    /// # Errors
    ///
    /// [`SessionError::Backend`] when the database will not open.
    pub fn spawn(path: PathBuf) -> Result<Self, SessionError> {
        let conn = crate::schema::open_write(&path).map_err(map_err)?;
        let (tx, rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("orrery-session-writer".into())
            .spawn(move || run(conn, rx))
            .map_err(SessionError::backend)?;
        Ok(Self { tx })
    }

    /// Hand the actor an op and wait for its answer.
    ///
    /// # Errors
    ///
    /// [`SessionError::Backend`] when the actor is gone, which only happens at
    /// shutdown.
    pub fn send(&self, op: WriteOp) -> Result<(), SessionError> {
        self.tx.send(op).map_err(|_| SessionError::Backend {
            detail: "the session writer has stopped".into(),
        })
    }
}

/// Await a reply, turning a dropped sender into a backend error rather than a
/// panic.
///
/// # Errors
///
/// Whatever the op failed with, or [`SessionError::Backend`] when the actor
/// dropped the reply.
pub async fn wait<T>(rx: oneshot::Receiver<Result<T, SessionError>>) -> Result<T, SessionError> {
    rx.await.map_err(|_| SessionError::Backend {
        detail: "the session writer dropped a reply".into(),
    })?
}

fn run(mut conn: Connection, mut rx: mpsc::UnboundedReceiver<WriteOp>) {
    while let Some(op) = rx.blocking_recv() {
        // Every arm answers. A failure is a value, never the end of the loop.
        match op {
            WriteOp::CreateSession {
                session,
                root,
                workspace,
                profile,
                reply,
            } => {
                let out = create_session(&mut conn, session, root, &workspace, &profile)
                    .map(|()| session);
                let _ = reply.send(out);
            }
            WriteOp::Append {
                session,
                branch,
                seq,
                id,
                kind,
                reply,
            } => {
                let out = append(&mut conn, session, branch, seq, id, &kind).map(|()| id);
                let _ = reply.send(out);
            }
            WriteOp::Branch {
                child,
                session,
                parent,
                forked_at,
                label,
                reply,
            } => {
                let out =
                    branch(&mut conn, child, session, parent, forked_at, &label).map(|()| child);
                let _ = reply.send(out);
            }
            WriteOp::CloseBranch {
                branch,
                state,
                reply,
            } => {
                let _ = reply.send(close_branch(&mut conn, branch, &state));
            }
            WriteOp::Compact {
                session,
                branch,
                seq,
                id,
                kind,
                upto,
                reply,
            } => {
                let _ = reply.send(compact(&mut conn, session, branch, seq, id, &kind, upto));
            }
        }
    }
    tracing::debug!("session writer stopped: every handle was dropped");
}

fn create_session(
    conn: &mut Connection,
    session: SessionId,
    root: BranchId,
    workspace: &str,
    profile: &str,
) -> Result<(), SessionError> {
    let tx = conn.transaction().map_err(map_err)?;
    tx.execute(
        "INSERT INTO sessions (id, workspace, profile, root_branch, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            convert::id_str(&session),
            workspace,
            profile,
            convert::id_str(&root),
            convert::now_ms()
        ],
    )
    .map_err(map_err)?;
    tx.execute(
        "INSERT INTO branches (id, session, parent_branch, forked_at_turn, label, state) \
         VALUES (?1, ?2, NULL, NULL, 'main', 'open')",
        params![convert::id_str(&root), convert::id_str(&session)],
    )
    .map_err(map_err)?;
    tx.commit().map_err(map_err)
}

/// One turn, one transaction. The turn row and the event that announces it land
/// together or not at all, so a replay can never see an event for a turn that
/// is not there.
fn append(
    conn: &mut Connection,
    session: SessionId,
    branch: BranchId,
    seq: Seq,
    id: TurnId,
    kind: &TurnKind,
) -> Result<(), SessionError> {
    let payload = convert::encode_kind(kind)?;
    let usage = kind.usage();
    let tx = conn.transaction().map_err(map_err)?;
    insert_turn(&tx, branch, seq, id, kind.tag(), &payload)?;
    insert_event(&tx, session, id, usage)?;
    tx.commit().map_err(map_err)
}

fn insert_turn(
    tx: &rusqlite::Transaction<'_>,
    branch: BranchId,
    seq: Seq,
    id: TurnId,
    tag: &str,
    payload: &[u8],
) -> Result<(), SessionError> {
    tx.execute(
        "INSERT INTO turns (id, branch, seq, kind, payload, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            convert::id_str(&id),
            convert::id_str(&branch),
            seq.0 as i64,
            tag,
            payload,
            convert::now_ms()
        ],
    )
    .map_err(map_err)?;
    Ok(())
}

/// The session-wide event sequence is allocated **inside** the transaction, so
/// two branches appending at once cannot take the same number and leave a gap
/// a client would read as a lost frame.
fn insert_event(
    tx: &rusqlite::Transaction<'_>,
    session: SessionId,
    turn: TurnId,
    usage: Usage,
) -> Result<(), SessionError> {
    let id = convert::id_str(&session);
    let next: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE session = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(map_err)?;
    let event = Event::TurnSettled {
        seq: Seq(next as u64),
        turn,
        usage,
    };
    tx.execute(
        "INSERT INTO events (session, seq, payload) VALUES (?1, ?2, ?3)",
        params![id, next, convert::encode_event(&event)?],
    )
    .map_err(map_err)?;
    Ok(())
}

fn branch(
    conn: &mut Connection,
    child: BranchId,
    session: SessionId,
    parent: BranchId,
    forked_at: TurnId,
    label: &str,
) -> Result<(), SessionError> {
    conn.execute(
        "INSERT INTO branches (id, session, parent_branch, forked_at_turn, label, state) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'open')",
        params![
            convert::id_str(&child),
            convert::id_str(&session),
            convert::id_str(&parent),
            convert::id_str(&forked_at),
            label
        ],
    )
    .map_err(map_err)?;
    Ok(())
}

/// Only the child's row. Nothing here touches the parent — that is the whole
/// point.
fn close_branch(conn: &mut Connection, branch: BranchId, state: &str) -> Result<(), SessionError> {
    let n = conn
        .execute(
            "UPDATE branches SET state = ?2 WHERE id = ?1",
            params![convert::id_str(&branch), state],
        )
        .map_err(map_err)?;
    if n == 0 {
        return Err(SessionError::NoSuchBranch { branch });
    }
    Ok(())
}

/// The summary turn and the watermark, in one transaction. Nothing is deleted
/// and nothing is updated: `compact` writes.
fn compact(
    conn: &mut Connection,
    session: SessionId,
    branch: BranchId,
    seq: Seq,
    id: TurnId,
    kind: &TurnKind,
    upto: Seq,
) -> Result<u64, SessionError> {
    let payload = convert::encode_kind(kind)?;
    let usage = kind.usage();
    let tx = conn.transaction().map_err(map_err)?;
    insert_turn(&tx, branch, seq, id, kind.tag(), &payload)?;
    insert_event(&tx, session, id, usage)?;
    tx.execute(
        "INSERT INTO compactions (branch, upto_seq, summary_turn) VALUES (?1, ?2, ?3) \
         ON CONFLICT(branch, upto_seq) DO UPDATE SET summary_turn = excluded.summary_turn",
        params![
            convert::id_str(&branch),
            upto.0 as i64,
            convert::id_str(&id)
        ],
    )
    .map_err(map_err)?;
    let covered: i64 = tx
        .query_row(
            "SELECT count(*) FROM turns WHERE branch = ?1 AND seq <= ?2",
            params![convert::id_str(&branch), upto.0 as i64],
            |r| r.get(0),
        )
        .map_err(map_err)?;
    tx.commit().map_err(map_err)?;
    Ok(covered as u64)
}
