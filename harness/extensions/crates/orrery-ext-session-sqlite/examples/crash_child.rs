//! A session that appends turns until somebody kills it.
//!
//! Used by `tests/crash.rs`. It creates a session in the database it is handed,
//! prints the session and root branch ids, then appends turns forever, printing
//! one line per **committed** turn. The test reads those lines, kills the
//! process, and reopens the database: whatever was acknowledged must be there.
//!
//! It is an example rather than a `#[test]` because the point is a real process
//! that a real `TerminateProcess` can end. `cargo test` builds examples, so the
//! test finds this binary beside its own.

use std::io::Write;
use std::time::Duration;

use orrery_ext_session_sqlite::SqliteSessionStore;
use orrery_proto::UserInput;
use orrery_session::SessionStore;
use orrery_session::turn::{NewTurn, TurnKind};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: crash_child <path/to/sessions.db>");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");

    rt.block_on(async move {
        let store = SqliteSessionStore::open(&path).expect("open");
        let session = store.create("/ws", "crash").await.expect("create");
        let root = store.open(session).await.expect("open").root;
        line(&format!("SESSION {session} {root}"));

        let lease = store.lease(root).await.expect("lease");
        for i in 1u64.. {
            store
                .append(
                    &lease,
                    NewTurn::new(TurnKind::User {
                        input: UserInput::text(format!("turn {i}")),
                    }),
                )
                .await
                .expect("append");
            // Printed only after the transaction has committed. Every line the
            // test reads is a turn the store promised to keep.
            line(&format!("OK {i}"));
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
}

fn line(s: &str) {
    let mut out = std::io::stdout();
    writeln!(out, "{s}").expect("write");
    out.flush().expect("flush");
}
