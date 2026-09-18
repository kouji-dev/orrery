//! A `Vec`-backed session store, for the sub-agent tests.
//!
//! **Not a shipped backend.** It is the same store `orrery-session`'s own
//! conformance test uses, copied rather than depended on because a `tests/`
//! binary cannot be linked by another crate — and because this crate must not
//! grow a dependency on `orrery-ext-session-sqlite`, which would be a core
//! crate depending on an extension (`deps-check` rule 1).
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use orrery_proto::{BranchId, Event, Seq, SessionId, TokenBudget, TurnId};
use orrery_session::algebra::{self, Materialised, TokenCounter};
use orrery_session::lease::{BranchLease, BranchStatus, LeaseRegistry};
use orrery_session::turn::{
    BranchOutcome, CompactResult, NewTurn, SessionHandle, SessionSummary, StoredEvent, TurnRow,
};
use orrery_session::{SessionError, SessionStore};

#[derive(Default)]
pub struct Branch {
    session: SessionId,
    parent: Option<BranchId>,
    forked_at: Option<TurnId>,
    #[allow(dead_code)]
    label: String,
    closed: bool,
}

#[derive(Default)]
pub struct Session {
    workspace: String,
    profile: String,
    created_at: i64,
    root: BranchId,
    branches: Vec<BranchId>,
    events: Vec<StoredEvent>,
}

#[derive(Default)]
pub struct Inner {
    sessions: HashMap<SessionId, Session>,
    branches: HashMap<BranchId, Branch>,
    turns: Vec<TurnRow>,
    compactions: Vec<(BranchId, Seq, TurnId)>,
}

#[derive(Default)]
pub struct MemoryStore {
    inner: Mutex<Inner>,
    leases: LeaseRegistry,
    clock: AtomicU64,
}

impl MemoryStore {
    fn now(&self) -> i64 {
        self.clock.fetch_add(1, Ordering::Relaxed) as i64
    }

    /// The branch's whole ancestry, oldest first: the parent's rows up to the
    /// fork point, then the child's own.
    fn ancestry(inner: &Inner, branch: BranchId) -> Result<Vec<TurnRow>, SessionError> {
        let b = inner
            .branches
            .get(&branch)
            .ok_or(SessionError::NoSuchBranch { branch })?;

        let mut rows = match (b.parent, b.forked_at) {
            (Some(parent), Some(at)) => {
                let cut = inner
                    .turns
                    .iter()
                    .find(|r| r.id == at)
                    .map(|r| r.seq)
                    .ok_or(SessionError::NoSuchTurn { turn: at })?;
                let mut up = Self::ancestry(inner, parent)?;
                up.retain(|r| r.branch != parent || r.seq <= cut);
                up
            }
            _ => Vec::new(),
        };
        let mut own: Vec<TurnRow> = inner
            .turns
            .iter()
            .filter(|r| r.branch == branch)
            .cloned()
            .collect();
        own.sort_by_key(|r| r.seq);
        rows.append(&mut own);
        Ok(rows)
    }

    fn watermark(inner: &Inner, branch: BranchId) -> Option<Seq> {
        inner
            .compactions
            .iter()
            .filter(|(b, ..)| *b == branch)
            .map(|(_, seq, _)| *seq)
            .max()
    }
}

#[async_trait]
impl SessionStore for MemoryStore {
    async fn create(&self, workspace: &str, profile: &str) -> Result<SessionId, SessionError> {
        let session = SessionId::new();
        let root = BranchId::new();
        let now = self.now();
        let mut inner = self.inner.lock().unwrap();
        inner.sessions.insert(
            session,
            Session {
                workspace: workspace.to_owned(),
                profile: profile.to_owned(),
                created_at: now,
                root,
                branches: vec![root],
                events: Vec::new(),
            },
        );
        inner.branches.insert(
            root,
            Branch {
                session,
                parent: None,
                forked_at: None,
                label: "main".into(),
                closed: false,
            },
        );
        drop(inner);
        self.leases.register(root, Seq(1), BranchStatus::Open);
        Ok(session)
    }

    async fn open(&self, session: SessionId) -> Result<SessionHandle, SessionError> {
        let inner = self.inner.lock().unwrap();
        let s = inner
            .sessions
            .get(&session)
            .ok_or(SessionError::NoSuchSession { session })?;
        Ok(SessionHandle {
            session,
            root: s.root,
            workspace: s.workspace.clone(),
            profile: s.profile.clone(),
            branches: s.branches.clone(),
        })
    }

    /// Newest first, as `orrery session list` prints them. Implemented here and
    /// not inherited from the trait's default: the default refuses, and the
    /// conformance suite counts refusing as non-conformant.
    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError> {
        let inner = self.inner.lock().unwrap();
        let mut out: Vec<SessionSummary> = inner
            .sessions
            .iter()
            .map(|(id, s)| SessionSummary {
                session: *id,
                workspace: s.workspace.clone(),
                profile: s.profile.clone(),
                created_at: s.created_at,
                turns: inner
                    .turns
                    .iter()
                    .filter(|r| {
                        inner
                            .branches
                            .get(&r.branch)
                            .is_some_and(|b| b.session == *id)
                    })
                    .count() as u64,
            })
            .collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
    }

    async fn lease(&self, branch: BranchId) -> Result<BranchLease, SessionError> {
        self.leases.lease(branch)
    }

    async fn append(&self, lease: &BranchLease, turn: NewTurn) -> Result<TurnId, SessionError> {
        lease.ensure_open()?;
        let branch = lease.branch();
        let seq = lease.next_seq();
        let id = TurnId::new();
        let created_at = self.now();

        let mut inner = self.inner.lock().unwrap();
        if inner
            .turns
            .iter()
            .any(|r| r.branch == branch && r.seq == seq)
        {
            return Err(SessionError::Corrupt {
                detail: format!("duplicate ({branch}, {seq})"),
            });
        }
        let usage = turn.kind.usage();
        inner.turns.push(TurnRow {
            id,
            branch,
            seq,
            kind: turn.kind,
            created_at,
        });
        let session = inner.branches[&branch].session;
        let s = inner.sessions.get_mut(&session).expect("session");
        let next = Seq(s.events.len() as u64 + 1);
        s.events.push(StoredEvent {
            seq: next,
            event: Event::TurnSettled {
                seq: next,
                turn: id,
                usage,
            },
        });
        drop(inner);
        lease.commit_seq(seq);
        Ok(id)
    }

    async fn branch(&self, from: TurnId, label: &str) -> Result<BranchId, SessionError> {
        let child = BranchId::new();
        let mut inner = self.inner.lock().unwrap();
        let parent = inner
            .turns
            .iter()
            .find(|r| r.id == from)
            .map(|r| r.branch)
            .ok_or(SessionError::NoSuchTurn { turn: from })?;
        let session = inner.branches[&parent].session;
        inner.branches.insert(
            child,
            Branch {
                session,
                parent: Some(parent),
                forked_at: Some(from),
                label: label.to_owned(),
                closed: false,
            },
        );
        inner
            .sessions
            .get_mut(&session)
            .expect("session")
            .branches
            .push(child);
        drop(inner);
        self.leases.register(child, Seq(1), BranchStatus::Open);
        Ok(child)
    }

    /// Writes only the **child's** rows. It never reaches for the parent's
    /// lease, which is the whole reason a sub-agent does not deadlock its
    /// parent.
    async fn close_branch(
        &self,
        lease: BranchLease,
        _outcome: BranchOutcome,
    ) -> Result<(), SessionError> {
        let branch = lease.branch();
        let mut inner = self.inner.lock().unwrap();
        inner
            .branches
            .get_mut(&branch)
            .ok_or(SessionError::NoSuchBranch { branch })?
            .closed = true;
        drop(inner);
        lease.mark_closed();
        drop(lease);
        Ok(())
    }

    async fn materialise(
        &self,
        branch: BranchId,
        budget: TokenBudget,
        counter: &dyn TokenCounter,
    ) -> Result<Materialised, SessionError> {
        let inner = self.inner.lock().unwrap();
        let rows = Self::ancestry(&inner, branch)?;
        let mark = Self::watermark(&inner, branch);
        drop(inner);
        Ok(algebra::materialise(&rows, mark, budget, counter))
    }

    async fn compact(
        &self,
        lease: &BranchLease,
        upto: Seq,
        summary: NewTurn,
    ) -> Result<CompactResult, SessionError> {
        lease.ensure_open()?;
        let branch = lease.branch();
        let id = self.append(lease, summary).await?;
        let mut inner = self.inner.lock().unwrap();
        inner.compactions.push((branch, upto, id));
        let covered = inner
            .turns
            .iter()
            .filter(|r| r.branch == branch && r.seq <= upto)
            .count() as u64;
        Ok(CompactResult {
            summary: id,
            watermark: upto,
            covered,
        })
    }

    async fn events_since(
        &self,
        session: SessionId,
        since: Option<Seq>,
    ) -> Result<Vec<StoredEvent>, SessionError> {
        let inner = self.inner.lock().unwrap();
        let s = inner
            .sessions
            .get(&session)
            .ok_or(SessionError::NoSuchSession { session })?;
        let floor = since.map_or(0, |s| s.0);
        Ok(s.events
            .iter()
            .filter(|e| e.seq.0 > floor)
            .cloned()
            .collect())
    }
}

impl MemoryStore {
    /// Every row on one branch, in sequence order.
    ///
    /// The whole point of a sub-agent running on a branch: its turns are
    /// ordinary rows somebody can go and read.
    pub fn rows(&self, branch: BranchId) -> Vec<TurnRow> {
        let inner = self.inner.lock().unwrap();
        let mut rows: Vec<TurnRow> = inner
            .turns
            .iter()
            .filter(|r| r.branch == branch)
            .cloned()
            .collect();
        rows.sort_by_key(|r| r.seq);
        rows
    }

    /// Whether a branch has been closed.
    pub fn is_closed(&self, branch: BranchId) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.branches.get(&branch).is_some_and(|b| b.closed)
    }
}
