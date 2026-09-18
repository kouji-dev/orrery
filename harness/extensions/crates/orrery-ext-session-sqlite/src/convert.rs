//! Between SQLite's vocabulary and the session store's.
//!
//! Ids are stored as their `Display` form — a uuid string, which is what makes
//! a session database readable with `sqlite3` and greppable in a bug report.
//! Turn payloads are `serde_json`.

use std::str::FromStr;

use orrery_proto::{BranchId, Event, Seq, SessionId, TurnId};
use orrery_session::SessionError;
use orrery_session::turn::{TurnKind, TurnRow};
use rusqlite::Row;

/// Everything SQLite can say, in the store's terms.
///
/// A constraint violation is the store's **own invariant** breaking — a
/// duplicate `(branch, seq)`, a second compaction at the same watermark — so it
/// is [`SessionError::Corrupt`], something a person can be told about, and
/// never a panic in a background task. Everything else is a backend failure.
#[must_use]
pub fn map_err(err: rusqlite::Error) -> SessionError {
    use rusqlite::ErrorCode::{ConstraintViolation, DatabaseCorrupt};
    match &err {
        rusqlite::Error::SqliteFailure(e, _)
            if matches!(e.code, ConstraintViolation | DatabaseCorrupt) =>
        {
            SessionError::corrupt(err)
        }
        rusqlite::Error::FromSqlConversionFailure(..)
        | rusqlite::Error::InvalidColumnType(..)
        | rusqlite::Error::IntegralValueOutOfRange(..) => SessionError::corrupt(err),
        _ => SessionError::backend(err),
    }
}

/// A payload blob that will not deserialise is corruption, not a backend fault:
/// the bytes are there and they are wrong.
#[must_use]
pub fn map_json(err: serde_json::Error) -> SessionError {
    SessionError::corrupt(format!("payload: {err}"))
}

/// Serialise a turn's payload.
///
/// # Errors
///
/// [`SessionError::Corrupt`] when the kind will not serialise.
pub fn encode_kind(kind: &TurnKind) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(kind).map_err(map_json)
}

/// Deserialise a turn's payload.
///
/// # Errors
///
/// [`SessionError::Corrupt`] when the blob is not a turn.
pub fn decode_kind(blob: &[u8]) -> Result<TurnKind, SessionError> {
    serde_json::from_slice(blob).map_err(map_json)
}

/// Serialise a stored event.
///
/// # Errors
///
/// [`SessionError::Corrupt`] when the event will not serialise.
pub fn encode_event(event: &Event) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(event).map_err(map_json)
}

/// Deserialise a stored event.
///
/// # Errors
///
/// [`SessionError::Corrupt`] when the blob is not an event.
pub fn decode_event(blob: &[u8]) -> Result<Event, SessionError> {
    serde_json::from_slice(blob).map_err(map_json)
}

/// Parse an id out of a text column.
///
/// # Errors
///
/// [`SessionError::Corrupt`] when the column does not hold one.
pub fn parse_id<T: FromStr>(raw: &str, what: &str) -> Result<T, SessionError> {
    raw.parse()
        .map_err(|_| SessionError::corrupt(format!("{what} column holds `{raw}`")))
}

/// `SELECT id, branch, seq, kind, payload, created_at FROM turns` into a row.
///
/// # Errors
///
/// [`SessionError::Corrupt`] when a column is not what the schema promises.
pub fn turn_row(row: &Row<'_>) -> Result<TurnRow, SessionError> {
    let id: String = row.get(0).map_err(map_err)?;
    let branch: String = row.get(1).map_err(map_err)?;
    let seq: i64 = row.get(2).map_err(map_err)?;
    let payload: Vec<u8> = row.get(4).map_err(map_err)?;
    Ok(TurnRow {
        id: parse_id::<TurnId>(&id, "turns.id")?,
        branch: parse_id::<BranchId>(&branch, "turns.branch")?,
        seq: Seq(seq as u64),
        kind: decode_kind(&payload)?,
        created_at: row.get(5).map_err(map_err)?,
    })
}

/// Unix milliseconds, saturating rather than panicking on a clock before 1970.
#[must_use]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The text form every id column holds.
#[must_use]
pub fn id_str<T: std::fmt::Display>(id: &T) -> String {
    id.to_string()
}

/// A session id from a text column.
///
/// # Errors
///
/// [`SessionError::Corrupt`] when the column does not hold one.
pub fn session_id(raw: &str) -> Result<SessionId, SessionError> {
    parse_id(raw, "session")
}
