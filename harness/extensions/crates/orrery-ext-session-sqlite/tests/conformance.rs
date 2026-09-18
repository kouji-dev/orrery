//! The same suite the in-test memory store runs, against SQLite — plus the two
//! compaction assertions that need to look at the rows directly (Task 7).

use std::sync::Arc;

use orrery_ext_session_sqlite::SqliteSessionStore;
use orrery_proto::{Seq, TokenBudget, Usage, UserInput};
use orrery_session::algebra::CharsOverFour;
use orrery_session::conformance;
use orrery_session::turn::{NewTurn, TurnKind};
use orrery_session::{SessionError, SessionStore};

fn store(dir: &tempfile::TempDir) -> Arc<dyn SessionStore> {
    Arc::new(SqliteSessionStore::open(dir.path().join("sessions.db")).expect("open"))
}

fn concrete(dir: &tempfile::TempDir) -> SqliteSessionStore {
    SqliteSessionStore::open(dir.path().join("sessions.db")).expect("open")
}

fn user(text: &str) -> NewTurn {
    NewTurn::new(TurnKind::User {
        input: UserInput::text(text),
    })
}

fn unbounded() -> TokenBudget {
    TokenBudget {
        max: u64::MAX,
        reserve: 0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sqlite_passes_the_conformance_suite() {
    let dir = tempfile::tempdir().expect("tempdir");
    conformance::run_conformance(store(&dir)).await;
}

/// Task 8 on the real backend, on its own so a failure names itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn child_close_does_not_deadlock_the_parent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(&dir);
    conformance::child_close_does_not_deadlock_the_parent(&*store).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_lease_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(&dir);
    conformance::second_lease_is_refused(&*store).await;
}

/// Task 7 · `compact` writes. Reads the `turns` table directly, because the
/// trait cannot see whether a row was deleted — only whether the view changed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_rows_survive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = concrete(&dir);

    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;
    let lease = store.lease(root).await.expect("lease");
    for i in 1..=5u64 {
        store
            .append(&lease, user(&format!("turn {i}")))
            .await
            .expect("append");
    }
    store
        .compact(
            &lease,
            Seq(3),
            NewTurn::new(TurnKind::Summary {
                covers: (Seq(1), Seq(3)),
                text: "the first three".into(),
                usage: Usage::default(),
            }),
        )
        .await
        .expect("compact");
    drop(lease);

    let rows = store.reader().turns_on(root).await.expect("read the rows");
    let seqs: Vec<u64> = rows.iter().map(|r| r.seq.0).collect();
    assert_eq!(
        seqs,
        vec![1, 2, 3, 4, 5, 6],
        "the compacted rows are still on disk; the summary is row 6"
    );
    assert!(
        matches!(rows[5].kind, TurnKind::Summary { .. }),
        "the summary is a row like any other"
    );
}

/// Task 7 · two compactions, the later one wins, and the earlier one is still
/// a row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn materialise_uses_the_highest_watermark() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = concrete(&dir);

    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;
    let lease = store.lease(root).await.expect("lease");
    for i in 1..=6u64 {
        store
            .append(&lease, user(&format!("turn {i}")))
            .await
            .expect("append");
    }
    for (upto, text) in [(2u64, "first pass"), (4, "second pass")] {
        store
            .compact(
                &lease,
                Seq(upto),
                NewTurn::new(TurnKind::Summary {
                    covers: (Seq(1), Seq(upto)),
                    text: text.into(),
                    usage: Usage::default(),
                }),
            )
            .await
            .expect("compact");
    }
    drop(lease);

    let out = store
        .materialise(root, unbounded(), &CharsOverFour)
        .await
        .expect("materialise");
    assert_eq!(out.watermark, Some(Seq(4)));

    let rows = store.reader().turns_on(root).await.expect("rows");
    assert_eq!(
        rows.len(),
        8,
        "six turns and two summaries, all still there"
    );
}

/// Everything survives the process going away and coming back — the ordinary
/// case, with no kill involved. The crash test covers the violent one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_reopens_where_it_left_off() {
    let dir = tempfile::tempdir().expect("tempdir");
    let session;
    let root;
    {
        let store = concrete(&dir);
        session = store.create("/ws", "default").await.expect("create");
        root = store.open(session).await.expect("open").root;
        let lease = store.lease(root).await.expect("lease");
        for i in 1..=3u64 {
            store
                .append(&lease, user(&format!("turn {i}")))
                .await
                .expect("append");
        }
    }

    let store = concrete(&dir);
    let handle = store.open(session).await.expect("reopen");
    assert_eq!(handle.root, root);
    assert_eq!(handle.workspace, "/ws");

    // The lease picks up at 4, not at 1: the branch's counter comes from the
    // rows, not from memory.
    let lease = store.lease(root).await.expect("lease");
    assert_eq!(lease.next_seq(), Seq(4));
    store.append(&lease, user("turn 4")).await.expect("append");
    drop(lease);

    let out = store
        .materialise(root, unbounded(), &CharsOverFour)
        .await
        .expect("materialise");
    assert_eq!(out.messages.len(), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_session_is_not_a_panic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = concrete(&dir);
    let ghost = orrery_proto::SessionId::new();
    assert!(matches!(
        store.open(ghost).await,
        Err(SessionError::NoSuchSession { .. })
    ));
    assert!(matches!(
        store.events_since(ghost, None).await,
        Err(SessionError::NoSuchSession { .. })
    ));
    assert!(matches!(
        store.lease(orrery_proto::BranchId::new()).await,
        Err(SessionError::NoSuchBranch { .. })
    ));
}
