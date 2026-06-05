use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Runtime};
use uuid::Uuid;

/// Watches every agent's worktree concurrently and emits `agent://changed` on
/// edits. One watcher per agent (keyed by id) so background agents stay live —
/// not just the one currently on screen.
pub struct WatchService {
    watchers: Mutex<HashMap<Uuid, notify::RecommendedWatcher>>,
}

impl Default for WatchService {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchService {
    pub fn new() -> Self {
        Self { watchers: Mutex::new(HashMap::new()) }
    }

    /// Watch `path` for `id`, replacing that agent's previous watcher (if any).
    /// Other agents' watchers are untouched.
    pub fn watch<R: Runtime>(&self, app: AppHandle<R>, id: Uuid, path: PathBuf) {
        let mut guard = self.watchers.lock().unwrap();
        guard.remove(&id); // drop this agent's previous watcher → stops it
        if !path.is_dir() {
            return;
        }

        let app = app.clone();
        let ids = id.to_string();
        let last = Mutex::new(Instant::now() - Duration::from_secs(1));
        let handler = move |res: notify::Result<notify::Event>| {
            if res.is_err() {
                return;
            }
            // leading-edge debounce: at most one event per 250ms
            let mut l = last.lock().unwrap();
            if l.elapsed() < Duration::from_millis(250) {
                return;
            }
            *l = Instant::now();
            let _ = app.emit("agent://changed", serde_json::json!({ "id": ids }));
        };

        if let Ok(mut watcher) = notify::recommended_watcher(handler) {
            if watcher.watch(&path, RecursiveMode::Recursive).is_ok() {
                guard.insert(id, watcher);
            }
        }
    }

    /// Stop watching one agent's worktree (e.g. when it is removed).
    pub fn unwatch(&self, id: Uuid) {
        self.watchers.lock().unwrap().remove(&id);
    }
}
