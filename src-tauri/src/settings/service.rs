use rusqlite::OptionalExtension;

use crate::core::database::DB;
use crate::core::errors::{AppResult, DbError};

use super::model::Settings;

/// The one row's key. A key/value table (instead of a single-column table) so a
/// later need for more documents (e.g. per-project overrides) is a new key, not
/// a migration.
const KEY: &str = "settings";

#[derive(Clone)]
pub struct SettingsService {
    db: DB,
}

impl SettingsService {
    pub fn new(db: DB) -> Self {
        let svc = Self { db };
        svc.init_schema();
        svc
    }

    fn init_schema(&self) {
        let c = self.db.lock().unwrap();
        c.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .unwrap();
    }

    /// The persisted settings. A missing row is a fresh install → `Default`; a
    /// malformed document (manual edit, partial write) also falls back to
    /// `Default` rather than erroring — settings must never block startup. The
    /// struct's `#[serde(default)]` covers the missing/unknown-key cases, so
    /// only outright invalid JSON hits the fallback.
    pub fn get(&self) -> AppResult<Settings> {
        let raw: Option<String> = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [KEY],
                |r| r.get(0),
            )
            .optional()
            .map_err(DbError::Sqlite)?
        };
        Ok(raw
            .and_then(|s| {
                serde_json::from_str(&s)
                    .inspect_err(|e| log::warn!("settings row malformed, using defaults: {e}"))
                    .ok()
            })
            .unwrap_or_default())
    }

    /// Persist the whole document (upsert the one row).
    pub fn set(&self, settings: &Settings) -> AppResult<()> {
        let json = serde_json::to_string(settings)
            .map_err(|e| crate::core::errors::AppError::Other(format!("settings encode: {e}")))?;
        let c = self.db.lock().unwrap();
        c.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
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
    use std::sync::{Arc, Mutex};

    fn svc() -> SettingsService {
        let db: DB = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        SettingsService::new(db)
    }

    #[test]
    fn fresh_install_reads_defaults() {
        assert_eq!(svc().get().unwrap(), Settings::default());
    }

    #[test]
    fn set_then_get_round_trips() {
        let s = svc();
        let mut want = Settings::default();
        want.channel = "beta".into();
        want.branch_template = "{name}/{tool}/{date}".into();
        want.auto_approve.insert("claude".into(), "everything".into());
        want.volume = 15;
        s.set(&want).unwrap();
        assert_eq!(s.get().unwrap(), want);
    }

    #[test]
    fn second_set_overwrites_the_one_row() {
        let s = svc();
        let mut first = Settings::default();
        first.channel = "beta".into();
        s.set(&first).unwrap();
        let mut second = Settings::default();
        second.channel = "stable".into();
        second.sound = false;
        s.set(&second).unwrap();
        assert_eq!(s.get().unwrap(), second, "upsert replaces, never duplicates");
    }

    #[test]
    fn malformed_row_falls_back_to_defaults() {
        let s = svc();
        {
            let c = s.db.lock().unwrap();
            c.execute(
                "INSERT INTO settings (key, value) VALUES ('settings', 'not json {')",
                [],
            )
            .unwrap();
        }
        assert_eq!(
            s.get().unwrap(),
            Settings::default(),
            "garbage must never error or block startup"
        );
    }

    #[test]
    fn unknown_keys_in_stored_doc_are_ignored() {
        let s = svc();
        {
            let c = s.db.lock().unwrap();
            c.execute(
                "INSERT INTO settings (key, value)
                 VALUES ('settings', '{\"channel\":\"beta\",\"fromTheFuture\":true}')",
                [],
            )
            .unwrap();
        }
        let got = s.get().unwrap();
        assert_eq!(got.channel, "beta");
        assert_eq!(got.update_policy, "auto", "missing keys default");
    }
}
