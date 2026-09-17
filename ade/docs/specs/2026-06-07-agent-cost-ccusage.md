# Cost tracking: ccusage total in the status bar — design spec

Date: 2026-06-07
Status: **agreed, not yet implemented**

Show the user what their Claude usage is costing. v1 is a **single global "Total cost"**
number in the status bar, next to the existing CPU/RAM metrics. Computed by shelling out to
the **`ccusage`** npm tool, which already parses Claude Code's transcripts *and* prices them.

This is one of two specs from the 2026-06-07 brainstorm. The other is
`2026-06-07-agent-completion-remote-ops.md` (independent feature).

---

## Scope

**In:** one global all-time **Total cost** (USD), pushed to the status bar on an interval,
sourced from `ccusage`.

**Out (deferred):** per-agent cost, katrix-only scoping (filter to our session IDs),
time-windowed views (today/month), and cost for codex/cursor/gemini.

---

## Key decisions (from brainstorm)

- **Use `ccusage`, don't reinvent it.** ccusage parses `~/.claude/projects/**/*.jsonl` and
  applies up-to-date pricing (LiteLLM data). We skip building a transcript parser **and** a
  pricing table.
- **No katrix-side persistence needed.** ccusage reads the on-disk transcripts fresh each
  run; those JSONL files persist independently of katrix. So **restarting the app loses
  nothing**, and because Claude keeps the files even after we delete an agent's worktree, the
  total **survives agent deletion** too — for free. This directly answers the original "will
  I lose the data on restart?" question: no.
- **Global total (simplest).** The readout shows ccusage's whole-machine total — **all**
  Claude Code usage, katrix or not. No session-ID filtering in v1. (The number reflects total
  Claude spend, not just katrix agents — an accepted v1 simplification.)
- **Cumulative, never reset.** It's an all-time total by nature; stopping/resuming agents
  just keeps adding.
- **Pin ccusage as a dependency.** Added to `package.json` so there's no first-run `npx`
  download; run via local `node`. Fall back to `npx ccusage` if the local copy isn't
  resolvable.

---

## Backend (Rust)

New module `src-tauri/src/cost/` (mirrors the `metrics/` module's shape).

### Reading the total

- Invoke `ccusage daily --json` via `std::process::Command` and parse `.totals.totalCost`
  (confirmed field; `ccusage daily --json` → `{ daily: [...], totals: { …, totalCost } }`).
  - Default window is all-time → `totals.totalCost` is the global grand total.
- **Invocation resolution (first that works):**
  1. local pinned binary (e.g. `node node_modules/ccusage/dist/index.js daily --json`),
  2. `npx ccusage daily --json`.
- **Availability:** if no Node / ccusage can't run / JSON doesn't parse → treat as
  **unavailable** (don't error the app).

```
read_total() -> Option<f64>   // None when unavailable
```

### Pushing to the UI

- A lightweight ticker (own thread / async task) emits **every 60s** plus once at startup.
  Cost moves slowly, so 60s is deliberately lighter than the 3s `system://metrics` tick and
  keeps the cost of spawning Node negligible.
- Event **`system://cost`** with payload:

```
{ "totalCost": 12.34, "currency": "USD", "available": true }
// or { "available": false } when ccusage can't run
```

(Folding this into the existing `system://metrics` payload is an alternative, but a separate
event keeps the slow cost cadence independent of the fast metrics cadence.)

---

## Frontend (Angular)

- A small `CostStore` (signals, mirrors `MetricsStore`) subscribes to `system://cost`.
- The **status bar** renders `$X.XX` beside the CPU/RAM readouts when `available`, and
  **hides** the readout entirely when not.
- No interaction in v1 (no popover/breakdown) — a single number.

---

## Error handling

- ccusage missing, Node absent, non-zero exit, or unparseable JSON → `available: false` →
  the readout disappears. The feature **never blocks or crashes** anything.
- Spawn failures are logged at `debug`/`warn`, not surfaced as user notifications (it's
  ambient telemetry, not an action).

---

## Testing

- **Rust unit:** parse a captured `ccusage daily --json` sample → assert `totalCost`
  extracted; assert a malformed/empty blob yields `None` (unavailable).
- **Frontend:** `CostStore` spec — applies a `system://cost` event to the signal; status bar
  shows the formatted value when available and nothing when not.
- **E2E (per global rule):** with ccusage resolvable, assert a `$`-value appears in the
  status bar; with it forced unavailable, assert the readout is absent. (No assertion on the
  exact dollar amount — it depends on real machine usage.)

---

## Future (explicitly deferred, cheap to add later)

- **Per-agent cost** — `ccusage session --json` keyed by the `session_id` katrix already
  captures per agent; surface on the agent card.
- **katrix-only total** — filter sessions to our captured IDs.
- **Time windows** — today / this month via ccusage's `daily`/`monthly`.
- **Other tools** — codex/cursor/gemini once their usage formats are wired.
