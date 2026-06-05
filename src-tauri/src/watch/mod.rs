use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Runtime};
use uuid::Uuid;

/// Watches the *active* agent's worktree and emits `agent://changed` on edits.
/// Only one watch is active at a time (the agent currently being viewed).
pub struct WatchService {
    current: Mutex<Option<(Uuid, notify::RecommendedWatcher)>>,
}

impl Default for WatchService {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchService {
    pub fn new() -> Self {
        Self { current: Mutex::new(None) }
    }

    /// Watch `path` for the given agent, replacing any previous watch (which stops it).
    pub fn watch<R: Runtime>(&self, app: AppHandle<R>, id: Uuid, path: PathBuf) {
        let mut guard = self.current.lock().unwrap();
        *guard = None; // drop the previous watcher → stops watching
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
                *guard = Some((id, watcher));
            }
        }
    }

    pub fn unwatch(&self) {
        *self.current.lock().unwrap() = None;
    }
}
