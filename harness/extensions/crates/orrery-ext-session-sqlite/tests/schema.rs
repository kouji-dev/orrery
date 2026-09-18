//! Task 4 · the schema, and what it refuses.

use orrery_ext_session_sqlite::schema;
use orrery_session::SessionError;
use rusqlite::Connection;

fn conn(dir: &tempfile::TempDir) -> Connection {
    let mut c = Connection::open(dir.path().join("session.db")).expect("open");
    schema::prepare(&c).expect("pragmas");
    schema::apply(&mut c).expect("apply");
    c
}

#[test]
fn schema_applies_and_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut c = Connection::open(dir.path().join("session.db")).expect("open");
    schema::prepare(&c).expect("pragmas");

    schema::apply(&mut c).expect("first apply");
    schema::apply(&mut c).expect("second apply is a no-op, not an error");
    schema::apply(&mut c).expect("and a third");

    let version: i64 = c
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .expect("schema_version row");
    assert_eq!(version, schema::SCHEMA_VERSION);
}

#[test]
fn wal_and_synchronous_are_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    let c = conn(&dir);

    let mode: String = c
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .expect("journal_mode");
    assert_eq!(mode.to_lowercase(), "wal");

    // 1 == NORMAL.
    let sync: i64 = c
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .expect("synchronous");
    assert_eq!(sync, 1, "synchronous=NORMAL");

    let fk: i64 = c
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .expect("foreign_keys");
    assert_eq!(fk, 1, "foreign_keys=ON");
}

/// A duplicate `(branch, seq)` is the store's own invariant breaking. It must
/// come back as `SessionError::Corrupt` — a value somebody can be told about —
/// and never as a panic in a background task.
#[test]
fn unique_branch_seq_is_enforced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let c = conn(&dir);

    let insert = |id: &str| {
        c.execute(
            "INSERT INTO turns (id, branch, seq, kind, payload, created_at) \
             VALUES (?1, 'branch-a', 1, 'user', X'7B7D', 0)",
            [id],
        )
    };

    insert("turn-1").expect("the first row");
    let err = insert("turn-2").expect_err("the second row collides on (branch, seq)");

    match orrery_ext_session_sqlite::convert::map_err(err) {
        SessionError::Corrupt { detail } => {
            assert!(
                detail.to_lowercase().contains("constraint")
                    || detail.to_lowercase().contains("unique"),
                "the report says what broke: {detail}"
            );
        }
        other => panic!("a constraint violation is Corrupt, not {other:?}"),
    }
}

#[test]
fn every_table_the_plan_names_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let c = conn(&dir);
    for table in [
        "sessions",
        "branches",
        "turns",
        "compactions",
        "events",
        "meta",
    ] {
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .expect("query");
        assert_eq!(n, 1, "missing table {table}");
    }
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='turns_branch_seq'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(n, 1, "missing index turns_branch_seq");
}
