# Performance Tracing & Command Observability — Design

_2026-06-09 · branch `worktree-perf-tracing`_

## Problem

We have no visibility into command performance. When the app feels slow we can't tell
which Tauri command is to blame, whether the cost is in the Rust handler or in IPC, or
how often a command runs. We want to **locate what is slow** — in local dev (rich) and on
a shipped install (lightweight, local-only, no paid cloud/telemetry).

## Goals

- Per-command **rolling aggregates**: calls/10s, avg round-trip, avg exec, p95, max, error %.
- **Color-coded** surface that makes slow commands obvious (green / amber / red), sortable
  slowest-first.
- Split **round-trip (frontend-observed)** from **exec (Rust handler)** so IPC overhead is
  visible (`overhead = round-trip − exec`).
- **Two tiers**: dev = rich (recent-call feed, debug perf logs); prod = aggregates only,
  in-memory, **no upload** — the in-app panel is the surface.
- Logging that **never blocks a command thread** (async file writes on a dedicated thread).
- Negligible overhead in production (O(1) per call).

## Non-goals (this pass)

- Nested sub-operation spans (git/db breakdown) — that's the deferred "Approach C". The
  `timed` wrapper is designed so spans can be added later without rework.
- Cross-restart persistence / diagnostics export of prod metrics — aggregates are live
  in-memory only for now. (Revisit if we later want users to send us numbers.)
- Auto-upload to any service. Everything stays on the machine.

## Chosen approach — "B"

Round-trip timing at the single frontend chokepoint **plus** per-command exec timing in
Rust, merged by command name in a DevTools panel. (A = round-trip only; C = full spans.
B is the sweet spot: pinpoints where time goes without instrumenting every internal op.)

## Architecture

```
 Angular                                   Rust
 ┌─────────────────────────┐               ┌──────────────────────────┐
 │ InstrumentedBridge      │  invoke       │ #[command] fns           │
 │  wraps TauriBridge      │ ────────────▶ │  wrapped by perf::timed  │
 │  times round-trip       │ ◀──────────── │   records exec → registry│
 │  → PerfStore.record()   │  result       │                          │
 │                         │               │ CommandStats registry    │
 │ PerfStore (signals)     │ ◀─ perf://stats (every 2s) ── pushes exec │
 │  merge RT + exec        │   event        │  aggregates              │
 │                         │               └──────────────────────────┘
 │ DevPanel ▸ Performance  │
 │  color-coded table      │               Async log writer:
 └─────────────────────────┘               log record → bounded channel → writer thread → file
```

### Components

#### 1. Frontend round-trip — `InstrumentedBridge` (decorator)

- New `InstrumentedBridge implements Bridge`, constructed around the real `TauriBridge`.
  Provided for the `BRIDGE` token in place of `TauriBridge`. Keeps `TauriBridge` pure.
- Around each `invoke(cmd, payload)`: capture `start`, await, capture `end`; call
  `perf.record(cmd, end - start, ok)` in both success and error paths (error still records,
  flagged `ok:false`). Re-throws unchanged.
- `on` / `pickDirectory` pass through untouched.
- Raw-`invoke` callers (updater `update_check`/`update_perform`) stay uninstrumented this
  pass — documented gap.

#### 2. Frontend aggregation — `PerfStore` (signal store)

- `record(cmd, roundTripMs, ok)` feeds a per-command ring buffer of recent samples
  `{ts, ms, ok}` (capped, e.g. last 128) — enough for p95/max and a 10s-window count.
- Derived per command: `calls10s` (samples with `ts > now-10_000`), `avgRt`, `p95Rt`,
  `maxRt`, `errPct`.
- Merges backend **exec** aggregates received on `perf://stats` (keyed by command) → exposes
  `avgExec`, and `overhead = avgRt − avgExec`.
- Exposes a `rows()` computed signal: one row per command seen, for the panel.
- A `clear()` for the panel's reset button.

#### 3. Backend exec timing — `perf` module

- `perf::timed(cmd: &'static str, f)` runs `f`, records `elapsed` into a global
  `CommandStats` registry, returns `f`'s output. Sync + async variants (`timed` / `timed_async`).
- Each `#[tauri::command]` handler wraps its body in `perf::timed("<cmd>", || { ... })`.
  Mechanical, ~1 line/command. **Default: instrument all ~30 commands** (confirm at review;
  alternative is hot commands only — agent_spawn/diff, project_commits, git).
- `CommandStats`: `Mutex<HashMap<&'static str, CmdAgg>>` where `CmdAgg` keeps a small ring of
  recent `(ts, micros)` → computes count/window, avg, p95, max. O(1) push.
- A push loop (mirrors the existing `system://metrics` thread in `lib.rs`) emits
  `perf://stats` every 2s with the per-command exec aggregates. Skipped/empty when no calls.

#### 4. Async logging — dedicated writer thread (Phase 2)

- Replace the synchronous `LogDir` target with a non-blocking file sink: log records →
  bounded `std::sync::mpsc` channel → a dedicated writer thread that appends to the logfile.
  On overflow, **drop + increment a dropped-count** rather than block the producer.
- `Stdout` + `Webview` targets keep current behavior (frontend still sees logs).
- Implementation: a custom `log::Log` fan-out replacing `tauri_plugin_log`'s file target,
  preserving the `log::info!`/`warn!`/`debug!` call sites unchanged.
- This is the highest-risk piece (touches the logging stack) and is isolated as **Phase 2**
  so Phase 1 (perf tracing) can land first.

#### 5. DevTools surface — `DevPanel ▸ Performance`

- New "Performance" section in `dev-panel.component.ts` (or a sibling component it imports):
  a table — `cmd · calls/10s · avg RT · avg exec · overhead · p95 · max · err%`.
- **Color coding** by latency threshold (configurable consts):
  green `≤ 16ms` · amber `16–100ms` · red `> 100ms`; `err% > 0` rendered red.
- Sorted **slowest-first** by avg RT (header click to re-sort). A "reset" clears `PerfStore`.
- **Dev tier extra**: a live "recent calls" feed (last N `{ts, cmd, ms, ok}`) under the table,
  shown only when `isDevMode()`.

## Tiers / overhead

| | Prod | Dev |
|---|---|---|
| Round-trip aggregates | yes | yes |
| Exec aggregates (`perf://stats`) | yes | yes |
| Recent-calls feed | no | yes |
| Perf debug logs | no | yes (`debug`) |
| Storage | in-memory only | in-memory only |
| Upload | never | never |

Per-call cost is one timestamp + a ring-buffer push on each side — negligible. The panel and
recent-feed are dev-gated by `isDevMode()`; the aggregate counters run in both.

## Data shapes

```ts
// frontend
interface PerfSample { ts: number; ms: number; ok: boolean; }
interface PerfRow {
  cmd: string;
  calls10s: number; avgRt: number; p95Rt: number; maxRt: number; errPct: number;
  avgExec?: number; overhead?: number; // from perf://stats
}
```

```rust
// perf://stats payload: Vec<CmdExec>
struct CmdExec { cmd: String, calls10s: u32, avg_ms: f64, p95_ms: f64, max_ms: f64 }
```

## Thresholds & constants (defaults — confirm at review)

- Latency colors: `16ms` (green/amber), `100ms` (amber/red).
- Rolling window: `10s`.
- Sample ring per command: `128`.
- `perf://stats` push interval: `2s`.

## Testing

- `PerfStore`: aggregation correctness — avg, p95, 10s-window count, err%, RT/exec merge,
  `clear()`. (vitest)
- `InstrumentedBridge`: records on success and on thrown error (ok=false), re-throws,
  passes through `on`/`pickDirectory`. (vitest)
- `CommandStats` (Rust): push/aggregate — count window, avg, p95, max; concurrent pushes
  don't panic. (`cargo test`)
- Phase 2 async log writer: drains in order, drops+counts on overflow, flushes on shutdown.

## Phasing

- **Phase 1** — perf tracing end to end: `InstrumentedBridge`, `PerfStore`, `perf` module +
  `CommandStats` + `perf://stats` push, command wrappers, DevPanel Performance section. Ships
  the whole "locate what's slow, colored red" feature.
- **Phase 2** — async logging (dedicated writer thread for the file sink).

## Open decisions for review

1. Instrument **all ~30 commands** (default) vs **hot commands only** to start.
2. Thresholds `16/100ms` and window `10s` — good, or different?
3. Include **Phase 2 async logging** now, or land Phase 1 first and do logging after?

## Risks

- Replacing `tauri_plugin_log`'s file target could regress webview/stdout logging — mitigated
  by keeping those targets and isolating the change to Phase 2.
- Wrapping every command is mechanical but broad; a macro could reduce churn if it grows.
- `perf://stats` adds a 2s timer thread — same proven pattern as `system://metrics`.
