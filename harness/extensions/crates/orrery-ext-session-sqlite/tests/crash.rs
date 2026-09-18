//! Task 6 · crash durability: a session loses at most one turn.
//!
//! Not a simulation. A real child process opens the database, appends turns and
//! is killed outright — on Windows `Child::kill` is `TerminateProcess`, which
//! gives the child no chance to flush, close the connection or checkpoint the
//! WAL. That is the failure this test is about: the OS still holds the pages,
//! so `synchronous=NORMAL` is durable against exactly this and expensive only
//! against losing the machine.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str::FromStr;

use orrery_ext_session_sqlite::SqliteSessionStore;
use orrery_proto::{BranchId, SessionId, TokenBudget};
use orrery_session::SessionStore;
use orrery_session::algebra::CharsOverFour;

/// `cargo test` builds examples into `target/<profile>/examples`, next to the
/// `deps/` this test binary lives in.
fn crash_child() -> PathBuf {
    let mut p = std::env::current_exe().expect("current_exe");
    p.pop(); // deps/
    p.pop(); // debug/ or release/
    p.push("examples");
    p.push(format!("crash_child{}", std::env::consts::EXE_SUFFIX));
    if !p.exists() {
        // `cargo test -p orrery-ext-session-sqlite` builds examples; `cargo test
        // --test crash` does not. Build it rather than fail on how the test was
        // invoked.
        let ok = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args([
                "build",
                "-p",
                "orrery-ext-session-sqlite",
                "--example",
                "crash_child",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(
            ok && p.exists(),
            "could not build crash_child at {}",
            p.display()
        );
    }
    p
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
fn loses_at_most_one_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("sessions.db");

    let mut child = Command::new(crash_child())
        .arg(&db)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn the child");

    let stdout = child.stdout.take().expect("stdout");
    let mut lines = BufReader::new(stdout).lines();

    let header = lines
        .next()
        .expect("the child announced its session")
        .expect("read");
    let mut parts = header.split_whitespace();
    assert_eq!(parts.next(), Some("SESSION"));
    let session = SessionId::from_str(parts.next().expect("session id")).expect("session id");
    let root = BranchId::from_str(parts.next().expect("branch id")).expect("branch id");

    // Read a handful of acknowledged commits, then pull the plug.
    const ACKED: u64 = 5;
    let mut last_acked = 0u64;
    for line in lines.by_ref() {
        let line = line.expect("read");
        let n: u64 = line
            .strip_prefix("OK ")
            .expect("an OK line")
            .parse()
            .expect("a turn number");
        assert_eq!(n, last_acked + 1, "the child acks in order");
        last_acked = n;
        if last_acked >= ACKED {
            break;
        }
    }
    assert_eq!(last_acked, ACKED);

    // On Windows this is TerminateProcess: no unwinding, no flush, no cleanup.
    child.kill().expect("kill the child");
    let status = child.wait().expect("reap");
    assert!(!status.success(), "the child was killed, not finished");
    drop(lines);

    // The WAL is still sitting beside the database, unclosed.
    let wal = db.with_extension("db-wal");
    assert!(
        wal.exists(),
        "a killed writer leaves its WAL behind, at {}",
        wal.display()
    );

    // Reopen. No repair step, no `PRAGMA integrity_check`, no manual recovery:
    // opening the database is what recovers it.
    let rt = runtime();
    rt.block_on(async move {
        let store = SqliteSessionStore::open(&db).expect("reopen recovers the WAL");
        let handle = store.open(session).await.expect("the session is there");
        assert_eq!(handle.root, root);

        let rows = store.reader().turns_on(root).await.expect("rows");
        let kept = rows.len() as u64;
        assert!(
            kept >= last_acked,
            "every acknowledged turn survived: acked {last_acked}, kept {kept}"
        );
        assert!(
            kept <= last_acked + 1,
            "at most one turn is lost, and it is one that was never acknowledged: \
             acked {last_acked}, kept {kept}"
        );

        // Contiguous: a half-written turn would have shown up as a gap.
        let seqs: Vec<u64> = rows.iter().map(|r| r.seq.0).collect();
        assert_eq!(seqs, (1..=kept).collect::<Vec<u64>>());

        // And the session still materialises, which is the thing that actually
        // matters: a session that survives but cannot be replayed is lost.
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
            .expect("materialise after the crash");
        assert_eq!(out.messages.len() as u64, kept);

        // The reopened session takes turns again, from where it stopped.
        let lease = store.lease(root).await.expect("lease");
        assert_eq!(lease.next_seq().0, kept + 1);
        store
            .append(
                &lease,
                orrery_session::turn::NewTurn::new(orrery_session::turn::TurnKind::User {
                    input: orrery_proto::UserInput::text("after the crash"),
                }),
            )
            .await
            .expect("the session carries on");
    });
}
