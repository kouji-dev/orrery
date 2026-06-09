# Global Performance Fix — Design

_2026-06-09 · branch `perf_enhancements` · companion to
[`docs/perf/2026-06-09-perf-report.md`](../../perf/2026-06-09-perf-report.md) (the
investigation that produced these decisions)._

## Problem

Startup blocked the UI for ~5.2s; perf-panel round-trips showed four agent commands pinned
at identical ~5,220ms. The investigation found five bottlenecks (B1–B5); an S1-a spike
(blanket `#[tauri::command(async)]`) **confirmed B1** — round-trips collapsed from a shared
5.2s queue to per-command real costs — and **revealed B2's true size**: `agent_commits`
is ~5.2s of real work (per-commit `.stats()` content diffs), not the ~900ms estimated.

## Decisions (as converged in discussion)

| # | Decision | Status |
|---|---|---|
| D1 | Blanket `#[tauri::command(async)]` on all sync commands | in tree, spike-verified |
| D2 | Heavy commands additionally move to the blocking pool (`spawn_blocking`) | this work |
| D3 | `log()` files-count from `deltas().len()` (keep the "N files" label, drop `.stats()`) | in tree |
| D4 | Frontend: git data separated from `Agent` into per-id `Loadable` hashMaps | this work |
| D5 | Eager at startup: `agent_list`, `project_list`, per-agent `agent_changes` (unitary), `agent_watch` | this work |
| D6 | Lazy on agent open: `agent_tree`, `agent_commits` (limit 10 + "Load more" via offset) | this work |
| D7 | Watcher reloads only a changed agent's **already-loaded** entries | this work |
| D8 | B5: implement the Rust perf half (`perf::timed` + `perf://stats` push) — frontend merge already exists | this work |
| D9 | B4: keep the `system_cost`/`system_metrics` primes (they cover the lost-first-emit gap; the cost thread emits before the webview subscribes), but on the blocking pool via D2 | this work |
| D10 | Deferred: B3 visibility-gating, liveness-tick hashMap migration, SHA files-count cache, dropping the files label | not now |

## Phase 1 — Backend threading completion (B1 = S1-c)

D1 is already applied (32 attribute swaps). This phase converts the **heavy set** to
`async fn` + `tauri::async_runtime::spawn_blocking`, so multi-second blocking work leaves
the tokio core workers:

- git2/fs: `agent_changes`, `agent_commits`, `agent_tree`, `agent_diff`, `agent_commit`,
  `agent_discard`, `agent_push`, `agent_spawn`, `agent_remove`, `project_commits`,
  `project_create`, `project_init_git`, `project_detect_git`
- process/sleep: `system_cost` (ccusage spawn, ~2s), `system_metrics` (sysinfo + 200ms
  sleep, ~2s cold), `detect_tools`

Mechanics: `AgentService`/`ProjectService` get `#[derive(Clone)]` (fields are `Arc`-backed
`DB` + unit-struct `GitService`); handlers clone the service into the closure, `.await` the
join handle, map `JoinError` to `AppError::Other`. Event emits (`emit_entity`) happen after
the await, on the async context. Commands keep their exact signatures and return types —
no frontend impact.

## Phase 2 — `agent_commits` pagination (backend)

`agent_commits(id, limit, offset)` — new optional `offset` (default 0), implemented as
revwalk `.skip(offset)`. `GitService::log(path, limit, offset)` gains the parameter; the
existing `take(limit)` follows the skip. `project_commits` keeps `offset=0` semantics
(callers unchanged).

D3 (already in tree) makes each walked commit cheap: `files` comes from `deltas().len()`
(tree-diff only — no blob loads, no xdiff). The "N files" label in the commit feed is
preserved with the same value (no rename detection in either path).

## Phase 3 — Frontend store separation (B2 core)

### New `AgentWorkStore` (`src/app/agents/agent-work.store.ts`)

```ts
type Loadable<T> = { status: 'idle' | 'loading' | 'ready' | 'error'; data: T };

changes: Signal<Record<string, Loadable<AgentFile[]>>>;
commits: Signal<Record<string, Loadable<Commit[]> & { hasMore: boolean }>>;
trees:   Signal<Record<string, Loadable<FileNode[]>>>;
// per-id reads: changesFor(id) / commitsFor(id) / treeFor(id) — computed lookups
```

Rules that make the map shape deliver its change-detection win:

- **Entry reference identity**: updating agent X's entry must not rebuild other entries —
  unchanged ids return identical references, so only X's consumers re-render. This is the
  "1000 terminals, one reload" requirement.
- **No coordination with the agent list**: the list (`AgentsStore.all`) drives card
  existence; map entries arriving before/after their agent's info is harmless (user's
  race-condition analysis).
- **`idle` ≠ empty**: lazy data not yet requested is *unknown*, not "no data". (The
  Commit/Discard menu enablement reads `changes`, which is eager — semantics preserved.)
- **Stale-while-revalidate** for commits: reloads keep previous rows visible
  (existing behavior, also the base for Load-more appends).
- Generation guards (current `changesGen` pattern) move along with the load methods.

### Loading triggers

- **Startup** (replaces the scan-everything effect in `agent-runtime.service.ts:109`):
  per-agent `loadChanges(id)` + `watch(id)` for every agent. No tree, no commits.
- **Agent open** (effect on `activeAgent()`): `ensureTree(id)` + `ensureCommits(id)` —
  no-ops unless `idle`.
- **Load more** (commit feed button): `loadMoreCommits(id)` appends the next page
  (`offset = data.length`, `limit 10`, `hasMore = page.length === limit`).
- **Watcher** (`agent://changed {id}`): `loadChanges(id)` always; `loadFiles(id)` only if
  tree not `idle`. Commits unaffected (reload stays tied to commit actions, as today).

### Consumer migration (Agent model loses `git_changes`/`git_commits`/`files`)

| Consumer | Change |
|---|---|
| `overview/agent-card.component` | badges + counts read `changesFor(id)`; small loader while `loading` |
| `overview/kanban-view.component` | totals read `changesFor(id)` |
| `sidebar/agent-row.component` | +add/−del badges read `changesFor(id)` |
| `agents/agent-actions.service` | Commit/Discard enablement reads `changesFor(id)` |
| `right-panel/git-tab.component` | changes list + agent commits read store; "Load more" button |
| `right-panel/file-tree.component` | tree reads `treeFor(id)` |
| `workspace/diff-view.component` | changes read `changesFor(id)` |
| `agents/agent-runtime.service` | load methods + overlay git fields move out to `AgentWorkStore` |
| `models.ts` | `Agent` drops the three transient fields |

`commit-feed.component` (project commits via `ProjectActionsService`) is untouched.

## Phase 4 — Backend exec metrics (B5, per the existing perf spec)

The frontend merge (`PerfStore.mergeExec`, avgExec/overhead columns) already exists and
waits for events. Implement the Rust half exactly as specced in
`2026-06-09-perf-tracing-design.md`:

- `perf` module: `CommandStats` registry (`Mutex<HashMap<&'static str, CmdAgg>>`, ring of
  recent `(ts, micros)`, O(1) push) + `perf::timed(cmd, f)` wrapper.
- Wrap each command body (mechanical, same files Phase 1 touches).
- Push thread (mirrors the metrics loop): every 2s emit `perf://stats`
  (`Vec<CmdExec> { cmd, calls10s, avgMs, p95Ms, maxMs }`), skip when empty.
- Frontend: add the `perf://stats` event constant + subscription wiring if absent.

This is also the verification instrument: post-fix, `overhead = RT − exec` should be ~1ms
across the board.

## Testing

- **Rust** (`cargo test`): `log()` files-count exactness (1-file and 2-file commits);
  `log()` offset pagination (skip/take windows, out-of-range → empty); `CommandStats`
  aggregation (window count, avg, p95, max, concurrent pushes).
- **Vitest**: `AgentWorkStore` — Loadable transitions, entry reference identity for
  untouched ids, Load-more append + `hasMore`, stale-while-revalidate, generation guards;
  existing `PerfStore`/component specs updated for the store split.
- **E2E (runtime)**: CDP harness against the dev app — startup perf-panel capture
  (no command above its real cost, no 5.2s rows), concurrency probe stays parallel,
  exec/overhead columns populated, agent open loads tree+commits lazily, Load more pages.
  Results appended to the perf report as the "after" record.

## Risks

- Consumer migration touches 8 files of template/computed plumbing — mitigated by vitest
  on the store + the E2E pass.
- `spawn_blocking` closures need owned data — `derive(Clone)` on Arc-backed services keeps
  this mechanical; no service internals change.
- Offset pagination can show one duplicate row if a commit lands between pages — accepted
  (v1); a `before_sha` cursor is the follow-up if it ever matters.
