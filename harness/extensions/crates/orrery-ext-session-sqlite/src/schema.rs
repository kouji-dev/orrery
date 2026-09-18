//! The DDL, the pragmas, and the one row that gates a future migration.
//!
//! `payload` is a `serde_json` blob rather than a column per field. The turn
//! shape will keep moving — a new `TurnKind`, a new field on `Usage` — and a
//! migration per field is not worth it for a table nothing queries by content.
//! What *is* queried (`branch`, `seq`, `kind`) is a real column.

use rusqlite::Connection;

/// The schema this build writes. A future migration reads the `meta` row,
/// compares, and steps forward; there is nothing to step from yet.
pub const SCHEMA_VERSION: i64 = 1;

const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    workspace   TEXT,
    profile     TEXT,
    root_branch TEXT,
    created_at  INTEGER
);

CREATE TABLE IF NOT EXISTS branches (
    id             TEXT PRIMARY KEY,
    session        TEXT NOT NULL,
    parent_branch  TEXT,
    forked_at_turn TEXT,
    label          TEXT,
    state          TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS turns (
    id         TEXT PRIMARY KEY,
    branch     TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    payload    BLOB NOT NULL,
    created_at INTEGER,
    UNIQUE(branch, seq)
);

CREATE TABLE IF NOT EXISTS compactions (
    branch       TEXT NOT NULL,
    upto_seq     INTEGER NOT NULL,
    summary_turn TEXT NOT NULL,
    PRIMARY KEY (branch, upto_seq)
);

CREATE TABLE IF NOT EXISTS events (
    session TEXT NOT NULL,
    seq     INTEGER NOT NULL,
    payload BLOB NOT NULL,
    UNIQUE(session, seq)
);

CREATE INDEX IF NOT EXISTS turns_branch_seq ON turns(branch, seq);

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);
"#;

/// Set the pragmas. Called on **every** connection, writer and reader alike:
/// `journal_mode` persists in the file, the rest are per-connection and would
/// silently be the default on a reader that skipped them.
///
/// - `journal_mode=WAL` — a reader does not block the writer, which is the
///   whole reason the read path can be a second connection.
/// - `synchronous=NORMAL` — a commit is durable against a killed *process*
///   (the pages are with the OS), and at risk only against a lost machine.
///   That is exactly the "loses at most one turn" the plan asks for, at a
///   fraction of `FULL`'s cost per append.
/// - `foreign_keys=ON` — declared for the day the schema grows a `REFERENCES`.
/// - `busy_timeout` — one writer per session, several sessions per file: a
///   writer that arrives mid-commit waits rather than returning `SQLITE_BUSY`.
///
/// # Errors
///
/// Anything SQLite refuses.
pub fn prepare(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(())
}

/// Create everything, idempotently, in one transaction.
///
/// # Errors
///
/// Anything SQLite refuses.
pub fn apply(conn: &mut Connection) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch(DDL)?;
    tx.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [SCHEMA_VERSION],
    )?;
    tx.commit()
}

/// Open a connection, set the pragmas and apply the schema.
///
/// # Errors
///
/// Anything SQLite refuses.
pub fn open_write(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let mut conn = Connection::open(path)?;
    prepare(&conn)?;
    apply(&mut conn)?;
    Ok(conn)
}

/// Open a **read-only** connection to an existing database.
///
/// # Errors
///
/// Anything SQLite refuses.
pub fn open_read(path: &std::path::Path) -> rusqlite::Result<Connection> {
    use rusqlite::OpenFlags;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    // `journal_mode` is not settable on a read-only connection, and does not
    // need to be: it is already WAL in the file.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(conn)
}
