use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use std::fs;
use tauri::Manager;

#[derive(Default)]
pub struct Database {}

pub type DB = Arc<Mutex<Connection>>;
pub type ID = uuid::Uuid;

impl Database {
    pub fn get(app: &tauri::App) -> DB {
        let dir = app.path().app_data_dir().expect("no app data dir");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("orrery.db");
        let conn = Connection::open(path).unwrap();
        // Why -2000: 2MB page cache (negative = KiB). Orrery's tables are tiny
        // (agents/projects/tickets rows) so the SQLite default buys nothing —
        // set it modestly instead of taking the default (A0.6). Best-effort:
        // a failing PRAGMA must not block startup.
        let _ = conn.pragma_update(None, "cache_size", -2000);
        Arc::new(Mutex::new(conn))
    }
}
