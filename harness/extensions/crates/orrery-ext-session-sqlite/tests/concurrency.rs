//! Task 5 · the writer actor under load, and after a failure.

use std::sync::Arc;

use orrery_ext_session_sqlite::{SqliteSessionStore, writer};
use orrery_proto::{Seq, TokenBudget, UserInput};
use orrery_session::algebra::CharsOverFour;
use orrery_session::turn::{NewTurn, TurnKind};
use orrery_session::{SessionError, SessionStore};
use tokio::sync::oneshot;

fn store(dir: &tempfile::TempDir) -> Arc<SqliteSessionStore> {
    Arc::new(SqliteSessionStore::open(dir.path().join("sessions.db")).expect("open"))
}

fn user(text: &str) -> NewTurn {
    NewTurn::new(TurnKind::User {
        input: UserInput::text(text),
    })
}

/// 100 appends from 10 tasks, each taking the lease in turn, produce seq 1..100
/// with no gaps.
///
/// The lease is `try_lock`, so a task that finds the branch busy backs off and
/// tries again — that retry loop is what a caller who wanted a queue has to
/// write themselves, and writing it here is the point: the store refuses to
/// pretend it is a queue.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn appends_on_one_branch_are_ordered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(&dir);

    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;

    let mut tasks = Vec::new();
    for task in 0..10u64 {
        let store = Arc::clone(&store);
        tasks.push(tokio::spawn(async move {
            for n in 0..10u64 {
                loop {
                    match store.lease(root).await {
                        Ok(lease) => {
                            store
                                .append(&lease, user(&format!("t{task}-{n}")))
                                .await
                                .expect("append");
                            break;
                        }
                        Err(SessionError::BranchBusy { .. }) => {
                            tokio::task::yield_now().await;
                        }
                        Err(e) => panic!("unexpected: {e:?}"),
                    }
                }
            }
        }));
    }
    for t in tasks {
        t.await.expect("task");
    }

    let rows = store.reader().turns_on(root).await.expect("rows");
    assert_eq!(rows.len(), 100, "every append landed");
    let seqs: Vec<u64> = rows.iter().map(|r| r.seq.0).collect();
    assert_eq!(
        seqs,
        (1..=100).collect::<Vec<u64>>(),
        "one-based, contiguous, no gaps and no duplicates"
    );

    let lease = store.lease(root).await.expect("lease");
    assert_eq!(lease.next_seq(), Seq(101));
}

/// One op fails; the next succeeds. The actor does not die on a single bad
/// write — a session store that stops answering ends the session (§4.7), so a
/// duplicate key must cost exactly one op and nothing more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn writer_survives_a_failed_op() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(&dir);

    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;
    let lease = store.lease(root).await.expect("lease");
    store
        .append(&lease, user("the first turn"))
        .await
        .expect("append");

    // Hand the actor a row that collides on (branch, seq) with the one above.
    // Nothing the trait exposes can produce this, which is the whole reason the
    // writer handle is reachable from a test.
    let w = store.writer_for(session).expect("writer");
    let (reply, rx) = oneshot::channel();
    w.send(writer::WriteOp::Append {
        session,
        branch: root,
        seq: Seq(1),
        id: orrery_proto::TurnId::new(),
        kind: TurnKind::User {
            input: UserInput::text("a colliding row"),
        },
        reply,
    })
    .expect("send");
    match writer::wait(rx).await {
        Err(SessionError::Corrupt { .. }) => {}
        other => panic!("a duplicate (branch, seq) is Corrupt, got {other:?}"),
    }

    // The actor is still there, and the transaction that failed left nothing
    // behind.
    store
        .append(&lease, user("the turn after the failure"))
        .await
        .expect("the writer survived");
    drop(lease);

    let rows = store.reader().turns_on(root).await.expect("rows");
    assert_eq!(rows.len(), 2, "the failed op rolled back completely");
    assert_eq!(rows[1].seq, Seq(2));

    let out = store
        .materialise(
            root,
            TokenBudget {
                max: u64::MAX,
                reserve: 0,
            },
            &CharsOverFour,
        )
        .await
        .expect("materialise");
    assert_eq!(out.messages.len(), 2);
}

/// Two branches, two writers, one file. WAL and the busy timeout are what keep
/// this from being a `SQLITE_BUSY` storm.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_branches_append_at_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store(&dir);

    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;
    let lease = store.lease(root).await.expect("lease");
    let anchor = store.append(&lease, user("fork me")).await.expect("append");
    drop(lease);

    let left = store.branch(anchor, "left").await.expect("branch");
    let right = store.branch(anchor, "right").await.expect("branch");

    let mut tasks = Vec::new();
    for (branch, tag) in [(left, "l"), (right, "r")] {
        let store = Arc::clone(&store);
        tasks.push(tokio::spawn(async move {
            let lease = store.lease(branch).await.expect("lease");
            for n in 0..25u64 {
                store
                    .append(&lease, user(&format!("{tag}{n}")))
                    .await
                    .expect("append");
            }
        }));
    }
    for t in tasks {
        t.await.expect("task");
    }

    for branch in [left, right] {
        let rows = store.reader().turns_on(branch).await.expect("rows");
        assert_eq!(rows.len(), 25);
        assert_eq!(
            rows.iter().map(|r| r.seq.0).collect::<Vec<_>>(),
            (1..=25).collect::<Vec<u64>>()
        );
    }

    // The session's event log took all 51 without a gap: the event sequence is
    // allocated inside the transaction, not guessed outside it.
    let events = store.events_since(session, None).await.expect("events");
    assert_eq!(events.len(), 51);
    for (i, e) in events.iter().enumerate() {
        assert_eq!(e.seq, Seq(i as u64 + 1));
    }
}
