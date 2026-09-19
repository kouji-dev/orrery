//! A state directory past Windows' `MAX_PATH` still opens.
//!
//! The eval runner is where this bit: at a 244-character workspace the state
//! file reaches 263 characters, and `orrery eval run` came back
//! "session store backend failed: unable to open database file". Nothing was
//! wrong with the database — the path was simply handed to SQLite in its plain
//! form, and the Win32 layer under it stops at 260 characters unless the path
//! is given in the extended-length `\?\` form.
//!
//! The fix is in [`orrery_ext_session_sqlite::build`] / `SqliteSessionStore::open`,
//! and this pins it at a length that fails without it.

use std::path::PathBuf;

use orrery_ext_session_sqlite::SqliteSessionStore;
use orrery_proto::UserInput;
use orrery_session::SessionStore as _;
use orrery_session::turn::{NewTurn, TurnKind};

/// A directory whose path is at least `want` characters long.
fn deep(root: &std::path::Path, want: usize) -> PathBuf {
    let mut dir = root.to_path_buf();
    while dir.display().to_string().len() < want {
        dir.push("a-long-workspace-segment");
    }
    std::fs::create_dir_all(&dir).expect("the deep directory");
    dir
}

#[tokio::test]
async fn a_state_dir_past_max_path_still_opens() {
    let temp = tempfile::tempdir().expect("a temporary root");
    // 244 is the reported workspace length; `.orrery/sessions.db` is what takes
    // the whole thing over 260.
    let dir = deep(temp.path(), 244).join(".orrery");
    std::fs::create_dir_all(&dir).expect("the state directory");
    let db = dir.join("sessions.db");
    assert!(
        db.display().to_string().len() > 260,
        "the fixture has to cross MAX_PATH to be testing anything: {}",
        db.display().to_string().len()
    );

    let store = SqliteSessionStore::open(&db).expect("the store opens past MAX_PATH");
    assert!(
        store.path().display().to_string().contains("sessions.db"),
        "it is still the database it was asked for"
    );

    // Opening is not enough. A per-session writer is spawned on the first
    // append, long after `open` returned, and it opens its own connection: the
    // first version of this fix converted the path in `open` alone and the
    // whole run still died here.
    let session = store
        .create("a-long-workspace", "default")
        .await
        .expect("a session past MAX_PATH");
    let handle = store.open(session).await.expect("the session opens");
    let lease = store.lease(handle.root).await.expect("a lease");
    store
        .append(
            &lease,
            NewTurn::new(TurnKind::User {
                input: UserInput::text("does this reach the disk?"),
            }),
        )
        .await
        .expect("a turn appends past MAX_PATH");

    // And `build`, which is the entry point the harness actually calls, makes
    // the directory and opens the file at the same length.
    let other = deep(temp.path(), 244).join(".orrery-built");
    orrery_ext_session_sqlite::build(&other).expect("build opens past MAX_PATH");
}
