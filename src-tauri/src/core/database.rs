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
        let path = dir.join("katrix.db");
        Arc::new(Mutex::new(Connection::open(path).unwrap()))
    }
}
