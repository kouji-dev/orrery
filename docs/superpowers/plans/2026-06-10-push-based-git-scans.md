# Push-Based Git Scans Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Invert the worktree-refresh flow from ping→pull to push-with-change-detection so `agent_changes` is never pulled in steady state and `agent_commits` refreshes only when HEAD actually moves.

**Architecture:** The per-agent watcher debounce thread in `src-tauri/src/watch/mod.rs` stops emitting a contentless ping and instead runs the git scan itself (status + HEAD oid), suppresses no-op results via fingerprint, and pushes the data in the `agent://changed` event. It also watches the worktree's *gitdir* (`main/.git/worktrees/<name>/`) so agent-driven commits — which touch no watched file today — trigger pushes too. The frontend store adopts pushed scans (`applyScan`), refreshes commits only on HEAD movement, and drops all eager/post-action pulls.

**Tech Stack:** Rust (notify 6, git2, tauri 2), Angular 20 signals, vitest, cargo test.

**Key facts (verified):**
- `watch/mod.rs:70` emits `agent://changed` `{id}` ping; frontend pulls `agent_changes` per ping.
- Linked-worktree commits write only to the main repo's `.git/worktrees/<name>/` — **outside the watched path** — so today the watcher never fires on an agent's own `git commit`; the UI relies on manual post-action pulls.
- `agent-runtime.service.ts:114` eagerly pulls changes for every agent at startup.
- `agent-actions.service.ts:84-85,97` pulls changes + refreshes commits after commit/discard.
- Event name `agent://changed` is kept; payload becomes `{id, changes, head}`.
- The pull command `agent_changes` stays registered — diff-view (`diff-view.component.ts:311`) still pulls on open.

**File map:**
- Modify: `src-tauri/src/watch/mod.rs` — `ScanResult`, `fingerprint`, `scan_loop` (replaces `debounce_loop`), gitdir watch, `watch_with_emit`
- Modify: `src-tauri/src/git/service.rs` — `head_oid()`
- Modify: `src-tauri/src/agents/service.rs` — `scan()`
- Modify: `src-tauri/src/agents/commands.rs` — `agent_watch` passes scan closure
- Modify: `src/app/agents/agent-work.store.ts` — `applyScan()`, remove `onWorktreeChanged()`
- Modify: `src/app/stores/agents.store.ts` — `onScan()` replaces `onWorktreeChanged()`
- Modify: `src/app/agents/agent-runtime.service.ts` — consume pushes, drop eager pull
- Modify: `src/app/agents/agent-actions.service.ts` — drop post-action pulls
- Test: `src-tauri/src/watch/mod.rs` (in-module), `src-tauri/src/git/service.rs` (in-module), `src/app/agents/agent-work.store.spec.ts`

---

### Task 1: `ScanResult` + fingerprint + `scan_loop` (Rust core)

**Files:**
- Modify: `src-tauri/src/watch/mod.rs`

- [ ] **Step 1: Write the failing tests**

Replace the existing `mod tests` block in `src-tauri/src/watch/mod.rs` (the two `debounce_loop` tests die with `debounce_loop` in the next step) with:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml watch::`
Expected: FAIL to compile — `ScanResult`, `scan_loop`, `FileChange` not found in `watch`.

- [ ] **Step 3: Implement `ScanResult`, `fingerprint`, `scan_loop`**

In `src-tauri/src/watch/mod.rs`: add imports, the type, and replace `debounce_loop` with `scan_loop`.

Update the import block at the top to:

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Runtime};
use uuid::Uuid;

use crate::git::service::FileChange;
```

Add after the `MAX_BURST` const:

```rust
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
```

Add `scan_loop` BELOW the existing `debounce_loop`. Do NOT delete or modify `debounce_loop` or `watch` in this task — they keep the app's current ping behavior intact until Task 3 swaps the wiring (at which point `debounce_loop` is deleted). Its two old tests are already replaced by the new suite above; two commits without direct coverage on a function about to be deleted is fine.

```rust
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
```

NOTE: the import change (`use std::sync::{Arc, Mutex};`) is compatible with the existing `debounce_loop`/`watch` code, which only used `Mutex`. If `cargo clippy` flags `scan_loop`/`ScanResult`/`fingerprint` as dead code in this intermediate state, add `#[allow(dead_code)]` on `scan_loop` with a `// Why: wired up in the next commit (watch push migration)` comment and remove it in Task 3.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml watch::`
Expected: 4 tests PASS. (Known repo flake: if an unrelated test fails sporadically, rerun once.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/watch/mod.rs
git commit -m "feat(watch): scan loop with fingerprint suppression

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: `GitService::head_oid` + `AgentService::scan`

**Files:**
- Modify: `src-tauri/src/git/service.rs`
- Modify: `src-tauri/src/agents/service.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/git/service.rs` (uses the existing `commit_file` helper in that module):

```rust
    #[test]
    fn head_oid_none_unborn_then_moves_on_commit() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        assert!(svc.head_oid(dir.path()).is_none(), "unborn HEAD");
        commit_file(dir.path(), "a.txt", "first");
        let h1 = svc.head_oid(dir.path()).unwrap();
        commit_file(dir.path(), "b.txt", "second");
        let h2 = svc.head_oid(dir.path()).unwrap();
        assert_ne!(h1, h2, "HEAD moves on commit");
        assert_eq!(h2.len(), 40, "full oid hex, not short sha");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml head_oid`
Expected: FAIL to compile — no method `head_oid`.

- [ ] **Step 3: Implement `head_oid` and `scan`**

In `src-tauri/src/git/service.rs`, add to `impl GitService` (next to `head_info`):

```rust
    /// Full HEAD oid hex, or None for an unborn HEAD / non-repo. Cheap (one ref
    /// read) — callers use it to detect "did HEAD move" without walking history.
    pub fn head_oid(&self, path: &Path) -> Option<String> {
        Repository::open(path)
            .ok()?
            .head()
            .ok()?
            .target()
            .map(|o| o.to_string())
    }
```

In `src-tauri/src/agents/service.rs`, add to `impl AgentService` (next to `changes`):

```rust
    /// One worktree scan for the watcher push: working-tree changes + HEAD oid.
    /// A missing agent (mid-removal race) scans empty — its watcher is about to
    /// be dropped anyway. Composition of tested pieces; covered end-to-end by
    /// the watch integration test.
    pub fn scan(&self, id: Uuid) -> crate::watch::ScanResult {
        let Ok(rec) = self.get(id) else {
            return crate::watch::ScanResult {
                changes: Vec::new(),
                head: None,
            };
        };
        let p = Path::new(&rec.worktree);
        crate::watch::ScanResult {
            changes: self.git.status(p),
            head: self.git.head_oid(p),
        }
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml head_oid`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/git/service.rs src-tauri/src/agents/service.rs
git commit -m "feat(git): head_oid + agent worktree scan source

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Wire the watcher to scan + push (incl. gitdir watch) and update `agent_watch`

**Files:**
- Modify: `src-tauri/src/watch/mod.rs:42-73` (the `watch` method)
- Modify: `src-tauri/src/agents/commands.rs:385-397` (`agent_watch`)

- [ ] **Step 1: Write the failing integration test**

Add to `mod tests` in `src-tauri/src/watch/mod.rs`. This is the feature's end-to-end test: a real notify watcher over a real linked git worktree, asserting (a) the initial push, (b) a push on file edit, and (c) a push when a commit happens — which only touches the gitdir outside the worktree.

```rust
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
            Uuid::new_v4(),
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml watcher_pushes`
Expected: FAIL to compile — no method `watch_with_emit`.

- [ ] **Step 3: Implement `watch_with_emit`, rewrite `watch`, update struct**

In `src-tauri/src/watch/mod.rs`, replace the `WatchService` struct, `new`, and `watch` with:

```rust
/// Watches every agent's worktree concurrently; on settled change bursts it
/// scans the worktree BACKEND-SIDE (git status + HEAD oid) and pushes the
/// result in `agent://changed` — the frontend never pulls in steady state.
pub struct WatchService {
    watchers: Mutex<HashMap<Uuid, notify::RecommendedWatcher>>,
    // Why: scans are git2 status walks — serializing them keeps N busy agents
    // from hammering the disk concurrently; each agent still scans ≤~1/s.
    scan_lock: Arc<Mutex<()>>,
}
```

```rust
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
            let _ = app.emit(
                "agent://changed",
                serde_json::json!({ "id": ids, "changes": s.changes, "head": s.head }),
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
        if !path.is_dir() {
            return;
        }

        // The notify handler is on the OS callback thread — keep it trivial: just
        // forward a tick. The dedicated thread debounces, scans, and pushes.
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
        guard.insert(id, watcher);

        let scan_lock = Arc::clone(&self.scan_lock);
        std::thread::spawn(move || {
            scan_loop(rx, SETTLE, MAX_BURST, scan_lock, scan, emit);
        });
    }

    /// Stop watching one agent's worktree (e.g. when it is removed).
    pub fn unwatch(&self, id: Uuid) {
        self.watchers.lock().unwrap().remove(&id);
    }
}
```

(With this rewrite: DELETE `debounce_loop` entirely — `scan_loop` replaces it — and remove any temporary `#[allow(dead_code)]` from Task 1. The old module doc comment on lines 20-22 is deleted too; the struct doc above replaces it.)

In `src-tauri/src/agents/commands.rs`, replace `agent_watch`:

```rust
/// Start watching an agent's worktree (replaces any previous watch). The
/// watcher scans backend-side and pushes `agent://changed` {id, changes, head}.
#[tauri::command(async)]
pub fn agent_watch<R: Runtime>(
    app: AppHandle<R>,
    watch: State<'_, WatchService>,
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<()> {
    crate::perf::timed("agent_watch", || {
        let worktree = svc.get(id)?.worktree;
        let scan_svc = svc.inner().clone();
        watch.watch(app, id, std::path::PathBuf::from(worktree), move || {
            scan_svc.scan(id)
        });
        Ok(())
    })
}
```

- [ ] **Step 4: Run the full Rust suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: ALL PASS, including `watcher_pushes_scan_on_file_edit_and_worktree_commit` (give it time — it polls up to 10s per phase). Then `cargo fmt --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`.
Expected: fmt clean, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/watch/mod.rs src-tauri/src/agents/commands.rs
git commit -m "feat(watch): push scan results; watch gitdir so agent commits refresh

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Frontend store — `applyScan` replaces `onWorktreeChanged`

**Files:**
- Modify: `src/app/agents/agent-work.store.ts`
- Test: `src/app/agents/agent-work.store.spec.ts`

- [ ] **Step 1: Write the failing tests**

In `src/app/agents/agent-work.store.spec.ts`, DELETE the test `"onWorktreeChanged reloads changes always, tree only when previously loaded"` (lines 89-102) and add:

```ts
  it("applyScan stores ready changes with zero bridge calls", () => {
    store.applyScan("a", [{ path: "x", add: 1, del: 0, state: "M" }], "h1");
    expect(store.changesFor("a").status).toBe("ready");
    expect(store.changesFor("a").data[0].path).toBe("x");
    expect(invokes.length).toBe(0);
  });

  it("applyScan reloads tree only when previously loaded", async () => {
    store.applyScan("a", [], "h1");
    expect(invokes.length).toBe(0); // tree idle → no pull
    store.ensureTree("a");
    resolvers.shift()!([]);
    await Promise.resolve();
    store.applyScan("a", [], "h1");
    expect(invokes.map((i) => i.cmd)).toEqual([Commands.AgentTree, Commands.AgentTree]);
  });

  it("applyScan refreshes commits only on a HEAD move, and only when loaded", async () => {
    store.applyScan("a", [], "h1"); // commits idle → nothing
    store.ensureCommits("a");
    resolvers.shift()!([commit("s1")]);
    await Promise.resolve();
    store.applyScan("a", [], "h1"); // same head → no refresh
    expect(invokes.filter((i) => i.cmd === Commands.AgentCommits).length).toBe(1);
    store.applyScan("a", [], "h2"); // head moved → refresh
    expect(invokes.filter((i) => i.cmd === Commands.AgentCommits).length).toBe(2);
  });

  it("a late pull resolve cannot stomp fresher pushed data", async () => {
    store.loadChanges("a");
    const stale = resolvers.shift()!;
    store.applyScan("a", [{ path: "pushed", add: 1, del: 0, state: "A" }], "h1");
    stale([{ path: "stale", add: 9, del: 9, state: "M" }]);
    await Promise.resolve();
    expect(store.changesFor("a").data[0].path).toBe("pushed");
  });
```

Note: the AgentFile literals need the `state` typed value; if TS complains, write them as `{ path: "x", add: 1, del: 0, state: "M" as const }`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `pnpm test`
Expected: FAIL — `applyScan` does not exist on `AgentWorkStore`.

- [ ] **Step 3: Implement `applyScan`, delete `onWorktreeChanged`**

In `src/app/agents/agent-work.store.ts`:

Add a field next to the gen guards (line ~31):

```ts
  // last pushed HEAD oid per agent — commits refresh only when it moves
  private lastHead: Record<string, string | null> = {};
```

DELETE the `onWorktreeChanged` method (lines 139-144) and add in its place:

```ts
  /** Backend watcher push: adopt the scanned changes; commits refresh only when
   *  HEAD actually moved (and the feed was ever opened); tree reloads only when
   *  previously loaded. Replaces the old ping → pull (`onWorktreeChanged`). */
  applyScan(id: string, changes: AgentFile[], head: string | null): void {
    // supersede any in-flight pull so its late resolve can't stomp fresher push data
    this.changesGen[id] = (this.changesGen[id] ?? 0) + 1;
    this.patch(this.changesMap, id, { status: "ready", data: changes });
    const moved = id in this.lastHead && this.lastHead[id] !== head;
    this.lastHead[id] = head;
    if (moved) this.refreshCommits(id);
    if (this.treeFor(id).status !== "idle") this.loadTree(id);
  }
```

In `dispose(id)`, add alongside the gen deletions:

```ts
    delete this.lastHead[id];
```

Also update the class doc comment (lines 13-18): replace the sentence `Lazy: changes load eagerly per agent at startup; tree/commits on first agent open.` with `Changes arrive as backend watcher pushes (applyScan); tree/commits stay lazy — on first agent open.`

- [ ] **Step 4: Run the tests to verify they pass**

Run: `pnpm test`
Expected: ALL PASS (the four new tests green; no other spec references `onWorktreeChanged` on the store except `agent-runtime.service.ts` wiring, fixed next task — vitest only compiles specs, but if the suite typechecks service files and fails on the missing method, proceed to Task 5 and run the suite there; commit both tasks together in that case).

- [ ] **Step 5: Commit**

```bash
git add src/app/agents/agent-work.store.ts src/app/agents/agent-work.store.spec.ts
git commit -m "feat(work-store): adopt backend scan pushes (applyScan)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Wire the push end-to-end; delete eager + post-action pulls

**Files:**
- Modify: `src/app/stores/agents.store.ts:113-120`
- Modify: `src/app/agents/agent-runtime.service.ts:107-128`
- Modify: `src/app/agents/agent-actions.service.ts:77-100`

- [ ] **Step 1: Replace `onWorktreeChanged` with `onScan` in the agents store**

In `src/app/stores/agents.store.ts`, replace the `onWorktreeChanged` method (lines 117-120) with:

```ts
  /** Subscribe to backend worktree scan pushes — the watcher computes the
   *  changes + HEAD oid and ships them with the notification (no pull needed). */
  onScan(
    cb: (p: { id: string; changes: AgentFile[]; head: string | null }) => void,
  ): Promise<() => void> {
    return this.bridge.on<{ id: string; changes: AgentFile[]; head: string | null }>(
      Events.AgentChanged,
      cb,
    );
  }
```

(`AgentFile` is already imported in this file for `changes()`.)

- [ ] **Step 2: Update the runtime service wiring**

In `src/app/agents/agent-runtime.service.ts`, replace lines 107-128 (the two effects + subscription block) with:

```ts
    // Watch EVERY agent's worktree — the backend watcher runs the initial scan
    // and pushes results (changes + HEAD), so no eager pull is needed here.
    // Tree + commits stay lazy — ensured on first open below.
    effect(() => {
      for (const a of this.agentsStore.all()) {
        if (this.watched.has(a.id)) continue;
        this.watched.add(a.id);
        void this.agentsStore.watch(a.id).catch(() => {});
      }
    });
    // Lazy: first time an agent becomes the active/scoped one, load its tree +
    // first commits page (no-ops when already loaded).
    effect(() => {
      const ag = this.activeAgent();
      if (!ag) return;
      this.work.ensureTree(ag.id);
      this.work.ensureCommits(ag.id);
    });
    void this.agentsStore
      .onScan((p) => this.work.applyScan(p.id, p.changes, p.head))
      .catch(() => {});
```

- [ ] **Step 3: Delete the post-action pulls**

In `src/app/agents/agent-actions.service.ts`:

`commitAgent` (lines 78-90) — remove the two `this.work.*` lines; the `.then` becomes:

```ts
      .then(() => {
        this.ui.flash("committed in " + (ag?.name ?? id));
        // changes badge + commits feed refresh arrive via the watcher push —
        // the commit touches the worktree's gitdir, which is watched.
        void this.projects.refreshCommits(this.projects.all().map((p) => p.id));
        if (ag) this.runtime.patchRuntime(id, { commits: ag.commits + 1 });
      })
```

`discardAgent` (lines 92-100) — remove `this.work.loadChanges(id);`; the `.then` becomes:

```ts
      .then(() => {
        // the checkout rewrites worktree files → the watcher pushes the rescan
        this.ui.flash("discarded changes");
      })
```

- [ ] **Step 4: Run the full frontend suite + typecheck**

Run: `pnpm test`
Expected: ALL PASS.
Run: `pnpm run build`
Expected: builds clean (catches template/DI type errors vitest misses). If `this.work` became unused in `agent-actions.service.ts`, remove the injection and its import.

- [ ] **Step 5: Commit**

```bash
git add src/app/stores/agents.store.ts src/app/agents/agent-runtime.service.ts src/app/agents/agent-actions.service.ts
git commit -m "feat(agents): consume scan pushes; drop eager and post-action pulls

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: End-to-end verification in the running app

**Files:** none (verification only)

- [ ] **Step 1: Full suites once more**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && pnpm test`
Expected: all green.

- [ ] **Step 2: Live smoke in the app**

Run: `pnpm dev` and verify, with at least one agent:
1. Startup: changes badges populate WITHOUT any `agent_changes` rows appearing in the DevConsole perf panel (the initial scan is backend-side; the panel's exec table should show `agent_watch` only).
2. Edit a file in an agent's worktree → badge updates within ~1s; perf panel still shows no `agent_changes` invocations.
3. Open the agent's git tab, run a commit from the UI → changes list clears and the commits feed gains the new commit (via gitdir push), with no `agent_commits` call until the push lands.
4. Let the agent itself `git commit` in its terminal → badge + feed update (this NEVER worked via the watcher before — gitdir coverage is new behavior).
5. Touch a build-artifact/ignored file → no UI churn (fingerprint suppression).

- [ ] **Step 3: Record the profiling note**

Append a dated section to `docs/superpowers/plans/2026-06-10-perf-roadmap.md` under technique #10 marking it DONE with the observed before/after `agent_changes` call counts from the perf panel.

- [ ] **Step 4: Commit (docs)**

```bash
git add docs/superpowers/plans/2026-06-10-perf-roadmap.md
git commit -m "docs(perf): mark push-based git scans done with observed call counts

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Non-goals (explicitly out of scope)

- **No commits caching** — per design discussion, the lever is calling less, not memoizing.
- **No summary/full payload split by visibility** — v1 pushes the full `FileChange[]` to everyone; pushes are already suppressed when nothing changed. Split later only if scans of huge diffs measurably hurt.
- **No per-commit stats deferral** — `GitService::log` already avoids `.stats()`; the delta count via tree diff is cheap at page size 10.
- **No new `agent_rescan` command** — gitdir watching covers commit/discard triggers naturally.

## Risks & notes

- The integration test depends on real fs-watcher latency; it polls up to 10s per phase. The repo has a known cargo-test flake — rerun once before investigating.
- `agent://changed` keeps its name but the payload grows `{changes, head}`; the only consumer (`agents.store.ts`) is updated in the same change set.
- Windows: notify's `ReadDirectoryChangesW` backend handles both watch roots (worktree + gitdir) fine; the gitdir watch is skipped automatically for plain repos (gitdir inside the watched path).
