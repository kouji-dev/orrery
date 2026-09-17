use rusqlite::OptionalExtension;

use crate::core::database::DB;
use crate::core::errors::{AppResult, DbError};

/// The one row's key. Same key/value shape as `settings` so a later need for
/// more documents (e.g. per-project workspaces) is a new key, not a migration.
const KEY: &str = "workspace";

#[derive(Clone)]
pub struct WorkspaceService {
    db: DB,
}

impl WorkspaceService {
    pub fn new(db: DB) -> Self {
        let svc = Self { db };
        svc.init_schema();
        svc
    }

    fn init_schema(&self) {
        let c = self.db.lock().unwrap();
        c.execute(
            "CREATE TABLE IF NOT EXISTS workspace (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .unwrap();
    }

    /// The persisted workspace document. `None` means "never saved" — the
    /// frontend uses that as its localStorage-migration signal — so a malformed
    /// row (manual edit, partial write) also maps to `None` rather than an
    /// error: the workspace must never block startup.
    pub fn get(&self) -> AppResult<Option<serde_json::Value>> {
        let raw: Option<String> = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT value FROM workspace WHERE key = ?1",
                [KEY],
                |r| r.get(0),
            )
            .optional()
            .map_err(DbError::Sqlite)?
        };
        Ok(raw.and_then(|s| {
            serde_json::from_str(&s)
                .inspect_err(|e| log::warn!("workspace row malformed, starting fresh: {e}"))
                .ok()
        }))
    }

    /// Persist the whole document (upsert the one row). Passthrough — the
    /// frontend owns the schema.
    pub fn set(&self, doc: &serde_json::Value) -> AppResult<()> {
        let json = serde_json::to_string(doc)
            .map_err(|e| crate::core::errors::AppError::Other(format!("workspace encode: {e}")))?;
        let c = self.db.lock().unwrap();
        c.execute(
            "INSERT INTO workspace (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![KEY, json],
        )
        .map_err(DbError::Sqlite)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn svc() -> WorkspaceService {
        let db: DB = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        WorkspaceService::new(db)
    }

    #[test]
    fn fresh_install_reads_none() {
        assert_eq!(svc().get().unwrap(), None, "None signals the migration path");
    }

    #[test]
    fn set_then_get_round_trips_arbitrary_json() {
        let s = svc();
        let doc = json!({
            "v": 2,
            "tabs": [{ "id": "orchestrator", "kind": "orchestrator" }],
            "activeTab": "tab3",
            "scroll": { "plain": { "a1:docs/x.md": 640 } },
            "fromTheFuture": { "nested": [1, 2, 3] }
        });
        s.set(&doc).unwrap();
        assert_eq!(s.get().unwrap(), Some(doc), "passthrough — schema is the frontend's");
    }

    #[test]
    fn second_set_overwrites_the_one_row() {
        let s = svc();
        s.set(&json!({ "v": 2, "activeTab": "tab1" })).unwrap();
        s.set(&json!({ "v": 2, "activeTab": "tab9" })).unwrap();
        assert_eq!(
            s.get().unwrap(),
            Some(json!({ "v": 2, "activeTab": "tab9" })),
            "upsert replaces, never duplicates"
        );
    }

    #[test]
    fn malformed_row_reads_none() {
        let s = svc();
        {
            let c = s.db.lock().unwrap();
            c.execute(
                "INSERT INTO workspace (key, value) VALUES ('workspace', 'not json {')",
                [],
            )
            .unwrap();
        }
        assert_eq!(
            s.get().unwrap(),
            None,
            "garbage must never error or block startup"
        );
    }
}
