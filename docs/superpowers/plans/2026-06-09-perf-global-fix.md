# Global Performance Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the 5.2s startup freeze: heavy commands on the blocking pool, commits paginated + cheap, git data in per-agent `Loadable` hashMap stores loaded lazily, and Rust exec metrics to prove it.

**Architecture:** Backend — blanket `(async)` commands (already in tree) + `spawn_blocking` for the measured-heavy set; `log()` gains `offset` and counts files via `deltas().len()` (in tree). Frontend — new `AgentWorkStore` holds `changes`/`commits`/`trees` as `Record<agentId, Loadable<T>>` with entry reference identity; `Agent` loses its git transients; consumers read per-id. New Rust `perf` module pushes `perf://stats` every 2s into the already-built `PerfStore` merge.

**Tech Stack:** Tauri v2 (Rust, git2, tokio), Angular 20 signals, vitest, cargo test.

**Spec:** `docs/superpowers/specs/2026-06-09-perf-global-fix-design.md`
**Branch:** `perf_enhancements` (never push). Working tree already contains: 32 `(async)` attribute swaps + the `deltas().len()` swap in `git/service.rs`.

---

### Task 0: Commit the in-tree S1-a spike

**Files:** none (commits existing changes)

- [ ] **Step 0.1:** Commit the attribute swaps separately from the deltas swap (which Task 1 tests first):

```bash
git add src-tauri/src/agents/commands.rs src-tauri/src/projects/commands.rs src-tauri/src/cost/commands.rs src-tauri/src/metrics/commands.rs src-tauri/src/appicon.rs
git commit -m "perf(commands): run all sync commands off the main thread via #[tauri::command(async)]"
```

(`src-tauri/src/git/service.rs` stays uncommitted until Task 1 adds its tests.)

---

### Task 1: `log()` — exact files-count test, then `offset` pagination

**Files:**
- Modify: `src-tauri/src/git/service.rs` (fn `log` ~line 74; tests module)
- Modify: `src-tauri/src/agents/service.rs:234-250` (fn `commits`)
- Modify: `src-tauri/src/projects/service.rs:211-227` (fn `commits`)

- [ ] **Step 1.1: Write the failing tests** — append to `mod tests` in `git/service.rs`:

```rust
    #[test]
    fn log_files_counts_changed_files_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        // one commit touching two files
        use git2::Signature;
        std::fs::write(dir.path().join("x.txt"), "x\n").unwrap();
        std::fs::write(dir.path().join("y.txt"), "y\n").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("x.txt")).unwrap();
        index.add_path(Path::new("y.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("Test", "t@t").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "two files", &tree, &[&parent]).unwrap();

        let log = svc.log(dir.path(), 10, 0);
        assert_eq!(log[0].files, 2, "two-file commit counts 2");
        assert_eq!(log[1].files, 1, "single-file commit counts 1");
    }

    #[test]
    fn log_offset_pages_through_history() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        for i in 0..5 {
            commit_file(dir.path(), &format!("f{i}.txt"), &format!("c{i}"));
        }
        let p1 = svc.log(dir.path(), 2, 0);
        let p2 = svc.log(dir.path(), 2, 2);
        let p3 = svc.log(dir.path(), 2, 4);
        assert_eq!(p1.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["c4", "c3"]);
        assert_eq!(p2.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["c2", "c1"]);
        assert_eq!(p3.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), ["c0"]);
        assert!(svc.log(dir.path(), 2, 99).is_empty(), "past-the-end offset is empty");
    }
```

- [ ] **Step 1.2: Run to verify failure** (signature mismatch — `log` takes 2 args):

```bash
cargo test --manifest-path src-tauri/Cargo.toml git::service
```
Expected: COMPILE ERROR — `log` takes 2 arguments but 3 were supplied.

- [ ] **Step 1.3: Add `offset` to `log()`** — in `git/service.rs`, change the signature and the walk chain:

```rust
    /// Most recent commits (newest first), `offset`-skipped for paging, or empty
    /// for a repo with no commits / no repo.
    pub fn log(&self, path: &Path, limit: usize, offset: usize) -> Vec<LogEntry> {
```

and:

```rust
        walk.flatten()
            .skip(offset)
            .take(limit)
```

- [ ] **Step 1.4: Thread `offset` through the services.** `agents/service.rs`:

```rust
    pub fn commits(&self, id: Uuid, limit: usize, offset: usize) -> AppResult<Vec<CommitView>> {
        let rec = self.record(id)?;
        Ok(self
            .git
            .log(Path::new(&rec.worktree), limit, offset)
```

`projects/service.rs` (no paging for the project feed — pass 0):

```rust
            .log(Path::new(&rec.path), limit, 0)
```

Fix the two existing call sites in tests/commands that call `log(.., 10)` / `svc.commits(id, limit)` to compile (existing `git/service.rs` tests: add `, 0`; `agents/commands.rs::agent_commits` is updated in Task 2 — for now pass `0`).

- [ ] **Step 1.5: Run tests to verify pass:**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```
Expected: PASS (all suites, including the two new tests).

- [ ] **Step 1.6: Commit:**

```bash
git add src-tauri/src/git/service.rs src-tauri/src/agents/service.rs src-tauri/src/projects/service.rs src-tauri/src/agents/commands.rs
git commit -m "perf(git): files count via deltas (no .stats() blob diffs) + offset paging in log()"
```

---

### Task 2: `agent_commits` offset across the IPC boundary

**Files:**
- Modify: `src-tauri/src/agents/commands.rs:80-87` (`agent_commits`)
- Modify: `src/app/data-source/bridge.ts` (no change needed — command name unchanged)
- Modify: `src/app/stores/agents.store.ts:86-89` (`commits`)

- [ ] **Step 2.1: Backend param** — `agents/commands.rs`:

```rust
#[tauri::command(async)]
pub fn agent_commits(
    svc: State<'_, AgentService>,
    id: Uuid,
    limit: Option<usize>,
    offset: Option<usize>,
) -> AppResult<Vec<crate::projects::model::CommitView>> {
    svc.commits(id, limit.unwrap_or(50), offset.unwrap_or(0))
}
```

- [ ] **Step 2.2: Frontend store method** — `agents.store.ts`:

```ts
  /** Commits on the agent's branch (worktree HEAD log), tagged with the agent id.
   *  Paged: `limit` newest-first commits starting at `offset`. */
  commits(id: string, limit: number, offset = 0): Promise<Commit[]> {
    return this.bridge.invoke<Commit[]>(Commands.AgentCommits, { id, limit, offset });
  }
```

- [ ] **Step 2.3: Compile both sides:**

```bash
cargo check --manifest-path src-tauri/Cargo.toml && npx tsc --noEmit -p tsconfig.json
```
Expected: cargo clean; tsc reports the existing `commits(agentId)` caller in `agent-runtime.service.ts` missing args — acceptable until Task 5 removes that caller. If tsc is not configured standalone, run `npx ng build --configuration development` instead and expect the same single caller error; defer it (Tasks 4–5 replace the caller). If the error blocks the build, temporarily pass `(agentId, 50, 0)` at `agent-runtime.service.ts:259`.

- [ ] **Step 2.4: Commit:**

```bash
git add src-tauri/src/agents/commands.rs src/app/stores/agents.store.ts src/app/agents/agent-runtime.service.ts
git commit -m "feat(commits): offset paging param through agent_commits"
```

---

### Task 3: Heavy commands → `spawn_blocking` (S1-c completion)

**Files:**
- Modify: `src-tauri/src/agents/service.rs:13` + `src-tauri/src/projects/service.rs:32` (derive Clone)
- Modify: `src-tauri/src/agents/commands.rs` (`agent_changes`, `agent_commits`, `agent_tree`, `agent_diff`, `agent_commit`, `agent_discard`, `agent_push`, `agent_spawn`, `agent_remove`)
- Modify: `src-tauri/src/projects/commands.rs` (`project_commits`, `project_create`, `project_init_git`, `project_detect_git`)
- Modify: `src-tauri/src/cost/commands.rs` (`system_cost`)
- Modify: `src-tauri/src/metrics/commands.rs` (`system_metrics`)
- Modify: `src-tauri/src/agents/commands.rs` (`detect_tools`)

- [ ] **Step 3.1: Make services cloneable** (fields are `Arc`-backed `DB`, unit-struct `GitService`, `PathBuf` — all `Clone`):

```rust
#[derive(Clone)]
pub struct AgentService {
```

```rust
#[derive(Clone)]
pub struct ProjectService {
```

- [ ] **Step 3.2: Convert the read-path agent commands.** Pattern (repeated for each — `(async)` attr drops since the fn becomes truly async):

```rust
#[tauri::command]
pub async fn agent_changes(
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<Vec<crate::git::service::FileChange>> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || svc.changes(id))
        .await
        .map_err(|e| AppError::Other(format!("join: {e}")))?
}
```

Apply the identical clone-into-closure shape to: `agent_commits` (move `id, limit, offset` in), `agent_tree` (closure body: `let agent = svc.get(id)?; Ok(crate::fs::tree(std::path::Path::new(&agent.worktree)))`), `agent_diff` (move `id, path, old_path`), `agent_commit`, `agent_discard`, `agent_push`, `detect_tools` (no service — `spawn_blocking(|| super::adapters::installed())`, return `AppResult<Vec<ToolStatus>>`).

- [ ] **Step 3.3: Convert the mutating agent commands** (emit AFTER the await, on the async context):

```rust
#[tauri::command]
pub async fn agent_spawn<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    req: AgentSpawnRequest,
) -> AppResult<Agent> {
    let (svc, projects) = (svc.inner().clone(), projects.inner().clone());
    let agent = tauri::async_runtime::spawn_blocking(move || {
        let project_path = projects.path_of(req.project_id)?;
        svc.spawn(req, std::path::Path::new(&project_path))
            .inspect_err(|e| log::error!("agent_spawn failed: {e:?}"))
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))??;
    emit_entity(&app, "agent", Change::Created, agent.clone());
    Ok(agent)
}
```

`agent_remove` follows the same shape (watch-unwatch + svc.remove inside the closure, `WatchService` is `State` — read `project_path` + call `watch.unwatch(id)` BEFORE the spawn_blocking, only `svc.remove(...)` inside).

- [ ] **Step 3.4: Convert project + system commands.** `project_commits`, `project_create`, `project_init_git`, `project_detect_git` — same clone-into-closure; `project_create`/`project_init_git` emit after await. `system_cost`:

```rust
#[tauri::command]
pub async fn system_cost() -> Result<CostSnapshot, String> {
    tauri::async_runtime::spawn_blocking(snapshot)
        .await
        .map_err(|e| format!("join: {e}"))
}
```

`system_metrics` (State reads happen BEFORE the closure; sampler work inside):

```rust
#[tauri::command]
pub async fn system_metrics(
    runtime: State<'_, RuntimeService>,
    agents: State<'_, AgentService>,
) -> Result<SystemMetrics, String> {
    let pids = runtime.pids();
    let labels = agent_labels(&agents);
    tauri::async_runtime::spawn_blocking(move || {
        let mut sampler = MetricsSampler::new();
        sampler.refresh();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        sampler.refresh();
        let mut metrics = sampler.sample(std::process::id(), &pids);
        for p in &mut metrics.procs {
            if p.id == "app" { continue; }
            if let Ok(uuid) = Uuid::parse_str(&p.id) {
                if let Some(name) = labels.get(&uuid) { p.label = name.clone(); }
            }
        }
        metrics
    })
    .await
    .map_err(|e| format!("join: {e}"))
}
```

(Refactor note: this inlines `sample_with_labels`; keep that helper for the lib.rs push loop, which still uses it.)

- [ ] **Step 3.5: Tests + clippy:**

```bash
cargo test --manifest-path src-tauri/Cargo.toml && cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```
Expected: PASS / no new warnings (pre-existing `serve`/`get` dead-code warnings remain).

- [ ] **Step 3.6: Commit:**

```bash
git add src-tauri/src
git commit -m "perf(commands): heavy git/fs/process commands on the blocking pool (spawn_blocking)"
```

---

### Task 4: `AgentWorkStore` (Loadable hashMaps) + vitest

**Files:**
- Create: `src/app/agents/agent-work.store.ts`
- Create: `src/app/agents/agent-work.store.spec.ts`
- Modify: `src/app/models.ts` (export `Loadable`)

- [ ] **Step 4.1: Write the failing tests** — `agent-work.store.spec.ts`:

```ts
import { TestBed } from "@angular/core/testing";
import { AgentWorkStore, COMMITS_PAGE } from "./agent-work.store";
import { BRIDGE, Commands } from "../data-source/bridge";
import { Commit } from "../models";

function commit(sha: string): Commit {
  return { sha, msg: sha, agent: "a", when: "1m", files: 1 } as unknown as Commit;
}

describe("AgentWorkStore", () => {
  let invokes: Array<{ cmd: string; payload?: Record<string, unknown> }>;
  let resolvers: Array<(v: unknown) => void>;

  beforeEach(() => {
    invokes = [];
    resolvers = [];
    TestBed.configureTestingModule({
      providers: [
        {
          provide: BRIDGE,
          useValue: {
            invoke: (cmd: string, payload?: Record<string, unknown>) => {
              invokes.push({ cmd, payload });
              return new Promise((res) => resolvers.push(res));
            },
            on: () => Promise.resolve(() => {}),
            pickDirectory: () => Promise.resolve(null),
          },
        },
      ],
    });
  });

  it("changes: idle -> loading -> ready, and untouched ids keep reference identity", async () => {
    const store = TestBed.inject(AgentWorkStore);
    expect(store.changesFor("a").status).toBe("idle");
    store.loadChanges("b"); // unrelated entry exists first
    resolvers.shift()!([]);
    await Promise.resolve();
    const bBefore = store.changesFor("b");

    store.loadChanges("a");
    expect(store.changesFor("a").status).toBe("loading");
    resolvers.shift()!([{ path: "x", add: 1, del: 0, state: "M" }]);
    await Promise.resolve();
    expect(store.changesFor("a").status).toBe("ready");
    expect(store.changesFor("a").data.length).toBe(1);
    expect(store.changesFor("b")).toBe(bBefore); // identity preserved
  });

  it("ensureCommits loads page one once; loadMoreCommits appends with offset", async () => {
    const store = TestBed.inject(AgentWorkStore);
    store.ensureCommits("a");
    store.ensureCommits("a"); // second ensure is a no-op
    expect(invokes.filter((i) => i.cmd === Commands.AgentCommits).length).toBe(1);
    expect(invokes[0].payload).toEqual({ id: "a", limit: COMMITS_PAGE, offset: 0 });
    resolvers.shift()!(Array.from({ length: COMMITS_PAGE }, (_, i) => commit(`s${i}`)));
    await Promise.resolve();
    expect(store.commitsFor("a").hasMore).toBe(true);

    store.loadMoreCommits("a");
    expect(invokes[1].payload).toEqual({ id: "a", limit: COMMITS_PAGE, offset: COMMITS_PAGE });
    resolvers.shift()!([commit("tail")]);
    await Promise.resolve();
    expect(store.commitsFor("a").data.length).toBe(COMMITS_PAGE + 1);
    expect(store.commitsFor("a").hasMore).toBe(false); // short page = end
  });

  it("refreshCommits keeps previous rows visible while reloading (SWR)", async () => {
    const store = TestBed.inject(AgentWorkStore);
    store.ensureCommits("a");
    resolvers.shift()!([commit("s1")]);
    await Promise.resolve();
    store.refreshCommits("a");
    expect(store.commitsFor("a").status).toBe("loading");
    expect(store.commitsFor("a").data.length).toBe(1); // stale rows kept
    resolvers.shift()!([commit("s2"), commit("s1")]);
    await Promise.resolve();
    expect(store.commitsFor("a").data.map((c) => c.sha)).toEqual(["s2", "s1"]);
  });

  it("superseded loads are discarded (generation guard)", async () => {
    const store = TestBed.inject(AgentWorkStore);
    store.loadChanges("a");
    const first = resolvers.shift()!;
    store.loadChanges("a"); // supersedes
    resolvers.shift()!([{ path: "new", add: 1, del: 0, state: "A" }]);
    await Promise.resolve();
    first([{ path: "old", add: 9, del: 9, state: "M" }]); // stale resolve arrives late
    await Promise.resolve();
    expect(store.changesFor("a").data[0].path).toBe("new");
  });

  it("onWorktreeChanged reloads changes always, tree only when previously loaded", async () => {
    const store = TestBed.inject(AgentWorkStore);
    store.onWorktreeChanged("a");
    expect(invokes.map((i) => i.cmd)).toEqual([Commands.AgentChanges]); // no tree: idle
    store.ensureTree("a");
    resolvers[1]!([]);
    await Promise.resolve();
    store.onWorktreeChanged("a");
    expect(invokes.map((i) => i.cmd)).toEqual([
      Commands.AgentChanges, Commands.AgentTree, Commands.AgentChanges, Commands.AgentTree,
    ]);
  });
});
```

- [ ] **Step 4.2: Run to verify failure:**

```bash
npx vitest run src/app/agents/agent-work.store.spec.ts
```
Expected: FAIL — cannot resolve `./agent-work.store`.

- [ ] **Step 4.3: Implement the store** — `agent-work.store.ts`:

```ts
import { computed, inject, Injectable, Signal, signal } from "@angular/core";
import { BRIDGE, Commands } from "../data-source/bridge";
import { AgentFile, Commit, FileNode, Loadable } from "../models";

/** Commits page size for the lazy feed (+ "Load more"). */
export const COMMITS_PAGE = 10;

const IDLE: Loadable<never[]> = { status: "idle", data: [] };
const IDLE_COMMITS = { ...IDLE, hasMore: false };

type CommitsEntry = Loadable<Commit[]> & { hasMore: boolean };

/**
 * Per-agent worktree data (git status / branch commits / file tree) as keyed
 * Loadable maps, SEPARATE from the Agent records so one agent's reload never
 * re-renders the others' consumers (entry reference identity is preserved for
 * untouched ids). `idle` means "never requested" — unknown, not empty. Lazy:
 * changes load eagerly per agent at startup; tree/commits on first agent open.
 */
@Injectable({ providedIn: "root" })
export class AgentWorkStore {
  private bridge = inject(BRIDGE);

  private readonly changesMap = signal<Record<string, Loadable<AgentFile[]>>>({});
  private readonly commitsMap = signal<Record<string, CommitsEntry>>({});
  private readonly treesMap = signal<Record<string, Loadable<FileNode[]>>>({});

  // generation guards: a newer load supersedes an in-flight older one
  private changesGen: Record<string, number> = {};
  private commitsGen: Record<string, number> = {};
  private treesGen: Record<string, number> = {};

  changesFor(id: string): Loadable<AgentFile[]> {
    return this.changesMap()[id] ?? IDLE;
  }
  commitsFor(id: string): CommitsEntry {
    return this.commitsMap()[id] ?? IDLE_COMMITS;
  }
  treeFor(id: string): Loadable<FileNode[]> {
    return this.treesMap()[id] ?? IDLE;
  }
  /** Reactive per-id view (for component fields). */
  changesSig(id: string): Signal<Loadable<AgentFile[]>> {
    return computed(() => this.changesFor(id));
  }

  // ---- changes (eager per agent; reloaded on watcher events) ----
  loadChanges(id: string): void {
    const gen = (this.changesGen[id] ?? 0) + 1;
    this.changesGen[id] = gen;
    const prev = this.changesFor(id);
    this.patch(this.changesMap, id, { status: "loading", data: prev.data });
    void this.bridge
      .invoke<AgentFile[]>(Commands.AgentChanges, { id })
      .then((files) => {
        if (this.changesGen[id] !== gen) return;
        this.patch(this.changesMap, id, { status: "ready", data: files });
      })
      .catch(() => {
        if (this.changesGen[id] !== gen) return;
        this.patch(this.changesMap, id, { status: "error", data: prev.data });
      });
  }

  // ---- commits (lazy; paged; stale-while-revalidate) ----
  ensureCommits(id: string): void {
    if (this.commitsFor(id).status !== "idle") return;
    this.loadCommitsPage(id, COMMITS_PAGE, 0, []);
  }
  loadMoreCommits(id: string): void {
    const cur = this.commitsFor(id);
    if (cur.status === "loading" || !cur.hasMore) return;
    this.loadCommitsPage(id, COMMITS_PAGE, cur.data.length, cur.data);
  }
  /** Reload from the top (after a commit action) keeping current rows visible. */
  refreshCommits(id: string): void {
    const cur = this.commitsFor(id);
    if (cur.status === "idle") return; // never opened — stay lazy
    this.loadCommitsPage(id, Math.max(COMMITS_PAGE, cur.data.length), 0, cur.data);
  }
  private loadCommitsPage(id: string, limit: number, offset: number, keep: Commit[]): void {
    const gen = (this.commitsGen[id] ?? 0) + 1;
    this.commitsGen[id] = gen;
    this.patch(this.commitsMap, id, {
      status: "loading", data: keep, hasMore: this.commitsFor(id).hasMore,
    });
    void this.bridge
      .invoke<Commit[]>(Commands.AgentCommits, { id, limit, offset })
      .then((page) => {
        if (this.commitsGen[id] !== gen) return;
        const data = offset === 0 ? page : [...keep, ...page];
        this.patch(this.commitsMap, id, { status: "ready", data, hasMore: page.length === limit });
      })
      .catch(() => {
        if (this.commitsGen[id] !== gen) return;
        this.patch(this.commitsMap, id, { status: "error", data: keep, hasMore: false });
      });
  }

  // ---- file tree (lazy; reloaded on watcher events once loaded) ----
  ensureTree(id: string): void {
    if (this.treeFor(id).status !== "idle") return;
    this.loadTree(id);
  }
  private loadTree(id: string): void {
    const gen = (this.treesGen[id] ?? 0) + 1;
    this.treesGen[id] = gen;
    const prev = this.treeFor(id);
    this.patch(this.treesMap, id, { status: "loading", data: prev.data });
    void this.bridge
      .invoke<FileNode[]>(Commands.AgentTree, { id })
      .then((nodes) => {
        if (this.treesGen[id] !== gen) return;
        this.patch(this.treesMap, id, { status: "ready", data: nodes });
      })
      .catch(() => {
        if (this.treesGen[id] !== gen) return;
        this.patch(this.treesMap, id, { status: "error", data: prev.data });
      });
  }
  /** Lazily expand one unloaded dir: splice its children into the loaded tree. */
  expandDir(id: string, path: string): void {
    void this.bridge.invoke<FileNode[]>(Commands.AgentDir, { id, path }).then((kids) => {
      const cur = this.treeFor(id);
      if (cur.status === "idle") return;
      const patch = (list: FileNode[]): FileNode[] =>
        list.map((n) => {
          if (n.path === path) return { ...n, children: kids };
          if (n.children) return { ...n, children: patch(n.children) };
          return n;
        });
      this.patch(this.treesMap, id, { ...cur, data: patch(cur.data) });
    });
  }

  /** Watcher event: this agent's worktree changed. Changes always reload
   *  (eager data feeding the always-visible badges); tree only if loaded. */
  onWorktreeChanged(id: string): void {
    this.loadChanges(id);
    if (this.treeFor(id).status !== "idle") this.loadTree(id);
  }

  /** Drop all of an agent's entries (on removal). */
  dispose(id: string): void {
    for (const map of [this.changesMap, this.commitsMap, this.treesMap] as const) {
      map.update((m) => {
        if (!(id in m)) return m;
        const { [id]: _drop, ...rest } = m;
        return rest;
      });
    }
    delete this.changesGen[id];
    delete this.commitsGen[id];
    delete this.treesGen[id];
  }

  /** Single-key update — untouched ids keep their entry references. */
  private patch<T>(map: ReturnType<typeof signal<Record<string, T>>>, id: string, entry: T): void {
    map.update((m) => ({ ...m, [id]: entry }));
  }
}
```

- [ ] **Step 4.4: Export `Loadable` from `models.ts`** (above the `Agent` interface):

```ts
/** Async per-entity sub-resource: `idle` = never requested (unknown, NOT empty). */
export interface Loadable<T> {
  status: "idle" | "loading" | "ready" | "error";
  data: T;
}
```

- [ ] **Step 4.5: Run tests to verify pass:**

```bash
npx vitest run src/app/agents/agent-work.store.spec.ts
```
Expected: PASS (5 tests).

- [ ] **Step 4.6: Commit:**

```bash
git add src/app/agents/agent-work.store.ts src/app/agents/agent-work.store.spec.ts src/app/models.ts
git commit -m "feat(stores): AgentWorkStore — per-agent Loadable maps for changes/commits/tree"
```

---

### Task 5: Rewire `AgentRuntimeService` to the work store

**Files:**
- Modify: `src/app/agents/agent-runtime.service.ts`

- [ ] **Step 5.1: Replace the scan-everything startup effect** (lines 106–124) with eager-changes + watch + lazy-on-open:

```ts
    // Watch EVERY agent's worktree and eagerly scan only its CHANGES (cheap, and
    // the overview badges/kanban/sidebar need them without opening the agent).
    // Tree + commits stay lazy — ensured on first open below.
    effect(() => {
      for (const a of this.agentsStore.all()) {
        if (this.watched.has(a.id)) continue;
        this.watched.add(a.id);
        this.work.loadChanges(a.id);
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
      .onWorktreeChanged((id) => this.work.onWorktreeChanged(id))
      .catch(() => {});
```

with `private work = inject(AgentWorkStore);` added to the service's injects and the import added.

- [ ] **Step 5.2: Delete the moved machinery** — remove from `agent-runtime.service.ts`: `loadChanges`, `loadCommits`, `loadFiles`, `expandDir`, `changesGen`, `commitsGen`, `filesGen` (lines 234–299). In `dispose(id)` add `this.work.dispose(id);`.

- [ ] **Step 5.3: Fix remaining internal callers** — any `this.loadChanges/loadCommits/loadFiles` references inside the service are gone with the effect rewrite; search to confirm:

```bash
npx vitest run src/app/agents && grep -rn "loadFiles\|loadCommits\|loadChanges\|expandDir" src/app --include="*.ts"
```
Expected: hits only in `agent-work.store.ts` and consumers updated in Task 6 (e.g. `file-tree` calling `expandDir` — note them for Task 6).

- [ ] **Step 5.4: Commit:**

```bash
git add src/app/agents/agent-runtime.service.ts
git commit -m "refactor(runtime): startup scans → AgentWorkStore; lazy tree/commits on agent open"
```

---

### Task 6: Consumer migration off `Agent.git_changes/git_commits/files`

**Files:**
- Modify: `src/app/overview/agent-card.component.ts:72,195-196`
- Modify: `src/app/overview/kanban-view.component.ts:75`
- Modify: `src/app/sidebar/agent-row.component.ts:43,78-79`
- Modify: `src/app/agents/agent-actions.service.ts:249,271` (+ wherever it reloads after commit/discard)
- Modify: `src/app/right-panel/git-tab.component.ts:149-155` + template (Load more)
- Modify: `src/app/right-panel/file-tree.component.ts:76` (+ tree source, expandDir)
- Modify: `src/app/workspace/diff-view.component.ts:279`
- Modify: `src/app/models.ts:104-107` (drop the three fields)

The uniform pattern — inject the store, lookup by id:

```ts
private work = inject(AgentWorkStore);
readonly ch = computed(() => this.work.changesFor(this.agent().id));
// templates: ch().data instead of ag.git_changes?.files; ch().status === 'loading' for loaders
```

- [ ] **Step 6.1: agent-card** — replace the computeds and the template reads:

```ts
  private work = inject(AgentWorkStore);
  readonly ch = computed(() => this.work.changesFor(this.agent().id));
  readonly totAdd = computed(() => this.ch().data.reduce((s, f) => s + f.add, 0));
  readonly totDel = computed(() => this.ch().data.reduce((s, f) => s + f.del, 0));
```

template `:72`: `{{ (ag.git_changes?.files?.length ?? 0) }}` → `{{ ch().data.length }}`, and immediately after it add the small loader: `@if (ch().status === 'loading') { <span class="spin-dot" title="scanning…">·</span> }` (reuse the project's existing loading-dot styling if present; otherwise a plain `·` with `opacity:.5`).

- [ ] **Step 6.2: kanban-view** — `:75`: replace `(ag.git_changes?.files ?? []).reduce(...)` with `this.work.changesFor(ag.id).data.reduce((s, f) => s + f.add, 0)` (inject `work` as above).

- [ ] **Step 6.3: sidebar agent-row** — same two computeds as agent-card; template `:43` condition becomes `(work.changesFor(ag.id).data.length) > 0`.

- [ ] **Step 6.4: agent-actions.service** — `:249,271`: `disabled: !ag.git_changes?.files.length` → `disabled: !this.work.changesFor(ag.id).data.length`. Where the service handles a successful `commitAgent`/`discardAgent`, add `this.work.loadChanges(id); this.work.refreshCommits(id);` after the invoke resolves (find the exact methods — they call `agentsStore.commit/discard`).

- [ ] **Step 6.5: git-tab** — swap the three computeds and add Load more:

```ts
  private work = inject(AgentWorkStore);
  readonly changes = computed<AgentFile[]>(() => {
    const ag = this.agent();
    return ag ? this.work.changesFor(ag.id).data : [];
  });
  readonly changesLoading = computed(() => {
    const ag = this.agent();
    return ag ? this.work.changesFor(ag.id).status === "loading" : false;
  });
  readonly commitsEntry = computed(() => {
    const ag = this.agent();
    return ag ? this.work.commitsFor(ag.id) : null;
  });
  readonly agentCommits = computed<Commit[]>(() => this.commitsEntry()?.data ?? []);
```

template `:131-135` block becomes:

```html
        <div class="up" style="font-size:9px;color:var(--ink-3);padding:4px 14px">Commits on this branch</div>
        <app-commit-feed [commits]="agentCommits()" [compact]="true" />
        @if (commitsEntry()?.status === 'loading' && !agentCommits().length) {
          <div style="padding:2px 14px;font-size:10.5px;color:var(--ink-4)">loading commits…</div>
        } @else if (!agentCommits().length) {
          <div style="padding:2px 14px 14px;font-size:10.5px;color:var(--ink-4)">no commits yet</div>
        }
        @if (commitsEntry()?.hasMore) {
          <button class="btn ghost-hair" style="margin:4px 14px 14px" (click)="work.loadMoreCommits(ag.id)">
            Load more
          </button>
        }
```

(make `work` accessible from the template: `readonly work = inject(AgentWorkStore);`).

- [ ] **Step 6.6: file-tree + diff-view** — `file-tree.component.ts:76` iterates `this.agent().git_changes?.files` and the tree source reads `ag.files` — switch both to `work.changesFor(id).data` / `work.treeFor(id)` (`.data` for nodes, `.status === 'loading'` for its loading state) and route its lazy-expand through `work.expandDir(id, path)`. `diff-view.component.ts:279`: `this.agent().git_changes?.files ?? []` → `this.work.changesFor(this.agent().id).data`.

- [ ] **Step 6.7: Drop the model fields** — delete `models.ts:104-107` (`files`, `git_changes`, `git_commits` + the transients comment). Compile-sweep:

```bash
grep -rn "git_changes\|git_commits" src/app --include="*.ts"; npx ng build --configuration development 2>&1 | tail -20
```
Expected: grep → no hits outside specs; build → clean. Fix any straggler the compiler names (e.g. dev-panel, specs) with the same pattern.

- [ ] **Step 6.8: Run all frontend tests:**

```bash
npx vitest run
```
Expected: PASS — update any component/service specs that constructed agents with `git_changes` fixtures to provide store state instead (mock `AgentWorkStore` or seed via `loadChanges` with a fake bridge).

- [ ] **Step 6.9: Commit:**

```bash
git add src/app
git commit -m "refactor(ui): consumers read AgentWorkStore maps; Agent drops git transients; Load more commits"
```

---

### Task 7: Rust `perf` module + `perf://stats` push (B5)

**Files:**
- Create: `src-tauri/src/perf/mod.rs`
- Modify: `src-tauri/src/lib.rs` (mod decl + push loop)
- Modify: `src/app/data-source/bridge.ts` (`Events.PerfStats`)
- Modify: `src/app/data-source/instrumented-bridge.ts` (subscribe + setExec)
- Modify: command files (timed wrappers)

- [ ] **Step 7.1: Write the failing Rust tests** — bottom of new `perf/mod.rs` (write module skeleton + tests together, run, iterate):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timed_records_and_snapshot_aggregates() {
        reset();
        timed("t_cmd", || std::thread::sleep(Duration::from_millis(2)));
        timed("t_cmd", || std::thread::sleep(Duration::from_millis(4)));
        let snap = snapshot();
        let row = snap.iter().find(|r| r.cmd == "t_cmd").expect("t_cmd present");
        assert_eq!(row.calls10s, 2);
        assert!(row.avg_ms >= 2.0, "avg {} >= 2ms", row.avg_ms);
        assert!(row.max_ms >= row.avg_ms);
        assert!(row.p95_ms >= row.avg_ms);
    }

    #[test]
    fn ring_caps_and_concurrent_pushes_dont_panic() {
        reset();
        let handles: Vec<_> = (0..8)
            .map(|_| std::thread::spawn(|| {
                for _ in 0..100 { record("t_conc", Duration::from_micros(50)); }
            }))
            .collect();
        for h in handles { h.join().unwrap(); }
        let snap = snapshot();
        let row = snap.iter().find(|r| r.cmd == "t_conc").unwrap();
        assert!(row.calls10s as usize <= RING, "window count capped by ring");
    }

    #[test]
    fn empty_registry_snapshots_empty() {
        reset();
        assert!(snapshot().is_empty());
    }
}
```

- [ ] **Step 7.2: Implement the module:**

```rust
//! Per-command exec timing (the Rust half of the perf spec): a global registry
//! of recent (ts, duration) samples per command, pushed to the webview every 2s
//! on `perf://stats`. O(1) per call; in-memory only; never uploaded.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Per-command sample ring cap (mirrors the frontend PerfStore ring).
pub(crate) const RING: usize = 128;
/// Rolling rate window (mirrors PERF_WINDOW_MS).
const WINDOW_MS: u128 = 10_000;

#[derive(Default)]
struct CmdAgg {
    /// (epoch ms, micros) — newest last, capped at RING.
    samples: Vec<(u128, u32)>,
}

/// One command's exec aggregate, shaped for the frontend `ExecAgg`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdExec {
    pub cmd: String,
    pub calls10s: u32,
    pub avg_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

static STATS: OnceLock<Mutex<HashMap<&'static str, CmdAgg>>> = OnceLock::new();
fn stats() -> &'static Mutex<HashMap<&'static str, CmdAgg>> {
    STATS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

/// Record one exec sample. O(1).
pub fn record(cmd: &'static str, elapsed: Duration) {
    let mut m = stats().lock().unwrap();
    let agg = m.entry(cmd).or_default();
    agg.samples.push((now_ms(), elapsed.as_micros().min(u32::MAX as u128) as u32));
    if agg.samples.len() > RING {
        agg.samples.remove(0);
    }
}

/// Run `f`, recording its wall time under `cmd`. The command wrappers use this.
pub fn timed<T>(cmd: &'static str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    record(cmd, t.elapsed());
    out
}

/// Current per-command aggregates (whole ring for latency, 10s window for rate).
pub fn snapshot() -> Vec<CmdExec> {
    let m = stats().lock().unwrap();
    let now = now_ms();
    m.iter()
        .filter(|(_, agg)| !agg.samples.is_empty())
        .map(|(cmd, agg)| {
            let mut ms: Vec<f64> =
                agg.samples.iter().map(|(_, us)| *us as f64 / 1000.0).collect();
            ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let calls10s =
                agg.samples.iter().filter(|(ts, _)| now.saturating_sub(*ts) < WINDOW_MS).count();
            let p95_idx = ((ms.len() as f64 * 0.95).ceil() as usize).clamp(1, ms.len()) - 1;
            CmdExec {
                cmd: (*cmd).to_string(),
                calls10s: calls10s as u32,
                avg_ms: ms.iter().sum::<f64>() / ms.len() as f64,
                p95_ms: ms[p95_idx],
                max_ms: *ms.last().unwrap(),
            }
        })
        .collect()
}

/// Drop all samples (tests).
#[cfg(test)]
pub(crate) fn reset() {
    stats().lock().unwrap().clear();
}

/// Emit `perf://stats` every 2s while the app runs (skipped when empty).
pub fn spawn_push_loop<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        let snap = snapshot();
        if !snap.is_empty() {
            use tauri::Emitter;
            let _ = app.emit("perf://stats", &snap);
        }
    });
}
```

- [ ] **Step 7.3: Run the perf tests:**

```bash
cargo test --manifest-path src-tauri/Cargo.toml perf::
```
Expected: PASS (3 tests). Note: tests share the global registry — they use distinct cmd keys (`t_cmd`, `t_conc`) and `reset()`; if cargo runs them in parallel and they flake, serialize with `cargo test perf:: -- --test-threads=1` and note it in the test comment.

- [ ] **Step 7.4: Wire into `lib.rs`** — add `pub mod perf;` with the other mods, and in `setup` next to the metrics loop:

```rust
            // Perf exec-aggregate push loop (perf://stats every 2s; see src/perf).
            perf::spawn_push_loop(app.handle().clone());
```

- [ ] **Step 7.5: Wrap every command body in `perf::timed`.** Sync `(async)` commands — wrap the whole body:

```rust
#[tauri::command(async)]
pub fn agent_list(svc: State<'_, AgentService>) -> AppResult<Vec<Agent>> {
    crate::perf::timed("agent_list", || svc.list())
}
```

`spawn_blocking` commands — wrap INSIDE the closure (times the handler work, not the pool queue):

```rust
    tauri::async_runtime::spawn_blocking(move || crate::perf::timed("agent_changes", move || svc.changes(id)))
```

Apply to all commands in `agents/commands.rs`, `projects/commands.rs`, `cost/commands.rs`, `metrics/commands.rs`, `appicon.rs` with their exact command names as the literal. Skip `update.rs` (raw-invoke callers are the documented gap).

- [ ] **Step 7.6: Frontend wiring** — `bridge.ts` Events:

```ts
  /** Per-command backend exec aggregates (pushed every 2s; dev + prod). */
  PerfStats: 'perf://stats',
```

`instrumented-bridge.ts` — subscribe in the constructor (the store's `setExec` merge already exists):

```ts
import { Bridge, Events } from "./bridge";
import { ExecAgg, PerfStore } from "../perf/perf.store";
...
  constructor(
    private readonly inner: Bridge,
    private readonly perf: PerfStore,
  ) {
    void this.inner.on<ExecAgg[]>(Events.PerfStats, (aggs) => this.perf.setExec(aggs)).catch(() => {});
  }
```

- [ ] **Step 7.7: Full test pass both sides:**

```bash
cargo test --manifest-path src-tauri/Cargo.toml && npx vitest run
```
Expected: PASS. (`instrumented-bridge.spec.ts` may need its fake bridge to grow a no-op `on` — it already has one per the Bridge interface.)

- [ ] **Step 7.8: Commit:**

```bash
git add src-tauri/src src/app/data-source
git commit -m "feat(perf): Rust exec timing — perf::timed wrappers + perf://stats push into PerfStore"
```

---

### Task 8: E2E runtime verification + perf report "after" addendum

**Files:**
- Modify: `docs/perf/2026-06-09-perf-report.md` (append results)

- [ ] **Step 8.1: Launch instrumented dev app:**

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222'; npm run dev
```

(background; wait for `http://127.0.0.1:9222/json/list` to expose the page target; CDP driver script from the spike: `$env:TEMP\orrery-cdp.mjs`).

- [ ] **Step 8.2: Capture the startup perf table** via CDP (open `.dvc-fab`, scrape `.dvc-tbl`). Verify:
  - no row's avg RT near 5,000ms; `agent_commits` ≤ ~600ms (10 commits, delta-count only, debug build)
  - `AVG EXEC` column populated (numbers, not `—`); `OVERHEAD` ≈ single-digit ms
  - `agent_tree` ABSENT from the table at startup (lazy — nothing invoked it yet)
- [ ] **Step 8.3: Lazy + Load-more probe:** click an agent tab (CDP: click a sidebar/overview agent element), re-scrape: `agent_tree` + `agent_commits` rows appear only now; git tab shows 10 commits + "Load more"; click "Load more", commit rows grow, a second `agent_commits` sample registers.
- [ ] **Step 8.4: Concurrency re-probe** (same probe file as the spike): `agent_list` returns in <20ms while `system_metrics` is in flight.
- [ ] **Step 8.5: Append the "after" capture + verdicts to `docs/perf/2026-06-09-perf-report.md`** under a `## After — 2026-06-09 fixes (branch perf_enhancements)` heading: the scraped table, the lazy/Load-more observations, exec/overhead confirmation.
- [ ] **Step 8.6: Final commit:**

```bash
git add docs/perf/2026-06-09-perf-report.md
git commit -m "docs(perf): after-fix capture — startup collapse, lazy loads, exec/overhead columns live"
```

---

## Self-review (done at plan time)

- **Spec coverage:** D1→Task 0, D2→Task 3, D3→Task 1, D4→Tasks 4–6, D5/D6/D7→Tasks 4–5, D8→Task 7, D9→Task 3 (system primes in heavy set), D10 deferred items absent by design. E2E → Task 8.
- **Type consistency:** `Loadable` defined Task 4 / consumed Task 6; `commits(id, limit, offset)` store signature (Task 2) matches `AgentWorkStore` invokes (Task 4 uses raw bridge with the same payload); `CmdExec` serde camelCase matches frontend `ExecAgg` (`avgMs/p95Ms/maxMs`; extra `calls10s` ignored by the merge).
- **Placeholder scan:** none — every step carries code or an exact command + expected output.
