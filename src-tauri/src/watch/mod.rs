use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Runtime};
use uuid::Uuid;

/// Quiet period: the worktree must see no fs events for this long before a burst
/// is considered settled and one `agent://changed` is emitted. Trailing-edge — we
/// wait for the move/write to FINISH so the UI scans the final state, not a
/// transient mid-operation one.
const SETTLE: Duration = Duration::from_millis(200);
/// Cap on a single coalesced burst: during sustained activity (e.g. a long write)
/// we still emit at least this often so the UI stays live.
const MAX_BURST: Duration = Duration::from_millis(1000);

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
        guard.remove(&id); // drop this agent's previous watcher → stops it (+ ends its debounce thread)
        if !path.is_dir() {
            return;
        }

        // The notify handler is on the OS callback thread — keep it trivial: just
        // forward a tick. A dedicated thread coalesces the burst (trailing-edge).
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let handler = move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        };
        let Ok(mut watcher) = notify::recommended_watcher(handler) else {
            return;
        };
        if watcher.watch(&path, RecursiveMode::Recursive).is_err() {
            return;
        }
        guard.insert(id, watcher);

        // Trailing-edge debounce thread. Ends when the watcher (and thus the
        // Sender) is dropped on re-watch / unwatch → `rx` disconnects.
        let ids = id.to_string();
        std::thread::spawn(move || {
            debounce_loop(rx, SETTLE, MAX_BURST, || {
                let _ = app.emit("agent://changed", serde_json::json!({ "id": ids }));
            });
        });
    }

    /// Stop watching one agent's worktree (e.g. when it is removed).
    pub fn unwatch(&self, id: Uuid) {
        self.watchers.lock().unwrap().remove(&id);
    }
}

/// Drive a debounce over `rx`: block for the first event, then `emit` once the
/// stream has been quiet for `settle` (or the burst exceeds `max_burst`). Repeats
/// per burst; returns when the `Sender` is dropped.
fn debounce_loop(rx: Receiver<()>, settle: Duration, max_burst: Duration, mut emit: impl FnMut()) {
    while rx.recv().is_ok() {
        // a burst started — coalesce until the stream is quiet for `settle`, so the
        // emit (and the frontend re-scan it triggers) sees the SETTLED filesystem,
        // not a transient mid-move state. `max_burst` caps sustained activity so a
        // long stream of writes still refreshes periodically.
        let burst_start = Instant::now();
        loop {
            match rx.recv_timeout(settle) {
                Ok(()) => {
                    if burst_start.elapsed() >= max_burst {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => break, // quiet → settled
                Err(RecvTimeoutError::Disconnected) => return, // watcher dropped
            }
        }
        emit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn run(settle: Duration) -> (std::sync::mpsc::Sender<()>, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let h = std::thread::spawn(move || {
            debounce_loop(rx, settle, Duration::from_secs(10), move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        });
        (tx, count, h)
    }

    #[test]
    fn waits_for_quiet_then_emits_once_per_burst() {
        let (tx, count, h) = run(Duration::from_millis(60));
        // a burst of rapid events, like a file move (rename-from/to + dir events)
        for _ in 0..5 {
            tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(8));
        }
        // trailing-edge: must NOT have emitted yet (still within the settle window)
        assert_eq!(count.load(Ordering::SeqCst), 0, "no emit until the worktree goes quiet");
        std::thread::sleep(Duration::from_millis(140)); // let it settle
        assert_eq!(count.load(Ordering::SeqCst), 1, "exactly one emit after the burst settles");
        drop(tx); // disconnect → thread exits
        h.join().unwrap();
    }

    #[test]
    fn a_later_burst_emits_again() {
        let (tx, count, h) = run(Duration::from_millis(60));
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(140)); // settle → emit #1
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(140)); // settle → emit #2
        assert_eq!(count.load(Ordering::SeqCst), 2, "a second burst triggers a second refresh");
        drop(tx);
        h.join().unwrap();
    }
}
