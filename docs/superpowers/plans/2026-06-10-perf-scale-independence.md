# Perf: scale independently of agent count

**Status: IMPLEMENTED 2026-06-10 (commits f2c1b7a..3422a06) — all of Part A + B1–B6
landed with two-stage subagent review per task + final integration review (READY).
Remaining: live smoke vs the acceptance budgets below via tools/perf-smoke.**

Symptoms (5 agents streaming, "hi" sent to one): typing in terminal very laggy;
every command's round-trip 100–640ms above backend exec; cost grows with agent
count. Goal: command latency and UI responsiveness O(1) in the number of
running agents.

## Verified root causes

### RC1 — Windows UI thread is a shared serial queue (CONFIRMED)

Every PTY batch becomes one `app.emit("agent://output", …)` from a per-agent
batcher thread (`src-tauri/src/runtime/mod.rs:157-177`, 8ms window / 16KB cap →
up to ~125 flushes/s **per agent**, more under size-cap pressure). Each emit is
marshaled to the Win32 main thread (`PostMessageW` via tao's `EventLoopProxy`)
and executed as `ICoreWebView2::ExecuteScript` — ≤16KB UTF-16 script conversion
per job, ~625 jobs/s at 5 agents.

Tauri v2 IPC on Windows shares that exact thread: the invoke request enters via
wry's `WebResourceRequested` COM callback **on the UI thread** (which also reads
the request body synchronously there), and because all our commands run on
tokio workers (`#[tauri::command(async)]`), the response completion is marshaled
back to the UI thread via `PostMessageW(EXEC_MSG_ID)`. **Every command crosses
the congested queue twice — request and response — while `perf::timed` measures
only the handler body.** That is the uniform 100–350ms "overhead" on commands
with 0.1ms exec (`agent_input` 118ms avg / 346ms max, `agent_resize` 161ms).

Typing lag is this, squared: each keystroke is one invoke (queue crossing ×2)
and its echo returns as a batched emit through the same queue, plus the 8ms
batch window. Secondary emitters pile onto the same queue: `perf://stats` every
2s, `system://metrics` every 3s while agents run, watcher `agent://changed`.

### RC2 — Webview JS thread per-chunk work, much of it dead (CONFIRMED core)

- `appendPtyTail` (`src/app/utils.ts:72-145`): `TOKEN_RE` ends in `|[\s\S]` →
  one token **per plain character**, and `writeAt` rebuilds the whole current
  line per character → O(chunk_chars × line_length) allocations per 80ms flush,
  for **all** agents, visible or hidden (`agent-runtime.service.ts:177-185`).
  The result feeds `liveLogs`, which has **zero template consumers**; hook-driven
  tools (claude/codex/cursor) skip the promptTail heuristics that read it.
  This is the single biggest pure-waste CPU item on the JS thread.
- Tiled-visible terminals bypass the new output scheduler entirely
  (`terminal.service.ts:172-179`: `element.isConnected` → direct `term.write`),
  so N visible panes = N unpaced xterm parse + WebGL pipelines.
- `bumpStats` allocates a new stats object per chunk on both scheduler paths
  (`terminal-output-scheduler.ts:143,173`).
- The 800ms elapsed tick patches runtime state and rebuilds the `agents`
  computed array (identity churn, `agent-runtime.service.ts:42-48,191`).
- Invoke resolutions are eval'd callbacks on this same thread — `avgRt` stops
  only when the thread frees up (`instrumented-bridge.ts:22-29`), which is the
  frontend half of the universal overhead.

Refuted along the way (do NOT act on these): xterm listeners are registered
outside the Angular zone (attach runs in `afterRenderEffect`, which Angular runs
via `runOutsideAngular`) — keystrokes do **not** trigger zone CD ticks;
`eventCoalescing: true` already coalesces event-driven CD per frame. A zoneless
migration is NOT part of this plan.

### RC3 — Background load scaling with agents (CONFIRMED with caveats)

- Per-agent git2 status scans (with line counts + untracked content) are
  serialized through a global `scan_lock` (`watch/mod.rs:47-52`); fingerprinting
  suppresses the emit, not the scan CPU. N busy agents keep the chain
  near-continuous (~129ms per scan).
- Metrics sweep refreshes **all machine processes** every 3s while any agent
  runs (~623ms measured); the one-shot `system_metrics` does a cold init + two
  full sweeps + 200ms sleep (~1s).
- `system_cost` shells out `cmd → npx → node` (ccusage, 5.4–5.8s measured); the
  push loop's immediate first run can overlap the frontend's startup one-shot →
  two concurrent node transcript scans.
- Latent hazard (not the cause of current numbers, fix anyway): the procs-map
  mutex is held across blocking ConPTY `write_all`/`flush`/`resize`
  (`runtime/mod.rs:242-264`) — one stuck agent pipe would block
  `agent_input`/`agent_resize` for **all** agents.

### Measurement caveat (affects how the dev-panel table is read)

`avgRt/p95/max` average the **whole 128-sample lifetime ring** with no time
cutoff (`perf.store.ts:107-126`, `perf/mod.rs:101-130`); only `calls10s` is
windowed. Every `calls10s=0` row (system_cost 5421ms, agent_commits 791ms,
agent_start 616ms…) is a frozen average from the start-burst, **not** current
steady-state latency. Also: for `agent_tree`/`agent_commits` the exec clock
starts inside `spawn_blocking`, so blocking-pool queue wait + serde of large
payloads (10k FileNode, 50 CommitView) land in "overhead" by construction.

## Plan

### Part A — Perf logic that does not impact app code

The existing pattern is already non-invasive (InstrumentedBridge wraps the
BRIDGE token; `perf::timed` wraps handler bodies). Extend it, don't spread it:

- **A1. Honest table.** Window the latency columns to the same 10s as
  `calls10s` (or render lifetime rows dimmed/"stale"). Merge the two
  STATS-mutex acquisitions per PTY flush (`record` + `record_volume`,
  `runtime/mod.rs:168-169`) into one.
- **A2. Queue-depth gauges (the missing signal).** Two cheap counters, both
  outside app logic: (1) UI-thread saturation probe — a 1s heartbeat timer on
  the Rust main thread whose observed-vs-expected drift = queue delay; (2) JS
  long-task observer (`PerformanceObserver('longtask')`) feeding PerfStore.
  These directly measure both congested threads instead of inferring from Rt.
- **A3. Repeatable load harness.** `tools/perf-smoke/`: spawns N synthetic
  agents running a noise generator (configurable bytes/s, TUI-style redraws),
  sends scripted keystrokes, exports the dev-panel capture JSON, asserts
  budgets. Run manually or in CI. Budgets (5 streaming agents): `agent_input`
  p95 ≤ 25ms; PTY-origin UI-thread jobs ≤ 75/s total; no JS long task > 100ms.

### Part B — Fixes (priority order, each lands with its E2E)

- **B1. One emit per frame, not per agent-flush (kills RC1's O(N)).** Replace
  per-agent batcher emits with a global multiplexer: per-agent coalescing
  buffers drain into a single `agent://output` emit every ~16ms carrying
  `[{id, chunk, seq}, …]`. UI-thread jobs become ≤ ~60/s **regardless of agent
  count**. Keep the 16KB per-agent cap; keep seq semantics for snapshot
  recovery. (Spike option, only if B1 proves insufficient: `tauri::ipc::Channel`
  / pull-based transport to leave the eval path entirely.)
- **B2. Focus-aware cadence.** Frontend tells backend which agent is focused
  (extend `agent_watch` or a new `agent_focus`); focused drains at 16ms, hidden
  at 100–250ms. Cuts both event volume and JS work; echo latency for the
  terminal you're typing in stays on the fast path.
- **B3. Kill the dead per-char folding (biggest JS win).** Store raw tail
  chunks in a bounded ring; run `appendPtyTail` lazily only when promptTail is
  actually read (gemini-only heuristics, exit handling). No behavior change for
  hook-driven tools. Rewrite `writeAt` to batch plain-text runs (no per-char
  line rebuild) for the remaining lazy path.
- **B4. Frontend micro-churn.** Gate `bumpStats` behind dev-panel-open signal;
  stop patching the agents array from the 800ms elapsed tick (derive elapsed in
  the component from a shared `now` signal).
- **B5. Background load.** Reuse the long-lived sysinfo sampler for the
  one-shot command (no cold double sweep); scope process refresh to known PIDs
  (agents + self) instead of `ProcessesToUpdate::All`; dedupe ccusage startup
  one-shot vs push-loop first run (share one snapshot); skip line-count diffing
  in watcher scans for non-focused agents (status-only scan).
- **B6. Procs-lock hazard.** Move ConPTY write/resize out from under the
  procs-map mutex (per-agent writer handle behind its own lock).

### Sequencing

A1+A2 first (one small PR — they make every later fix measurable), then B1
(re-measure with A3), then B3, then B2/B4/B5/B6 as measured priorities dictate.
Phase-2 backlog items (ACK backpressure, snapshot recovery, WebGL, sleep) stay
queued behind these; B1 is a prerequisite for a sane ACK design anyway.

### Acceptance (gated by the A3 harness)

With 5 agents streaming + typing into the focused terminal:
- `agent_input` p95 ≤ 25ms round-trip; felt echo ≤ 50ms.
- PTY-origin UI-thread jobs ≤ 75/s total, flat from 1 → 10 agents.
- No JS long task > 100ms attributable to PTY handling.
- Dev-panel table shows windowed (current) latencies.
