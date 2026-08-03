use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Runtime};
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
#[derive(Clone)]
pub struct ScanResult {
    pub changes: Vec<FileChange>,
    pub head: Option<String>,
}

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

/// Watches every agent's worktree concurrently; on settled change bursts it
/// scans the worktree BACKEND-SIDE (git status + HEAD oid) and pushes the
/// result in `agent://changed` — the frontend never pulls in steady state.
pub struct WatchService {
    watchers: Mutex<HashMap<Uuid, notify::RecommendedWatcher>>,
    // Why: scans are git2 status walks — serializing them keeps N busy agents
    // from hammering the disk concurrently; each agent still scans ≤~1/s.
    scan_lock: Arc<Mutex<()>>,
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
            scan_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Watch `path` for `id`, replacing that agent's previous watcher (if any).
    /// `scan` computes the pushed payload; runs on the agent's debounce thread.
    pub fn watch<R: Runtime>(
        &self,
        app: AppHandle<R>,
        id: Uuid,
        path: PathBuf,
        scan: impl Fn() -> ScanResult + Send + 'static,
    ) {
        let ids = id.to_string();
        self.watch_with_emit(id, path, scan, move |s| {
            let _ = crate::core::emit::emit_keyed(
                &app,
                "agent://changed",
                Some(ids.as_str()),
                &serde_json::json!({ "id": ids, "changes": s.changes, "head": s.head }),
            );
        });
    }

    fn watch_with_emit(
        &self,
        id: Uuid,
        path: PathBuf,
        scan: impl Fn() -> ScanResult + Send + 'static,
        emit: impl FnMut(ScanResult) + Send + 'static,
    ) {
        let mut guard = self.watchers.lock().unwrap();
        guard.remove(&id); // drop previous watcher → stops it (+ ends its scan thread)

        // Why: even when fs watching cannot start (missing dir, watcher error),
        // push one scan so the UI gets a definitive state — otherwise the
        // changes badge would stay unknown forever.
        let register = || -> Option<(notify::RecommendedWatcher, std::sync::mpsc::Receiver<()>)> {
            if !path.is_dir() {
                return None;
            }

            // The notify handler is on the OS callback thread — keep it cheap:
            // drop transient git-metadata noise, forward a tick for the rest.
            // The dedicated thread debounces, scans, and pushes.
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let handler = move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    // fail open: an event with no paths still ticks the debounce
                    if event.paths.is_empty() || event.paths.iter().any(|p| is_scan_relevant(p)) {
                        let _ = tx.send(());
                    }
                }
            };
            let Ok(mut watcher) = notify::recommended_watcher(handler) else {
                return None;
            };
            if watcher.watch(&path, RecursiveMode::Recursive).is_err() {
                return None;
            }
            // Why: a linked worktree's gitdir (HEAD, index, refs) lives under the
            // MAIN repo's .git/worktrees/<name>/ — a commit or checkout touches only
            // that dir, so without watching it an agent's own `git commit` would
            // never refresh the changes badge or commits feed. A plain repo's .git
            // sits inside `path` and is already covered by the recursive watch.
            if let Ok(repo) = git2::Repository::open(&path) {
                let gitdir = repo.path().to_path_buf();
                if !gitdir.starts_with(&path) {
                    let _ = watcher.watch(&gitdir, RecursiveMode::Recursive);
                }
            }
            Some((watcher, rx))
        };

        match register() {
            None => {
                // Registration failed — emit one definitive scan so the UI is not stale.
                let mut emit = emit;
                std::thread::spawn(move || {
                    emit(scan());
                });
            }
            Some((watcher, rx)) => {
                guard.insert(id, watcher);
                let scan_lock = Arc::clone(&self.scan_lock);
                std::thread::spawn(move || {
                    scan_loop(rx, SETTLE, MAX_BURST, scan_lock, scan, emit);
                });
            }
        }
    }

    /// Stop watching one agent's worktree (e.g. when it is removed).
    pub fn unwatch(&self, id: Uuid) {
        self.watchers.lock().unwrap().remove(&id);
    }
}

/// Why: agent CLIs run git constantly; every op churns transient git metadata
/// (index.lock create/delete, reflog appends, loose-object writes). Scanning on
/// that noise would turn each agent git call into a full status walk — the
/// fingerprint only suppresses the push, the scan CPU is already spent. Only
/// git metadata that can change what a scan reports (index, HEAD, refs) earns a
/// tick; regular worktree content always does. Segment-based so it covers both
/// plain repos (`<wt>/.git/…`) and linked-worktree gitdirs
/// (`….git/worktrees/<n>/…`) without path canonicalization, and fails OPEN:
/// unrecognized shapes count as relevant.
fn is_scan_relevant(path: &Path) -> bool {
    let mut in_git_metadata = false;
    for component in path.components() {
        let segment = component.as_os_str();
        if segment == ".git" {
            in_git_metadata = true;
            continue;
        }
        if in_git_metadata && (segment == "logs" || segment == "objects") {
            return false;
        }
    }
    !(in_git_metadata && path.extension().is_some_and(|e| e == "lock"))
}

/// Drive the scan-and-push loop: scan once at registration (the frontend's
/// startup state), then per settled fs burst re-scan and emit ONLY when the
/// result fingerprint changed — ignored-file noise (build artifacts) settles
/// to an identical scan and wakes nothing downstream.
fn scan_loop(
    rx: Receiver<()>,
    settle: Duration,
    max_burst: Duration,
    scan_lock: Arc<Mutex<()>>,
    scan: impl Fn() -> ScanResult,
    mut emit: impl FnMut(ScanResult),
) {
    let mut last = {
        let s = {
            let _serial = scan_lock.lock().unwrap();
            scan()
        };
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
        let s = {
            // Why the lock: scans are git2 status walks — serializing them keeps N
            // busy agents from hammering the disk concurrently; each agent still
            // scans at most ~1/s (debounce above).
            let _serial = scan_lock.lock().unwrap();
            scan()
        };
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
        let (tx, emitted, h) = run_scan_loop(vec![sr(&["a.txt"], "h1"), sr(&["a.txt"], "h1")]);
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
        let (tx, emitted, h) = run_scan_loop(vec![sr(&["a.txt"], "h1"), sr(&["a.txt"], "h2")]);
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

    #[test]
    fn scan_relevance_filters_transient_git_metadata() {
        // transient gitdir churn (every agent `git status/add/commit`) — ignored
        assert!(!is_scan_relevant(Path::new(
            "/m/.git/worktrees/a/index.lock"
        )));
        assert!(!is_scan_relevant(Path::new("/m/.git/index.lock")));
        assert!(!is_scan_relevant(Path::new(
            "/m/.git/worktrees/a/logs/HEAD"
        )));
        assert!(!is_scan_relevant(Path::new("/m/.git/objects/ab/cdef0123")));
        // git state that changes what a scan reports — relevant
        assert!(is_scan_relevant(Path::new("/m/.git/worktrees/a/index")));
        assert!(is_scan_relevant(Path::new("/m/.git/worktrees/a/HEAD")));
        assert!(is_scan_relevant(Path::new("/m/.git/refs/heads/agent/x")));
        // worktree content — always relevant, including lock-NAMED project files
        assert!(is_scan_relevant(Path::new("/wt/src/main.rs")));
        assert!(is_scan_relevant(Path::new("/wt/yarn.lock")));
        assert!(is_scan_relevant(Path::new("/wt/Cargo.lock")));
    }

    #[test]
    fn transient_gitdir_churn_does_not_rescan() {
        let git = crate::git::service::GitService::new();
        let main = tempfile::tempdir().unwrap();
        git.init(main.path()).unwrap();
        git.ensure_main_branch(main.path()).unwrap();

        let wt_root = tempfile::tempdir().unwrap();
        let wt = wt_root.path().join("agent_churn");
        git.create_worktree(main.path(), "agent_churn", "agent/churn", None, &wt)
            .unwrap();
        let gitdir = git2::Repository::open(&wt).unwrap().path().to_path_buf();

        let scans = Arc::new(AtomicUsize::new(0));
        let scan_count = scans.clone();
        let svc = WatchService::new();
        svc.watch_with_emit(
            Uuid::new_v4(),
            wt.clone(),
            move || {
                scan_count.fetch_add(1, Ordering::SeqCst);
                ScanResult {
                    changes: Vec::new(),
                    head: Some("h".into()),
                }
            },
            |_s| {},
        );

        let wait_for_scans = |n: usize| {
            for _ in 0..100 {
                if scans.load(Ordering::SeqCst) >= n {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            false
        };
        assert!(wait_for_scans(1), "initial scan");

        // simulate agent-side git ops: index.lock create/delete cycles, reflog
        // appends, loose-object writes — none of it changes scan output
        for _ in 0..3 {
            std::fs::write(gitdir.join("index.lock"), b"x").unwrap();
            std::thread::sleep(Duration::from_millis(30));
            let _ = std::fs::remove_file(gitdir.join("index.lock"));
            std::thread::sleep(Duration::from_millis(30));
        }
        std::fs::create_dir_all(gitdir.join("logs")).unwrap();
        std::fs::write(gitdir.join("logs").join("HEAD"), b"reflog line\n").unwrap();
        std::thread::sleep(Duration::from_millis(800)); // > SETTLE; debounce would fire
        assert_eq!(
            scans.load(Ordering::SeqCst),
            1,
            "transient git metadata churn must not trigger scans"
        );

        // control: a real worktree edit still scans (watcher alive, not over-filtered)
        std::fs::write(wt.join("hello.txt"), "hi\n").unwrap();
        assert!(wait_for_scans(2), "real content change still scans");
    }

    #[test]
    fn missing_worktree_still_pushes_one_scan() {
        let emitted: Arc<Mutex<Vec<ScanResult>>> = Arc::new(Mutex::new(Vec::new()));
        let e = emitted.clone();
        let svc = WatchService::new();
        svc.watch_with_emit(
            Uuid::new_v4(),
            PathBuf::from("Z:/definitely/not/a/dir"),
            || ScanResult {
                changes: Vec::new(),
                head: None,
            },
            move |s| e.lock().unwrap().push(s),
        );
        for _ in 0..50 {
            if !emitted.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            emitted.lock().unwrap().len(),
            1,
            "one definitive scan despite no watcher"
        );
    }

    #[test]
    fn watcher_pushes_scan_on_file_edit_and_worktree_commit() {
        let git = crate::git::service::GitService::new();
        let main = tempfile::tempdir().unwrap();
        git.init(main.path()).unwrap();
        git.ensure_main_branch(main.path()).unwrap(); // first commit to branch from

        let wt_root = tempfile::tempdir().unwrap();
        let wt = wt_root.path().join("agent_x");
        git.create_worktree(main.path(), "agent_x", "agent/x", None, &wt)
            .unwrap();

        let emitted: Arc<Mutex<Vec<ScanResult>>> = Arc::new(Mutex::new(Vec::new()));
        let e = emitted.clone();
        let svc = WatchService::new();
        let scan_git = git.clone();
        let scan_path = wt.clone();
        svc.watch_with_emit(
            uuid::Uuid::new_v4(),
            wt.clone(),
            move || ScanResult {
                changes: scan_git.status(&scan_path),
                head: scan_git.head_oid(&scan_path),
            },
            move |s| e.lock().unwrap().push(s),
        );

        // fs watcher latency varies by platform/CI — poll generously
        let wait_for = |n: usize| {
            for _ in 0..100 {
                if emitted.lock().unwrap().len() >= n {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            false
        };

        assert!(wait_for(1), "initial scan push on registration");
        let head0 = emitted.lock().unwrap()[0].head.clone();
        assert!(head0.is_some(), "worktree HEAD known at registration");

        std::fs::write(wt.join("hello.txt"), "hi\n").unwrap();
        assert!(wait_for(2), "file edit pushes a scan");
        assert!(
            emitted.lock().unwrap()[1]
                .changes
                .iter()
                .any(|c| c.path == "hello.txt"),
            "pushed scan carries the new file"
        );

        git.commit(&wt, "from agent", &[]).unwrap();
        assert!(
            wait_for(3),
            "a commit (gitdir-only fs activity) pushes a scan"
        );
        let last = emitted.lock().unwrap().last().unwrap().clone();
        assert_ne!(last.head, head0, "HEAD moved");
        assert!(last.changes.is_empty(), "worktree clean after commit-all");
    }
}
