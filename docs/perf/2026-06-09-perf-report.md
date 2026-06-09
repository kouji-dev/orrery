# Performance Bottleneck Report — Command Round-Trip Investigation

_2026-06-09 · investigation only, no implementation. Companion to
[`docs/superpowers/specs/2026-06-09-perf-tracing-design.md`](../superpowers/specs/2026-06-09-perf-tracing-design.md)
(the instrumentation that produced this data)._

## Context

The DevConsole perf panel (frontend round-trip instrumentation, commit `b9921b2`) captured
its first real startup profile. Symptom: **the UI blocks for ~5 seconds at startup**, with
several commands reporting ~5,200ms average round-trips.

## Captured data (dev tier, 2026-06-09T19:46Z)

```json
{
  "capturedAt": "2026-06-09T19:46:00.870Z",
  "tier": "dev",
  "windowMs": 10000,
  "rows": [
    { "cmd": "agent_watch",     "avgRt": 5224.6, "p95Rt": 5227,   "maxRt": 5227,   "errPct": 0, "avgExec": null },
    { "cmd": "agent_tree",      "avgRt": 5223.4, "p95Rt": 5224.6, "maxRt": 5224.6, "errPct": 0, "avgExec": null },
    { "cmd": "agent_commits",   "avgRt": 5223.0, "p95Rt": 5224.4, "maxRt": 5224.4, "errPct": 0, "avgExec": null },
    { "cmd": "agent_changes",   "avgRt": 5218.5, "p95Rt": 5224.1, "maxRt": 5224.1, "errPct": 0, "avgExec": null },
    { "cmd": "system_metrics",  "avgRt": 1796.2, "p95Rt": 1796.2, "maxRt": 1796.2, "errPct": 0, "avgExec": null },
    { "cmd": "system_cost",     "avgRt": 1796.1, "p95Rt": 1796.1, "maxRt": 1796.1, "errPct": 0, "avgExec": null },
    { "cmd": "detect_tools",    "avgRt": 69.3,   "p95Rt": 69.3,   "maxRt": 69.3,   "errPct": 0, "avgExec": null },
    { "cmd": "set_window_icon", "avgRt": 65.0,   "p95Rt": 65.0,   "maxRt": 65.0,   "errPct": 0, "avgExec": null },
    { "cmd": "project_list",    "avgRt": 35.0,   "p95Rt": 35.0,   "maxRt": 35.0,   "errPct": 0, "avgExec": null },
    { "cmd": "agent_list",      "avgRt": 13.7,   "p95Rt": 13.7,   "maxRt": 13.7,   "errPct": 0, "avgExec": null }
  ]
}
```

## How to read these numbers

There is **one queue** — the Tauri main thread — and every command waits in it.
Round-trip = _own work_ + _everything queued ahead_. Reconstructed startup timeline:

```
t=0ms      agent_list (14ms own work) — fast, first in line
t≈35ms     project_list, set_window_icon, detect_tools (~35–70ms each)
t≈1796ms   system_metrics + system_cost finish (ccusage scan ~1.7s)
t≈5220ms   agent_watch, agent_tree, agent_commits, agent_changes ALL finish
           (≈3.4s of real git/fs work split across the four, serialized)
```

`agent_watch` is actually a ~1ms operation (registers a file watcher); it reported
5,224ms because it was **last in line**. The entire 5.2s, the window event loop was
blocked → frozen UI.

---

## B1 — Every command serializes on the main thread 🔴 root cause

### What's happening

All ~30 Tauri commands are plain sync `fn`s. Tauri's rule: **synchronous commands execute
on the main thread; async commands execute on the tokio runtime.** The main thread also
pumps OS window events and dispatches IPC responses — one lane for everything.

### Source issue + evidence

The issue is **architectural, not any single slow function**: command work and the event
loop share one FIFO thread. Three independent pieces of evidence:

1. **The timing fingerprint.** `agent_changes` (5218.5) → `agent_commits` (5223.0) →
   `agent_tree` (5223.4) → `agent_watch` (5224.6): four _different_ operations finishing
   within 6ms of each other, in strict sequence. Independent slow ops don't do that; a
   draining FIFO queue does.
2. **`agent_watch` is provably cheap** (`agents/commands.rs:261-270` — one DB row lookup +
   watcher registration, microseconds). It reported 5,224ms. Only queue wait explains it.
3. **The freeze itself.** The WebView renders in its own process; only a blocked main
   thread explains all `invoke` replies stalling simultaneously plus native window stutter.

Nuance: B1 explains why _everything_ was slow at once, but **not** the ~3.4s of genuine
work between t≈1.8s and t≈5.2s — that's B2/B3 (real handler cost). Fixing B1 alone makes
the UI fluid; `agent_commits` would still take its real ~900ms.

### Strategies

#### S1-a — Blanket `#[tauri::command(async)]` (one token per command)

The attribute makes Tauri spawn the sync body on the tokio worker pool (~1 thread/core).

**Resolves:** the UI freeze entirely; cross-command queueing up to core count.
**Does not resolve:** each handler's own cost; bursts beyond ~#cores re-queue (invisibly).

| Pros | Cons |
|---|---|
| Smallest possible diff (~30 one-token edits) — lowest regression risk | Blocking git/IO sits on threads tokio intends for cheap async tasks ("wrong pool") |
| Trivially reviewable; behavior otherwise identical | Parallelism capped at core count; 16 git ops on 8 cores re-queues half |
| Reversible per-command | Heavy commands could starve future real async work (network, streams) |

#### S1-b — Full rewrite: `async fn` + `spawn_blocking` everywhere

Each handler becomes `async fn`, clones its service handle, ships the body to tokio's
**blocking pool** (grows to ~500 threads, built for blocking work).

**Resolves:** everything S1-a does, plus the parallelism cap and pool-misuse.

| Pros | Cons |
|---|---|
| Textbook-correct thread placement, future-proof | ~30 handlers restructured (move-closures, clone discipline, `JoinError` mapping) — biggest diff |
| Unbounded blocking parallelism | Most commands (`agent_input`, `agent_resize`, …) are microsecond-cheap — rewriting them buys nothing |
| Tokio workers always free | Biggest review surface = biggest chance of a new real bug |

#### S1-c — Hybrid: blanket attr + `spawn_blocking` for the heavy ~14 ⭐ recommended

S1-a everywhere; the genuinely blocking commands additionally get S1-b treatment:
git2 ones (`agent_changes/commits/tree/diff/commit/discard/push/spawn/remove`,
`project_commits/create/init_git/detect_git`) + process spawners (`system_cost`,
`detect_tools`).

**Resolves:** what S1-b resolves, at roughly half the churn.

| Pros | Cons |
|---|---|
| Right pool where it matters; one-liner for the rest | Two idioms coexist (mitigable with a ~5-line `blocking(move \|\| …)` helper) |
| Heavy git bursts can't cap out or starve anything | More work than S1-a; judgment call on the "heavy" set |
| Incremental: blanket lands first (instantly verifiable), heavies convert after | |

**Why S1-c:** S1-a is 95% of the user-visible win, but startup fires `4 × N agents` git
commands at once — exactly the burst shape that hits S1-a's core-count ceiling. ~14
mechanical conversions remove that ceiling; the two steps land as separate commits.

---

## B2 — `GitService::log()` pays a full tree-diff per commit 🔴 biggest single handler

### What's happening

`git/service.rs:88-99`: for each of up to **50 commits**, `log()` runs
`diff_tree_to_tree(parent, commit)` + `.stats()` — solely to display the changed-files
count badge in the commit feed. 50 complete libgit2 tree diffs per call,
O(commits × tree size). Called by `agent_commits` **and** `project_commits`, re-called on
watcher events.

### Source issue

**An O(n) decoration computed eagerly for a UI nicety.** The expensive field (`files`) is
the least important data in the row but dominates the call's cost. The ~500–900ms/call
estimate comes from the 3.4s of real work across the four startup commands; the exact
split is what B5's exec metrics would confirm.

### Strategies

#### S2-a — Cache `files` count by commit SHA ⭐ recommended

`Mutex<HashMap<Oid, usize>>` inside `GitService`. A commit's diff vs. its parent is
**immutable** — no invalidation, ever. First call fills; subsequent calls (watcher ticks,
tab switches, other agents on the same repo — one shared `GitService`) only diff new
commits.

**Resolves:** repeat-call cost — the steady-state burn, which is most calls.
**Does not resolve:** the very first call after launch.

| Pros | Cons |
|---|---|
| ~50× on every call after the first; zero behavior change | First call still pays full price (startup!) |
| No invalidation logic — key is cryptographically immutable | Unbounded growth (~50 bytes/commit — irrelevant) |
| ~20 lines, fully unit-testable | Adds an (uncontended) lock |

#### S2-b — Drop the eager `files` count (or fetch lazily)

**Resolves:** first-call cost too — `log()` becomes a pure revwalk, fast even cold.

| Pros | Cons |
|---|---|
| The only strategy fixing the _startup_ hit of this function | UI regression (badge gone) or two-phase load complexity (badge pop-in) |
| Simplest backend code afterwards | Product decision, not just engineering |

#### S2-c — Reduce `limit` 50 → 20

| Pros | Cons |
|---|---|
| One line, 2.5× cheaper | Still O(commits) tree-diffs; less history; problem returns as repos grow |

#### S2-d — Shell out to `git log --shortstat` once

| Pros | Cons |
|---|---|
| Native git diff is faster than libgit2's per-commit loop | Reintroduces process spawns (~100–300ms on Windows — what the codebase deliberately avoids) |
| | Output-parsing fragility; mixes two git backends |

**Why S2-a (optionally + S2-b later):** zero risk, kills the recurring cost. If cold-start
still bothers afterwards, S2-b is the follow-up — justified by B5 data.

---

## B3 — Full worktree re-scan every ~1s while agents write 🟡 steady-state jank

### What's happening

The watcher is well built (trailing-edge 200ms settle, 1s max-burst, `watch/mod.rs`). But
each `agent://changed` triggers **both** `loadFiles` (full tree walk, up to 10k nodes) and
`loadChanges` (full `status()` with line-level diffs incl. untracked content) —
`agent-runtime.service.ts:120-123`. A busy agent = both scans every second, per agent —
today on the main thread → stutter _while agents run_, distinct from the startup freeze.

### Source issue

**Recompute-the-world on every tick instead of incremental updates** — a reasonable v1
whose cost was invisible until measured. Ranked 🟡 because B1's fix demotes it from "UI
jank" to "background CPU".

### Strategies

#### S3-a — Nothing beyond B1 ⭐ recommended

Off the main thread, the 1s rescan can't touch the UI; generation counters (`changesGen`
etc.) already discard superseded results.

| Pros | Cons |
|---|---|
| Free | CPU still burns in the background; 5+ busy agents on big repos → fans spin |

#### S3-b — Visibility-gate the scans

Only rescan agents whose tree/diff panel is on screen; scan-on-reveal for the rest.

| Pros | Cons |
|---|---|
| Eliminates most background work at high agent counts | Stale flash on reveal; touches UI state logic; **speculative until measured** |

#### S3-c — Widen the debounce dials (settle 200→500ms, max-burst 1→3s)

| Pros | Cons |
|---|---|
| Two constants; 3× fewer scans during heavy writes | Live tree/diff feels less live; per-scan cost unchanged |

**Why S3-a:** YAGNI — B1 removes the symptom; whether the remaining CPU matters is an
empirical question the perf panel answers afterwards. Optimizing now is optimizing blind.

---

## B4 — `system_cost` runs ccusage inline at startup 🟡

### What's happening

`cost/commands.rs:5-8` spawns the ccusage scan synchronously as a "prime" so the status
bar paints early — comment estimates "a few hundred ms"; measured ~1.7s holding the lane
until t≈1.8s, ahead of the agent data the user actually waits for.

### Source issue

**A duplicate of existing infrastructure placed on the critical path.** A 60s push thread
already produces this exact snapshot; the inline command exists only to beat the first
push. The comment's estimate was ~5–10× optimistic on the slowest path possible (node
process spawn on Windows).

### Strategies

#### S4-a — Ride along with B1 (no extra work)

**Resolves:** its freeze/queue contribution. The spawn still happens, but parallel and
harmless.

#### S4-b — Delete the prime; push thread emits immediately at startup ⭐ recommended

**Verify first:** if the push thread sleeps 60s before its first push, deleting the prime
means a minute with no cost figure — the fix is an immediate first push at thread start,
which makes the command fully redundant.

| Pros | Cons |
|---|---|
| Removes a ~1.7s process spawn from startup entirely; deletes code | First cost paint waits for the thread's first emit (sub-second if it pushes at start) |
| One source of truth for cost data | Requires the loop-order check above |

---

## B5 — No backend exec timing (`avgExec: null` everywhere) 🟢 observability gap

### What's happening

The perf spec had two halves; only the frontend (round-trip) half was built. There is no
`perf` module in `src-tauri` — no `perf::timed`, no `perf://stats` push. The panel cannot
split _handler execution_ from _queue wait_ — exactly the axis this analysis runs along.

### Source issue

**A half-landed feature whose missing half is the verification tool for everything above.**
Every per-handler number here is inference from one timeline. With exec metrics:
`overhead = roundTrip − exec ≈ queue time`. After B1, overhead should collapse to ~1ms
across the board — a single capture proves the fix.

### Strategies

#### S5-a — Implement the Rust half (per the existing spec) ⭐ recommended

| Pros | Cons |
|---|---|
| Proves B1 worked (overhead → ~0) and quantifies B2 exactly | Touches all ~30 commands (mechanical 1-line wrapper) + one module + a 2s push thread |
| Permanent regression guard — a future sync command sticks out instantly | Real implementation work (though the spec is already approved) |
| **Synergy:** B1's fix already touches every command attribute — same files, same pass | |

#### S5-b — Skip; verify with round-trip numbers alone

| Pros | Cons |
|---|---|
| Less work now | RT alone can't distinguish "fixed" from "queueing less"; exec/overhead columns stay dead; blind next time |

---

## Composition & predicted after-picture

| | Resolves | Leaves behind |
|---|---|---|
| **B1** (S1-c) | UI freeze, phantom 5s round-trips, queue coupling | Real handler costs |
| **B2** (S2-a) | Dominant recurring handler cost (`log()`) | First-call cost (S2-b if it matters) |
| **B4** (S4-b) | 1.7s process spawn on the startup path | — |
| **B5** (S5-a) | Blindness; verifies all of the above | — |
| **B3** (S3-a) | — deferred, measured later with B5 data | Background CPU during heavy writes |

Predicted for the same capture after the fixes: `agent_watch` ~5,224ms → **~2ms**;
`agent_tree`/`agent_changes` → real cost (~100–400ms, visible in exec column);
`agent_commits` → real cost first call, **~10ms** after (cache); startup freeze **gone**;
every row showing `overhead ≈ 1ms` as proof.

## Open decisions

1. **B1 strategy:** S1-a / S1-b / S1-c (recommended: S1-c hybrid).
2. **Session scope:** B1 only · B1+B2 · B1+B2+B4+B5 (recommended: all four, B3 deferred).
