//! The conformance suite, run against a `Vec`-backed store.
//!
//! The store in this file is **not a shipped backend**. It exists to prove that
//! [`orrery_session::conformance::run_conformance`] runs at all, before
//! `orrery-ext-session-sqlite` exists — otherwise the suite and the first
//! backend would be written together and each would be shaped around the
//! other's bugs.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use orrery_proto::{BranchId, Event, Seq, SessionId, TokenBudget, TurnId, Usage};
use orrery_session::algebra::{self, Materialised, TokenCounter};
use orrery_session::conformance;
use orrery_session::lease::{BranchLease, BranchStatus, LeaseRegistry};
use orrery_session::turn::{
    BranchOutcome, CompactResult, NewTurn, SessionHandle, StoredEvent, TurnKind, TurnRow,
};
use orrery_session::{SessionError, SessionStore};

#[derive(Default)]
struct Branch {
    session: SessionId,
    parent: Option<BranchId>,
    forked_at: Option<TurnId>,
    #[allow(dead_code)]
    label: String,
    closed: bool,
}

#[derive(Default)]
struct Session {
    workspace: String,
    profile: String,
    root: BranchId,
    branches: Vec<BranchId>,
    events: Vec<StoredEvent>,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<SessionId, Session>,
    branches: HashMap<BranchId, Branch>,
    turns: Vec<TurnRow>,
    compactions: Vec<(BranchId, Seq, TurnId)>,
}

#[derive(Default)]
struct MemoryStore {
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
        let mut inner = self.inner.lock().unwrap();
        inner.sessions.insert(
            session,
            Session {
                workspace: workspace.to_owned(),
                profile: profile.to_owned(),
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
        if inner.turns.iter().any(|r| r.branch == branch && r.seq == seq) {
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

fn store() -> std::sync::Arc<dyn SessionStore> {
    std::sync::Arc::new(MemoryStore::default())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_store_passes_the_conformance_suite() {
    conformance::run_conformance(store()).await;
}

/// Task 8, run on its own as well as inside the suite, so a failure names
/// itself instead of hiding behind whatever ran before it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn child_close_does_not_deadlock_the_parent() {
    let store = store();
    conformance::child_close_does_not_deadlock_the_parent(&*store).await;
}

#[tokio::test]
async fn second_lease_is_refused() {
    let store = store();
    conformance::second_lease_is_refused(&*store).await;
}

#[tokio::test]
async fn two_branches_lease_concurrently() {
    let store = store();
    conformance::two_branches_lease_concurrently(&*store).await;
}

/// The lease is the only way to append, and it is released by being dropped.
#[tokio::test]
async fn a_dropped_lease_frees_the_branch() {
    let store = store();
    let session = store.create("/ws", "p").await.unwrap();
    let root = store.open(session).await.unwrap().root;
    {
        let _l = store.lease(root).await.unwrap();
        assert!(matches!(
            store.lease(root).await,
            Err(SessionError::BranchBusy { .. })
        ));
    }
    assert!(store.lease(root).await.is_ok());
}

/// A summary turn is a row like any other: it has an id, a seq and a usage.
#[tokio::test]
async fn a_summary_is_an_ordinary_row() {
    let store = store();
    let session = store.create("/ws", "p").await.unwrap();
    let root = store.open(session).await.unwrap().root;
    let lease = store.lease(root).await.unwrap();
    store
        .append(
            &lease,
            NewTurn::new(TurnKind::Summary {
                covers: (Seq(1), Seq(1)),
                text: "nothing much".into(),
                usage: Usage::default(),
            }),
        )
        .await
        .unwrap();
    assert_eq!(lease.next_seq(), Seq(2));
}
