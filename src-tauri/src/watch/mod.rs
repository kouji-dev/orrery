use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Runtime};
use uuid::Uuid;

use crate::git::service::FileChange;

/// Quiet period: the worktree must see no fs events for this long before a burst
/// is considered settled and one `agent://changed` is emitted. Trailing-edge — we
/// wait for the move/write to FINISH so the UI scans the final state, not a
/// transient mid-operation one.
const SETTLE: Duration = Duration::from_millis(200);
/// Cap on a single coalesced burst: during sustained activity (e.g. a long write)
/// we still emit at least this often so the UI stays live.
const MAX_BURST: Duration = Duration::from_millis(1000);

/// One scan push: the worktree's working-tree changes plus its HEAD oid
/// (None = unborn repo). Computed backend-side so the frontend never pulls.
#[allow(dead_code)] // Why: wired up in the next commit (watch push migration)
#[derive(Clone)]
pub struct ScanResult {
    pub changes: Vec<FileChange>,
    pub head: Option<String>,
}

#[allow(dead_code)] // Why: wired up in the next commit (watch push migration)
fn fingerprint(s: &ScanResult) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.head.hash(&mut h);
    for c in &s.changes {
        c.path.hash(&mut h);
        c.add.hash(&mut h);
        c.del.hash(&mut h);
        c.state.hash(&mut h);
        c.old_path.hash(&mut h);
    }
    h.finish()
}

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
        Self {
            watchers: Mutex::new(HashMap::new()),
        }
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

/// Drive the scan-and-push loop: scan once at registration (the frontend's
/// startup state), then per settled fs burst re-scan and emit ONLY when the
/// result fingerprint changed — ignored-file noise (build artifacts) settles
/// to an identical scan and wakes nothing downstream.
#[allow(dead_code)] // Why: wired up in the next commit (watch push migration)
fn scan_loop(
    rx: Receiver<()>,
    settle: Duration,
    max_burst: Duration,
    scan_lock: Arc<Mutex<()>>,
    scan: impl Fn() -> ScanResult,
    mut emit: impl FnMut(ScanResult),
) {
    let mut last = {
        let _serial = scan_lock.lock().unwrap();
        let s = scan();
        let fp = fingerprint(&s);
        emit(s);
        fp
    };
    while rx.recv().is_ok() {
        // a burst started — coalesce until the stream is quiet for `settle`, so
        // the scan sees the SETTLED filesystem, not a transient mid-move state.
        // `max_burst` caps sustained activity so long writes still refresh.
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
        // Why the lock: scans are git2 status walks — serializing them keeps N
        // busy agents from hammering the disk concurrently; each agent still
        // scans at most ~1/s (debounce above).
        let _serial = scan_lock.lock().unwrap();
        let s = scan();
        let fp = fingerprint(&s);
        if fp != last {
            last = fp;
            emit(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn fc(path: &str) -> FileChange {
        FileChange {
            path: path.into(),
            add: 1,
            del: 0,
            state: "M".into(),
            old_path: None,
        }
    }

    fn sr(paths: &[&str], head: &str) -> ScanResult {
        ScanResult {
            changes: paths.iter().map(|p| fc(p)).collect(),
            head: Some(head.into()),
        }
    }

    /// Drive scan_loop with a scripted sequence of scan results (the last one
    /// repeats); returns (fs-tick sender, emitted scans, thread handle).
    fn run_scan_loop(
        results: Vec<ScanResult>,
    ) -> (
        std::sync::mpsc::Sender<()>,
        Arc<Mutex<Vec<ScanResult>>>,
        std::thread::JoinHandle<()>,
    ) {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let e = emitted.clone();
        let calls = AtomicUsize::new(0);
        let h = std::thread::spawn(move || {
            scan_loop(
                rx,
                Duration::from_millis(40),
                Duration::from_secs(10),
                Arc::new(Mutex::new(())),
                move || {
                    let i = calls.fetch_add(1, Ordering::SeqCst);
                    results[i.min(results.len() - 1)].clone()
                },
                move |s| e.lock().unwrap().push(s),
            );
        });
        (tx, emitted, h)
    }

    #[test]
    fn initial_scan_emits_without_any_fs_tick() {
        let (tx, emitted, h) = run_scan_loop(vec![sr(&["a.txt"], "h1")]);
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(
            emitted.lock().unwrap().len(),
            1,
            "registration pushes the current state"
        );
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn unchanged_rescan_is_suppressed() {
        let (tx, emitted, h) =
            run_scan_loop(vec![sr(&["a.txt"], "h1"), sr(&["a.txt"], "h1")]);
        std::thread::sleep(Duration::from_millis(80)); // initial scan emitted
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(160)); // settle + rescan
        assert_eq!(
            emitted.lock().unwrap().len(),
            1,
            "identical scan result must not re-emit"
        );
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn changed_files_emit_again() {
        let (tx, emitted, h) =
            run_scan_loop(vec![sr(&["a.txt"], "h1"), sr(&["a.txt", "b.txt"], "h1")]);
        std::thread::sleep(Duration::from_millis(80));
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(160));
        assert_eq!(emitted.lock().unwrap().len(), 2, "new file → new push");
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn head_only_move_emits() {
        let (tx, emitted, h) =
            run_scan_loop(vec![sr(&["a.txt"], "h1"), sr(&["a.txt"], "h2")]);
        std::thread::sleep(Duration::from_millis(80));
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(160));
        let scans = emitted.lock().unwrap();
        assert_eq!(scans.len(), 2, "HEAD move alone is a real change");
        assert_eq!(scans[1].head.as_deref(), Some("h2"));
        drop(scans);
        drop(tx);
        h.join().unwrap();
    }
}
