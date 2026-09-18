//! The read path: a second, read-only connection under `spawn_blocking`.
//!
//! WAL is what makes this work — a reader never blocks the writer and the
//! writer never blocks a reader, so `materialise` on a long branch does not
//! stall the turn that is appending to it.
//!
//! Every query here is synchronous and takes a `&Connection`. The async wrapper
//! is a thin shell over `spawn_blocking`, so the interesting half is testable
//! without a runtime.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use orrery_proto::{BranchId, Seq, SessionId, TurnId};
use orrery_session::SessionError;
use orrery_session::lease::BranchStatus;
use orrery_session::turn::{SessionHandle, StoredEvent, TurnRow};
use rusqlite::{Connection, OptionalExtension, params};

use crate::convert::{self, map_err};

/// What the `branches` table knows about one branch.
#[derive(Clone, Debug)]
pub struct BranchRow {
    /// Which session it belongs to.
    pub session: SessionId,
    /// Its parent, if it is not the root.
    pub parent: Option<BranchId>,
    /// The turn it forked at, if it is not the root.
    pub forked_at: Option<TurnId>,
    /// Whether it still takes turns.
    pub status: BranchStatus,
}

/// The read-only half of the store.
#[derive(Clone)]
pub struct Reader {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl Reader {
    /// Open the read connection.
    ///
    /// # Errors
    ///
    /// [`SessionError::Backend`] when SQLite will not open the file.
    pub fn open(path: PathBuf) -> Result<Self, SessionError> {
        let conn = crate::schema::open_read(&path).map_err(map_err)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path,
        })
    }

    /// Where the database is.
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Run a query off the runtime's worker threads.
    async fn with<T, F>(&self, f: F) -> Result<T, SessionError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, SessionError> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().map_err(|_| SessionError::Backend {
                detail: "the read connection is poisoned".into(),
            })?;
            f(&guard)
        })
        .await
        .map_err(SessionError::backend)?
    }

    /// One session, with every branch in it.
    pub async fn session(&self, session: SessionId) -> Result<SessionHandle, SessionError> {
        self.with(move |c| session_blocking(c, session)).await
    }

    /// One branch.
    pub async fn branch(&self, branch: BranchId) -> Result<BranchRow, SessionError> {
        self.with(move |c| branch_blocking(c, branch)).await
    }

    /// The sequence number the branch's next turn will take.
    pub async fn next_seq(&self, branch: BranchId) -> Result<Seq, SessionError> {
        self.with(move |c| next_seq_blocking(c, branch)).await
    }

    /// Which branch a turn sits on.
    pub async fn turn_branch(&self, turn: TurnId) -> Result<BranchId, SessionError> {
        self.with(move |c| turn_branch_blocking(c, turn)).await
    }

    /// A branch's whole ancestry, oldest first, and the highest watermark over
    /// it. One call, because `materialise` needs both and two round trips could
    /// straddle a compaction.
    pub async fn ancestry(
        &self,
        branch: BranchId,
    ) -> Result<(Vec<TurnRow>, Option<Seq>), SessionError> {
        self.with(move |c| {
            let rows = ancestry_blocking(c, branch)?;
            let mark = watermark_blocking(c, branch)?;
            Ok((rows, mark))
        })
        .await
    }

    /// The events for a session, `since` exclusive.
    pub async fn events_since(
        &self,
        session: SessionId,
        since: Option<Seq>,
    ) -> Result<Vec<StoredEvent>, SessionError> {
        self.with(move |c| events_since_blocking(c, session, since))
            .await
    }

    /// Every turn on one branch, ignoring ancestry and ignoring watermarks.
    /// This is what a compaction test reads to prove the originals survived.
    pub async fn turns_on(&self, branch: BranchId) -> Result<Vec<TurnRow>, SessionError> {
        self.with(move |c| turns_on_blocking(c, branch)).await
    }
}

/// See [`Reader::session`].
pub fn session_blocking(c: &Connection, session: SessionId) -> Result<SessionHandle, SessionError> {
    let id = convert::id_str(&session);
    let (workspace, profile, root): (String, String, String) = c
        .query_row(
            "SELECT workspace, profile, root_branch FROM sessions WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(map_err)?
        .ok_or(SessionError::NoSuchSession { session })?;

    let mut stmt = c
        .prepare("SELECT id FROM branches WHERE session = ?1 ORDER BY rowid")
        .map_err(map_err)?;
    let branches = stmt
        .query_map(params![id], |r| r.get::<_, String>(0))
        .map_err(map_err)?
        .map(|row| {
            row.map_err(map_err)
                .and_then(|raw| convert::parse_id::<BranchId>(&raw, "branches.id"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SessionHandle {
        session,
        root: convert::parse_id(&root, "sessions.root_branch")?,
        workspace,
        profile,
        branches,
    })
}

/// See [`Reader::branch`].
pub fn branch_blocking(c: &Connection, branch: BranchId) -> Result<BranchRow, SessionError> {
    let (session, parent, forked_at, state): (String, Option<String>, Option<String>, String) = c
        .query_row(
            "SELECT session, parent_branch, forked_at_turn, state FROM branches WHERE id = ?1",
            params![convert::id_str(&branch)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(map_err)?
        .ok_or(SessionError::NoSuchBranch { branch })?;

    Ok(BranchRow {
        session: convert::session_id(&session)?,
        parent: parent
            .map(|p| convert::parse_id::<BranchId>(&p, "branches.parent_branch"))
            .transpose()?,
        forked_at: forked_at
            .map(|t| convert::parse_id::<TurnId>(&t, "branches.forked_at_turn"))
            .transpose()?,
        status: if state == "open" {
            BranchStatus::Open
        } else {
            BranchStatus::Closed
        },
    })
}

/// See [`Reader::next_seq`].
pub fn next_seq_blocking(c: &Connection, branch: BranchId) -> Result<Seq, SessionError> {
    let max: i64 = c
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM turns WHERE branch = ?1",
            params![convert::id_str(&branch)],
            |r| r.get(0),
        )
        .map_err(map_err)?;
    Ok(Seq(max as u64 + 1))
}

/// See [`Reader::turn_branch`].
pub fn turn_branch_blocking(c: &Connection, turn: TurnId) -> Result<BranchId, SessionError> {
    let raw: String = c
        .query_row(
            "SELECT branch FROM turns WHERE id = ?1",
            params![convert::id_str(&turn)],
            |r| r.get(0),
        )
        .optional()
        .map_err(map_err)?
        .ok_or(SessionError::NoSuchTurn { turn })?;
    convert::parse_id(&raw, "turns.branch")
}

/// See [`Reader::turns_on`].
pub fn turns_on_blocking(c: &Connection, branch: BranchId) -> Result<Vec<TurnRow>, SessionError> {
    let mut stmt = c
        .prepare(
            "SELECT id, branch, seq, kind, payload, created_at \
             FROM turns WHERE branch = ?1 ORDER BY seq",
        )
        .map_err(map_err)?;
    let rows = stmt
        .query_map(params![convert::id_str(&branch)], |r| {
            Ok(convert::turn_row(r))
        })
        .map_err(map_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_err)?;
    rows.into_iter().collect()
}

/// See [`Reader::ancestry`]. Walks parent-ward, taking each ancestor's rows up
/// to the turn the child forked at, then appends the branch's own.
pub fn ancestry_blocking(c: &Connection, branch: BranchId) -> Result<Vec<TurnRow>, SessionError> {
    let row = branch_blocking(c, branch)?;
    let mut rows = match (row.parent, row.forked_at) {
        (Some(parent), Some(at)) => {
            let cut: i64 = c
                .query_row(
                    "SELECT seq FROM turns WHERE id = ?1",
                    params![convert::id_str(&at)],
                    |r| r.get(0),
                )
                .optional()
                .map_err(map_err)?
                .ok_or(SessionError::NoSuchTurn { turn: at })?;
            let mut up = ancestry_blocking(c, parent)?;
            up.retain(|r| r.branch != parent || r.seq.0 <= cut as u64);
            up
        }
        _ => Vec::new(),
    };
    rows.extend(turns_on_blocking(c, branch)?);
    Ok(rows)
}

/// See [`Reader::ancestry`]. The highest watermark on the branch wins; an
/// earlier compaction is superseded, not undone.
pub fn watermark_blocking(c: &Connection, branch: BranchId) -> Result<Option<Seq>, SessionError> {
    let max: Option<i64> = c
        .query_row(
            "SELECT MAX(upto_seq) FROM compactions WHERE branch = ?1",
            params![convert::id_str(&branch)],
            |r| r.get(0),
        )
        .map_err(map_err)?;
    Ok(max.map(|v| Seq(v as u64)))
}

/// See [`Reader::events_since`].
pub fn events_since_blocking(
    c: &Connection,
    session: SessionId,
    since: Option<Seq>,
) -> Result<Vec<StoredEvent>, SessionError> {
    let id = convert::id_str(&session);
    let exists: i64 = c
        .query_row(
            "SELECT count(*) FROM sessions WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(map_err)?;
    if exists == 0 {
        return Err(SessionError::NoSuchSession { session });
    }

    let floor = since.map_or(0i64, |s| s.0 as i64);
    let mut stmt = c
        .prepare("SELECT seq, payload FROM events WHERE session = ?1 AND seq > ?2 ORDER BY seq")
        .map_err(map_err)?;
    let rows = stmt
        .query_map(params![id, floor], |r| {
            let seq: i64 = r.get(0)?;
            let payload: Vec<u8> = r.get(1)?;
            Ok((seq, payload))
        })
        .map_err(map_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_err)?;

    rows.into_iter()
        .map(|(seq, payload)| {
            Ok(StoredEvent {
                seq: Seq(seq as u64),
                event: convert::decode_event(&payload)?,
            })
        })
        .collect()
}
