use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Manager as _, Runtime};
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
    /// True when per-file line counts were computed (A2.2). Counts-only scans
    /// carry add/del = 0 — the frontend must not treat those as authoritative
    /// totals for the sidebar counters.
    pub full: bool,
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

type ScanFn = Box<dyn Fn() -> ScanResult + Send>;
type EmitFn = Box<dyn FnMut(ScanResult) + Send>;

/// The scan+emit half of an agent registration. Behind its own Arc<Mutex> so
/// the debounce thread can run a (slow) scan without holding the project's
/// state lock — the notify OS-callback thread must never wait on a scan.
struct AgentRun {
    scan: ScanFn,
    emit: EmitFn,
    /// fingerprint of the last emitted scan; None = nothing emitted yet, so
    /// the registration scan always ships (the frontend's startup state).
    last_fp: Option<u64>,
}

/// One agent registered on a project watcher.
struct AgentReg {
    /// This agent's routing roots: its worktree plus its private gitdir
    /// (`…/.git/worktrees/<n>/` for a linked worktree). Events are routed to
    /// the agent whose root is the LONGEST prefix of the event path.
    roots: Vec<PathBuf>,
    run: Arc<Mutex<AgentRun>>,
}

/// A pending (not yet scanned) fs burst for one agent.
#[derive(Clone, Copy)]
struct Burst {
    first: Instant,
    last: Instant,
}

/// A burst that is due IMMEDIATELY (used for the registration scan and the
/// focus-reveal rescan): backdated past both debounce deadlines.
fn immediate_burst(now: Instant) -> Burst {
    Burst {
        first: now.checked_sub(MAX_BURST).unwrap_or(now),
        last: now.checked_sub(SETTLE).unwrap_or(now),
    }
}

#[derive(Default)]
struct ProjectState {
    agents: HashMap<Uuid, AgentReg>,
    /// The BOUNDED event queue: at most ONE entry per agent, no matter how
    /// many fs events arrive — a build writing 100k files coalesces into a
    /// single burst per affected agent instead of accumulating events.
    pending: HashMap<Uuid, Burst>,
    closed: bool,
}

#[derive(Default)]
struct ProjectShared {
    state: Mutex<ProjectState>,
    cv: Condvar,
}

/// One notify watcher instance per PROJECT (keyed by the repo's common gitdir)
/// with N registered watch roots — main workdir, common gitdir, and every
/// worktree, including ones created OUTSIDE the project folder.
struct ProjectWatcher {
    /// Kept alive for its Drop (releases the OS watches). Also used to add
    /// roots as more agents register.
    watcher: notify::RecommendedWatcher,
    /// Roots currently registered on `watcher` (dedup for later registrations).
    roots: Vec<PathBuf>,
    shared: Arc<ProjectShared>,
}

/// Mark fs activity for one agent: extends its open burst or opens a new one.
/// Shared by the notify handler and the tests. Caller notifies the condvar.
fn tick_pending(st: &mut ProjectState, id: Uuid, now: Instant) {
    st.pending
        .entry(id)
        .and_modify(|b| b.last = now)
        .or_insert(Burst { first: now, last: now });
}

/// Longest-prefix route: the agent whose registered root is the longest prefix
/// of `path`. None when no agent root contains the path.
fn route(agents: &HashMap<Uuid, AgentReg>, path: &Path) -> Option<Uuid> {
    let mut best: Option<(usize, Uuid)> = None;
    for (id, reg) in agents {
        for root in &reg.roots {
            if path.starts_with(root) {
                let len = root.as_os_str().len();
                if best.is_none_or(|(l, _)| len > l) {
                    best = Some((len, *id));
                }
            }
        }
    }
    best.map(|(_, id)| id)
}

/// Drop roots that are duplicates or nested under another root (a recursive
/// watch on the parent already covers them). Pure — unit-tested directly.
fn dedup_roots(mut roots: Vec<PathBuf>) -> Vec<PathBuf> {
    roots.sort();
    roots.dedup();
    let mut out: Vec<PathBuf> = Vec::new();
    for r in roots {
        if !out.iter().any(|o| r.starts_with(o)) {
            out.push(r);
        }
    }
    out
}

/// Compute a project key + full watch-root set for an agent worktree:
/// (common gitdir, [worktree, its gitdir, common gitdir, main workdir,
/// every registered worktree]) — deduped. Worktrees registered elsewhere on
/// disk (a configured worktreeRoot outside the project folder) get their own
/// root, so they are covered too. Non-repos key on the path itself.
fn project_key_and_roots(path: &Path) -> (PathBuf, Vec<PathBuf>) {
    match git2::Repository::open(path) {
        Ok(repo) => {
            let common = repo.commondir().to_path_buf();
            let mut roots = vec![
                path.to_path_buf(),
                repo.path().to_path_buf(),
                common.clone(),
            ];
            if let Ok(main) = git2::Repository::open(&common) {
                if let Some(wd) = main.workdir() {
                    roots.push(wd.to_path_buf());
                }
                if let Ok(names) = main.worktrees() {
                    for n in names.iter() {
                        let Ok(Some(n)) = n else { continue };
                        if let Ok(wt) = main.find_worktree(n) {
                            roots.push(wt.path().to_path_buf());
                        }
                    }
                }
            }
            (common, dedup_roots(roots))
        }
        Err(_) => (path.to_path_buf(), vec![path.to_path_buf()]),
    }
}

/// Watches each project's root set with ONE notify watcher (A0.4 — watchers
/// scale with projects, not agents); routes settled change bursts to the
/// owning agent by longest-prefix match, scans BACKEND-SIDE (git status +
/// HEAD oid) and pushes the result in `agent://changed` — the frontend never
/// pulls in steady state.
pub struct WatchService {
    /// project key (common gitdir) → its single watcher + debounce thread.
    projects: Mutex<HashMap<PathBuf, ProjectWatcher>>,
    /// agent id → project key (for unwatch/rescan).
    agent_index: Mutex<HashMap<Uuid, PathBuf>>,
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
            projects: Mutex::new(HashMap::new()),
            agent_index: Mutex::new(HashMap::new()),
            scan_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Watch `path` for `id`, replacing that agent's previous registration.
    /// `scan` computes the pushed payload; runs on the project debounce thread.
    pub fn watch<R: Runtime>(
        &self,
        app: AppHandle<R>,
        id: Uuid,
        path: PathBuf,
        scan: impl Fn() -> ScanResult + Send + 'static,
    ) {
        let ids = id.to_string();
        let history_wt = path.clone();
        self.watch_with_emit(id, path, scan, move |s| {
            // B4.4: every settled burst snapshots the dirty files' CURRENT
            // content into local history (content-addressed — repeat bursts of
            // identical content cost nothing). Runs on the debounce thread.
            if let Some(history) = app.try_state::<crate::history::HistoryService>() {
                let paths: Vec<String> = s.changes.iter().map(|c| c.path.clone()).collect();
                let _ = history.snapshot(id, &history_wt, &paths, "watch");
            }
            let _ = crate::core::emit::emit_keyed(
                &app,
                "agent://changed",
                Some(ids.as_str()),
                &serde_json::json!({ "id": ids, "changes": s.changes, "head": s.head, "countsFull": s.full }),
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
        self.unwatch(id); // drop any previous registration for this agent

        // Why: even when fs watching cannot start (missing dir, watcher error),
        // push one scan so the UI gets a definitive state — otherwise the
        // changes badge would stay unknown forever.
        if !path.is_dir() {
            let mut emit = emit;
            std::thread::spawn(move || emit(scan()));
            return;
        }

        let (key, project_roots) = project_key_and_roots(&path);
        let mut projects = self.projects.lock().unwrap();

        if !projects.contains_key(&key) {
            let shared = Arc::new(ProjectShared::default());
            let handler_shared = Arc::clone(&shared);
            let handler_common = key.clone();
            // The notify handler runs on the OS callback thread — keep it
            // cheap: drop transient git-metadata noise, route the rest to the
            // owning agent's pending burst. The project debounce thread does
            // the scanning and pushing.
            let handler = move |res: notify::Result<notify::Event>| {
                let Ok(event) = res else { return };
                let now = Instant::now();
                let mut st = handler_shared.state.lock().unwrap();
                if st.closed {
                    return;
                }
                let mut ticked = false;
                let tick_all = |st: &mut ProjectState| {
                    let ids: Vec<Uuid> = st.agents.keys().copied().collect();
                    for aid in ids {
                        tick_pending(st, aid, now);
                    }
                };
                if event.paths.is_empty() {
                    // fail open: an event with no paths ticks every agent
                    tick_all(&mut st);
                    ticked = true;
                } else {
                    for p in event.paths.iter().filter(|p| is_scan_relevant(p)) {
                        match route(&st.agents, p) {
                            Some(aid) => {
                                tick_pending(&mut st, aid, now);
                                ticked = true;
                            }
                            // Unrouted events under the COMMON gitdir (shared
                            // refs/HEAD moved by any worktree) may affect any
                            // agent — tick all. Unrouted events elsewhere
                            // (main-checkout content) have no agent surface.
                            None if p.starts_with(&handler_common) => {
                                tick_all(&mut st);
                                ticked = true;
                            }
                            None => {}
                        }
                    }
                }
                drop(st);
                if ticked {
                    handler_shared.cv.notify_all();
                }
            };
            match notify::recommended_watcher(handler) {
                Ok(watcher) => {
                    let loop_shared = Arc::clone(&shared);
                    let loop_lock = Arc::clone(&self.scan_lock);
                    std::thread::spawn(move || {
                        project_loop(loop_shared, SETTLE, MAX_BURST, loop_lock);
                    });
                    projects.insert(
                        key.clone(),
                        ProjectWatcher {
                            watcher,
                            roots: Vec::new(),
                            shared,
                        },
                    );
                }
                Err(_) => {
                    // No watcher possible — one definitive scan (see above).
                    let mut emit = emit;
                    std::thread::spawn(move || emit(scan()));
                    return;
                }
            }
        }

        let pw = projects.get_mut(&key).expect("inserted above");
        // Register any not-yet-covered roots on the project watcher. Failures
        // (root missing on disk) are per-root best-effort: the agent still gets
        // its registration scan below, and its own worktree root was verified
        // `is_dir` above.
        for root in &project_roots {
            if pw.roots.iter().any(|r| root.starts_with(r)) {
                continue;
            }
            if pw.watcher.watch(root, RecursiveMode::Recursive).is_ok() {
                pw.roots.push(root.clone());
            }
        }

        // This agent's ROUTING roots: its worktree + its private gitdir (a
        // linked worktree's commits/checkouts touch only `.git/worktrees/<n>/`
        // in the main repo — already handled today; keep routing them here).
        let mut agent_roots = vec![path.clone()];
        if let Ok(repo) = git2::Repository::open(&path) {
            let gitdir = repo.path().to_path_buf();
            if !gitdir.starts_with(&path) {
                agent_roots.push(gitdir);
            }
        }

        {
            let mut st = pw.shared.state.lock().unwrap();
            st.agents.insert(
                id,
                AgentReg {
                    roots: agent_roots,
                    run: Arc::new(Mutex::new(AgentRun {
                        scan: Box::new(scan),
                        emit: Box::new(emit),
                        last_fp: None,
                    })),
                },
            );
            // Registration scan: due immediately (the frontend's startup state).
            st.pending.insert(id, immediate_burst(Instant::now()));
        }
        pw.shared.cv.notify_all();
        self.agent_index.lock().unwrap().insert(id, key);
    }

    /// Stop watching one agent's worktree (e.g. when it is removed). Dropping
    /// the LAST agent of a project tears the whole project watcher down.
    pub fn unwatch(&self, id: Uuid) {
        let Some(key) = self.agent_index.lock().unwrap().remove(&id) else {
            return;
        };
        let mut projects = self.projects.lock().unwrap();
        let Some(pw) = projects.get_mut(&key) else {
            return;
        };
        let empty = {
            let mut st = pw.shared.state.lock().unwrap();
            st.agents.remove(&id);
            st.pending.remove(&id);
            st.agents.is_empty()
        };
        if empty {
            if let Some(pw) = projects.remove(&key) {
                pw.shared.state.lock().unwrap().closed = true;
                pw.shared.cv.notify_all();
                drop(pw); // drops the notify watcher → OS watches released
            }
        }
    }

    /// Force a fresh scan+emit for one agent, bypassing the fingerprint
    /// suppression. The A2.2 reveal path: background scans were counts-only,
    /// so on focus the UI needs one full-detail push even when the file SET
    /// did not change.
    pub fn rescan(&self, id: Uuid) {
        let Some(key) = self.agent_index.lock().unwrap().get(&id).cloned() else {
            return;
        };
        let projects = self.projects.lock().unwrap();
        let Some(pw) = projects.get(&key) else {
            return;
        };
        let mut st = pw.shared.state.lock().unwrap();
        if let Some(reg) = st.agents.get(&id) {
            reg.run.lock().unwrap().last_fp = None; // next scan always emits
            st.pending.insert(id, immediate_burst(Instant::now()));
        }
        drop(st);
        pw.shared.cv.notify_all();
    }
}

/// Why: agent CLIs run git constantly; every op churns transient git metadata
/// (index.lock create/delete, reflog appends, loose-object writes — and each of
/// those bumps the gitdir directory's own mtime, which notify reports as an
/// event on the gitdir root). Scanning on that noise would turn each agent git
/// call into a full status walk — the fingerprint only suppresses the push, the
/// scan CPU is already spent. Worktree content (no `.git` segment) is ALWAYS
/// relevant — that side fails open. Gitdir paths fail CLOSED against a
/// whitelist: the file set that can change what a scan reports is bounded
/// (index, HEAD-ish refs, packed-refs, refs/**), and everything else — logs/,
/// objects/, tmp files, the gitdir-root mtime churn — is per-git-op noise a
/// status scan cannot observe. Segment-based so it covers both plain repos
/// (`<wt>/.git/…`) and linked-worktree gitdirs (`….git/worktrees/<n>/…`)
/// without path canonicalization.
fn is_scan_relevant(path: &Path) -> bool {
    let comps: Vec<&std::ffi::OsStr> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s),
            _ => None,
        })
        .collect();
    let Some(git_at) = comps.iter().rposition(|s| *s == ".git") else {
        return true; // plain worktree content — always relevant
    };
    let mut rest: &[&std::ffi::OsStr] = &comps[git_at + 1..];
    // linked-worktree gitdir: `.git/worktrees/<name>/<rest…>`
    if rest.first().is_some_and(|s| *s == "worktrees") {
        rest = if rest.len() >= 2 { &rest[2..] } else { &[] };
    }
    // empty rest = the gitdir root itself (directory-mtime churn) — noise
    let Some(first) = rest.first() else {
        return false;
    };
    if path.extension().is_some_and(|e| e == "lock") {
        return false; // index.lock / refs/….lock — mid-operation transients
    }
    matches!(
        first.to_str(),
        Some("index" | "HEAD" | "ORIG_HEAD" | "MERGE_HEAD" | "FETCH_HEAD" | "packed-refs" | "refs")
    )
}

/// The project debounce thread: waits until an agent's burst settles (no event
/// for `settle`, capped at `max_burst` for sustained activity), then scans that
/// agent and emits ONLY when the result fingerprint changed — ignored-file
/// noise (build artifacts) settles to an identical scan and wakes nothing
/// downstream. One thread per project, any number of agents.
fn project_loop(
    shared: Arc<ProjectShared>,
    settle: Duration,
    max_burst: Duration,
    scan_lock: Arc<Mutex<()>>,
) {
    loop {
        // Collect due agents (their run handles) under the state lock, but run
        // the scans OUTSIDE it — the notify handler must never wait on a scan.
        let due: Vec<Arc<Mutex<AgentRun>>> = {
            let mut st = shared.state.lock().unwrap();
            loop {
                if st.closed {
                    return;
                }
                let now = Instant::now();
                let mut due_ids: Vec<Uuid> = Vec::new();
                let mut next_due: Option<Duration> = None;
                for (aid, b) in st.pending.iter() {
                    let deadline = std::cmp::min(b.last + settle, b.first + max_burst);
                    if deadline <= now {
                        due_ids.push(*aid);
                    } else {
                        let d = deadline - now;
                        next_due = Some(next_due.map_or(d, |n: Duration| n.min(d)));
                    }
                }
                if !due_ids.is_empty() {
                    let mut due = Vec::new();
                    for aid in due_ids {
                        st.pending.remove(&aid);
                        if let Some(reg) = st.agents.get(&aid) {
                            due.push(Arc::clone(&reg.run));
                        }
                    }
                    break due;
                }
                st = match next_due {
                    Some(d) => shared.cv.wait_timeout(st, d).unwrap().0,
                    None => shared.cv.wait(st).unwrap(),
                };
            }
        };
        for run in due {
            let mut run = run.lock().unwrap();
            let s = {
                // Why the lock: scans are git2 status walks — serializing them
                // keeps N busy agents from hammering the disk concurrently;
                // each agent still scans at most ~1/s (debounce above).
                let _serial = scan_lock.lock().unwrap();
                (run.scan)()
            };
            let fp = fingerprint(&s);
            if run.last_fp != Some(fp) {
                run.last_fp = Some(fp);
                (run.emit)(s);
            }
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
            full: true,
        }
    }

    /// Drive project_loop with one agent whose scans are scripted (the last
    /// result repeats); returns (shared, agent id, emitted scans, handle).
    fn run_project_loop(
        results: Vec<ScanResult>,
    ) -> (
        Arc<ProjectShared>,
        Uuid,
        Arc<Mutex<Vec<ScanResult>>>,
        std::thread::JoinHandle<()>,
    ) {
        let shared = Arc::new(ProjectShared::default());
        let id = Uuid::new_v4();
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let e = emitted.clone();
        let calls = AtomicUsize::new(0);
        {
            let mut st = shared.state.lock().unwrap();
            st.agents.insert(
                id,
                AgentReg {
                    roots: vec![PathBuf::from("/wt")],
                    run: Arc::new(Mutex::new(AgentRun {
                        scan: Box::new(move || {
                            let i = calls.fetch_add(1, Ordering::SeqCst);
                            results[i.min(results.len() - 1)].clone()
                        }),
                        emit: Box::new(move |s| e.lock().unwrap().push(s)),
                        last_fp: None,
                    })),
                },
            );
            // registration scan: due immediately
            st.pending.insert(id, immediate_burst(Instant::now()));
        }
        let loop_shared = Arc::clone(&shared);
        let h = std::thread::spawn(move || {
            project_loop(
                loop_shared,
                Duration::from_millis(40),
                Duration::from_secs(10),
                Arc::new(Mutex::new(())),
            );
        });
        shared.cv.notify_all();
        (shared, id, emitted, h)
    }

    fn tick(shared: &Arc<ProjectShared>, id: Uuid) {
        tick_pending(&mut shared.state.lock().unwrap(), id, Instant::now());
        shared.cv.notify_all();
    }

    fn close(shared: &Arc<ProjectShared>) {
        shared.state.lock().unwrap().closed = true;
        shared.cv.notify_all();
    }

    #[test]
    fn initial_scan_emits_without_any_fs_tick() {
        let (shared, _id, emitted, h) = run_project_loop(vec![sr(&["a.txt"], "h1")]);
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(
            emitted.lock().unwrap().len(),
            1,
            "registration pushes the current state"
        );
        close(&shared);
        h.join().unwrap();
    }

    #[test]
    fn unchanged_rescan_is_suppressed() {
        let (shared, id, emitted, h) =
            run_project_loop(vec![sr(&["a.txt"], "h1"), sr(&["a.txt"], "h1")]);
        std::thread::sleep(Duration::from_millis(80)); // initial scan emitted
        tick(&shared, id);
        std::thread::sleep(Duration::from_millis(160)); // settle + rescan
        assert_eq!(
            emitted.lock().unwrap().len(),
            1,
            "identical scan result must not re-emit"
        );
        close(&shared);
        h.join().unwrap();
    }

    #[test]
    fn changed_files_emit_again() {
        let (shared, id, emitted, h) =
            run_project_loop(vec![sr(&["a.txt"], "h1"), sr(&["a.txt", "b.txt"], "h1")]);
        std::thread::sleep(Duration::from_millis(80));
        tick(&shared, id);
        std::thread::sleep(Duration::from_millis(160));
        assert_eq!(emitted.lock().unwrap().len(), 2, "new file → new push");
        close(&shared);
        h.join().unwrap();
    }

    #[test]
    fn head_only_move_emits() {
        let (shared, id, emitted, h) =
            run_project_loop(vec![sr(&["a.txt"], "h1"), sr(&["a.txt"], "h2")]);
        std::thread::sleep(Duration::from_millis(80));
        tick(&shared, id);
        std::thread::sleep(Duration::from_millis(160));
        let scans = emitted.lock().unwrap();
        assert_eq!(scans.len(), 2, "HEAD move alone is a real change");
        assert_eq!(scans[1].head.as_deref(), Some("h2"));
        drop(scans);
        close(&shared);
        h.join().unwrap();
    }

    #[test]
    fn burst_of_many_ticks_coalesces_to_one_pending_entry() {
        // The BOUNDED queue: 100k events collapse to one Burst per agent.
        let mut st = ProjectState::default();
        let id = Uuid::new_v4();
        let t0 = Instant::now();
        for _ in 0..100_000 {
            tick_pending(&mut st, id, Instant::now());
        }
        assert_eq!(st.pending.len(), 1, "one entry per agent, not per event");
        let b = st.pending[&id];
        assert!(b.first >= t0 && b.last >= b.first, "burst window tracked");
    }

    #[test]
    fn dedup_roots_drops_nested_and_duplicate_roots() {
        let roots = vec![
            PathBuf::from("/main"),
            PathBuf::from("/main/.git"),          // nested → covered by /main
            PathBuf::from("/main"),               // duplicate
            PathBuf::from("/elsewhere/wt_a"),     // outside worktree → kept
            PathBuf::from("/elsewhere/wt_a/sub"), // nested under kept root
            PathBuf::from("/elsewhere/wt_b"),
        ];
        let out = dedup_roots(roots);
        assert_eq!(
            out,
            vec![
                PathBuf::from("/elsewhere/wt_a"),
                PathBuf::from("/elsewhere/wt_b"),
                PathBuf::from("/main"),
            ]
        );
    }

    #[test]
    fn project_key_and_roots_covers_outside_worktrees() {
        let git = crate::git::service::GitService::new();
        let main = tempfile::tempdir().unwrap();
        git.init(main.path()).unwrap();
        git.ensure_main_branch(main.path()).unwrap();

        // worktree OUTSIDE the project folder (its own temp dir)
        let wt_root = tempfile::tempdir().unwrap();
        let wt = wt_root.path().join("agent_out");
        git.create_worktree(main.path(), "agent_out", "agent/out", None, &wt)
            .unwrap();

        let (key, roots) = project_key_and_roots(&wt);
        let common = git2::Repository::open(&wt).unwrap().commondir().to_path_buf();
        assert_eq!(key, common, "project key is the COMMON gitdir");
        assert!(
            roots.iter().any(|r| wt.starts_with(r)),
            "outside worktree covered: {roots:?}"
        );
        // canonicalize both sides: git2 may hand back canonicalized paths while
        // tempfile hands back the raw ones (8.3 / verbatim prefixes on Windows)
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let main_canon = canon(main.path());
        assert!(
            roots.iter().any(|r| main_canon.starts_with(canon(r))),
            "main workdir covered: {roots:?}"
        );
        // both agents of one project share one key → ONE watcher instance
        let (key2, _) = project_key_and_roots(main.path());
        assert_eq!(key, key2, "main checkout and worktree share the project key");
    }

    #[test]
    fn route_picks_longest_prefix_owner() {
        let mut agents = HashMap::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        agents.insert(
            a,
            AgentReg {
                roots: vec![PathBuf::from("/w"), PathBuf::from("/m/.git/worktrees/a")],
                run: Arc::new(Mutex::new(AgentRun {
                    scan: Box::new(|| sr(&[], "h")),
                    emit: Box::new(|_| {}),
                    last_fp: None,
                })),
            },
        );
        agents.insert(
            b,
            AgentReg {
                roots: vec![PathBuf::from("/w/deeper"), PathBuf::from("/m/.git/worktrees/b")],
                run: Arc::new(Mutex::new(AgentRun {
                    scan: Box::new(|| sr(&[], "h")),
                    emit: Box::new(|_| {}),
                    last_fp: None,
                })),
            },
        );
        assert_eq!(route(&agents, Path::new("/w/src/x.rs")), Some(a));
        assert_eq!(
            route(&agents, Path::new("/w/deeper/src/x.rs")),
            Some(b),
            "longest prefix wins over the shorter containing root"
        );
        assert_eq!(route(&agents, Path::new("/m/.git/worktrees/b/index")), Some(b));
        assert_eq!(route(&agents, Path::new("/m/.git/refs/heads/x")), None);
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
                    full: true,
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
                full: true,
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
    fn two_agents_of_one_project_share_one_watcher_instance() {
        let git = crate::git::service::GitService::new();
        let main = tempfile::tempdir().unwrap();
        git.init(main.path()).unwrap();
        git.ensure_main_branch(main.path()).unwrap();

        let wt_root = tempfile::tempdir().unwrap();
        let wt_a = wt_root.path().join("agent_a");
        let wt_b = wt_root.path().join("agent_b");
        git.create_worktree(main.path(), "agent_a", "agent/a", None, &wt_a)
            .unwrap();
        git.create_worktree(main.path(), "agent_b", "agent/b", None, &wt_b)
            .unwrap();

        let svc = WatchService::new();
        let (id_a, id_b) = (Uuid::new_v4(), Uuid::new_v4());
        svc.watch_with_emit(id_a, wt_a, || sr(&[], "h"), |_| {});
        svc.watch_with_emit(id_b, wt_b, || sr(&[], "h"), |_| {});
        assert_eq!(
            svc.projects.lock().unwrap().len(),
            1,
            "one watcher per PROJECT, not per agent"
        );
        svc.unwatch(id_a);
        assert_eq!(svc.projects.lock().unwrap().len(), 1, "project alive while an agent remains");
        svc.unwatch(id_b);
        assert!(
            svc.projects.lock().unwrap().is_empty(),
            "last agent gone → project watcher torn down"
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
                full: true,
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
