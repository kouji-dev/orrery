# Cost Tracking (ccusage) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a single global "Total cost" (USD) in the status bar, sourced from the `ccusage` CLI, refreshed on an interval.

**Architecture:** A backend `cost` module shells out to `ccusage daily --json` and parses `.totals.totalCost`. A push-loop in `lib.rs` setup emits `system://cost` every 60s (mirrors the existing 3s `system://metrics` loop). A frontend `CostStore` mirrors the latest payload into a signal the status bar reads; the readout hides when ccusage is unavailable.

**Tech Stack:** Rust (`std::process::Command`, `serde_json`), Tauri events, Angular 20 signals, `ccusage` (npm).

**Spec:** `docs/specs/2026-06-07-agent-cost-ccusage.md`

---

## File Structure

- Create: `src-tauri/src/cost/mod.rs` — `parse_total` (pure, tested) + `read_total` (runs ccusage).
- Create: `src-tauri/src/cost/commands.rs` — `system_cost` one-shot command (for initial paint).
- Modify: `src-tauri/src/lib.rs` — declare `mod cost`, register `system_cost`, add the 60s push-loop.
- Modify: `src/app/orchestra/data-source/bridge.ts` — `SystemCost` command + `SystemCost` event.
- Modify: `src/app/orchestra/models.ts` — `CostSnapshot` interface.
- Create: `src/app/orchestra/metrics/cost.store.ts` — `CostStore` (mirrors `MetricsStore`).
- Create: `src/app/orchestra/metrics/cost.store.spec.ts` — store unit test.
- Modify: `src/app/orchestra/status-bar/status-bar.component.ts` — render `$X.XX` from `CostStore`.

**Independence note:** This plan shares two files with the git-ops plan (`lib.rs`, `bridge.ts`). When run as parallel sub-agents, isolate in separate worktrees and merge — the edits are in different regions (cost adds a `mod cost`/`system_cost`/push-loop; git-ops touches the agent command list).

---

## Task 1: Pin ccusage as a dependency

**Files:**
- Modify: `package.json` (devDependencies)

- [ ] **Step 1: Add ccusage**

Run: `pnpm add -D ccusage`
Expected: `ccusage` appears under `devDependencies` in `package.json`; `pnpm-lock.yaml` updated.

- [ ] **Step 2: Confirm it runs**

Run: `pnpm exec ccusage daily --json`
Expected: JSON to stdout containing a top-level `"totals"` object with a `"totalCost"` number. (If the machine has no Claude usage, `totalCost` may be `0` — still valid JSON.)

- [ ] **Step 3: Commit**

```bash
git add package.json pnpm-lock.yaml
git commit -m "build: pin ccusage for cost tracking"
```

---

## Task 2: Backend cost parser (pure, TDD)

**Files:**
- Create: `src-tauri/src/cost/mod.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod cost;` — see Task 4)

- [ ] **Step 1: Create the module with a failing test**

Create `src-tauri/src/cost/mod.rs`:

```rust
//! Total Claude usage cost, read from the `ccusage` CLI. We don't persist
//! anything — ccusage reads the on-disk transcripts fresh each run, so the total
//! survives app restarts and agent deletion. Global all-time total (all Claude
//! Code usage on the machine, not just katrix agents).

use serde::Serialize;

pub mod commands;

/// A cost snapshot pushed to the UI. `available` is false when ccusage could not
/// run (no Node / not installed / bad output) — the UI then hides the readout.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CostSnapshot {
    pub total_cost: f64,
    pub currency: &'static str,
    pub available: bool,
}

impl CostSnapshot {
    pub fn unavailable() -> Self {
        Self { total_cost: 0.0, currency: "USD", available: false }
    }
    pub fn usd(total: f64) -> Self {
        Self { total_cost: total, currency: "USD", available: true }
    }
}

/// Pull `.totals.totalCost` out of a `ccusage daily --json` blob. `None` when the
/// JSON is malformed or the field is missing/non-numeric.
pub fn parse_total(json: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("totals")?.get("totalCost")?.as_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_total_reads_totals_total_cost() {
        let blob = r#"{"daily":[{"date":"2026-06-07","totalCost":1.5}],"totals":{"inputTokens":10,"outputTokens":20,"totalCost":12.34}}"#;
        assert_eq!(parse_total(blob), Some(12.34));
    }

    #[test]
    fn parse_total_none_on_garbage_or_missing() {
        assert_eq!(parse_total("not json"), None);
        assert_eq!(parse_total(r#"{"daily":[]}"#), None); // no totals
        assert_eq!(parse_total(r#"{"totals":{}}"#), None); // no totalCost
    }
}
```

- [ ] **Step 2: Wire `mod cost;` so the test compiles**

In `src-tauri/src/lib.rs`, add `mod cost;` to the module list (alphabetical, after `mod core;`):

```rust
mod core;
mod cost;
mod fs;
```

(Also create `src-tauri/src/cost/commands.rs` as an empty-ish file now so `pub mod commands;` resolves — Task 3 fills it. For this step put `// filled in Task 3` in it.)

- [ ] **Step 3: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib cost::tests`
Expected: `parse_total_reads_totals_total_cost` and `parse_total_none_on_garbage_or_missing` PASS.

- [ ] **Step 4: Add `read_total` (runs ccusage; not unit-tested — external process)**

Append to `src-tauri/src/cost/mod.rs` (above the `#[cfg(test)]`):

```rust
/// Run `ccusage daily --json` and return the global total cost, or `None` if it
/// can't run / parse. Tries the local pinned binary via `npx` first; on Windows
/// `npx` is a `.cmd`, so it's invoked through `cmd /C`.
pub fn read_total() -> Option<f64> {
    let output = run_ccusage()?;
    if !output.status.success() {
        log::warn!("ccusage exited non-zero");
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_total(&stdout)
}

#[cfg(windows)]
fn run_ccusage() -> Option<std::process::Output> {
    std::process::Command::new("cmd")
        .args(["/C", "npx", "ccusage", "daily", "--json"])
        .output()
        .ok()
}

#[cfg(not(windows))]
fn run_ccusage() -> Option<std::process::Output> {
    std::process::Command::new("npx")
        .args(["ccusage", "daily", "--json"])
        .output()
        .ok()
}

/// One snapshot: the global total, or `unavailable()` when ccusage can't run.
pub fn snapshot() -> CostSnapshot {
    match read_total() {
        Some(t) => CostSnapshot::usd(t),
        None => CostSnapshot::unavailable(),
    }
}
```

- [ ] **Step 5: Build to confirm it compiles**

Run: `cd src-tauri && cargo build`
Expected: builds clean (warnings about unused `snapshot`/`commands` are fine until Task 4).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/cost/mod.rs src-tauri/src/cost/commands.rs src-tauri/src/lib.rs
git commit -m "feat(cost): ccusage total parser + reader"
```

> **Production caveat (note in PR, not a blocker):** `npx ccusage` resolves the dev project's local install. A packaged build won't have `node_modules/ccusage` beside it — cost then reports `available:false` (readout hidden) unless ccusage is global or bundled. Acceptable for v1; revisit when packaging.

---

## Task 3: One-shot `system_cost` command (initial paint)

**Files:**
- Modify: `src-tauri/src/cost/commands.rs`

- [ ] **Step 1: Implement the command**

Replace the placeholder contents of `src-tauri/src/cost/commands.rs`:

```rust
use super::{snapshot, CostSnapshot};

/// Optional synchronous initial value so the status bar can paint a cost before
/// the first 60s push. Runs ccusage inline (a few hundred ms) — fine for a prime.
#[tauri::command]
pub fn system_cost() -> CostSnapshot {
    snapshot()
}
```

- [ ] **Step 2: Build**

Run: `cd src-tauri && cargo build`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/cost/commands.rs
git commit -m "feat(cost): system_cost one-shot command"
```

---

## Task 4: Register the command + 60s push-loop in lib.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Register the command**

In the `tauri::generate_handler![ ... ]` list (after `metrics::commands::system_metrics,`), add:

```rust
            metrics::commands::system_metrics,
            cost::commands::system_cost,
```

- [ ] **Step 2: Add the push-loop**

In the `.setup(|app| { ... })` closure, immediately AFTER the existing metrics `std::thread::spawn(...)` block (the one ending `let _ = metrics_app.emit("system://metrics", m);`), add:

```rust
            // Cost push loop: every 60s shell out to `ccusage` and emit the global
            // total on `system://cost`. Slow-moving, so 60s (vs metrics' 3s) keeps
            // node-spawn cost negligible. `available:false` is emitted when ccusage
            // can't run — the UI hides the readout.
            let cost_app = app.handle().clone();
            std::thread::spawn(move || {
                use tauri::Emitter;
                loop {
                    let snap = cost::snapshot();
                    let _ = cost_app.emit("system://cost", snap);
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            });
```

- [ ] **Step 3: Build + run the full backend suite**

Run: `cd src-tauri && cargo build && cargo test --lib`
Expected: builds; all tests pass (cost parser tests included).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(cost): register system_cost + 60s system://cost push loop"
```

---

## Task 5: Frontend bridge + model

**Files:**
- Modify: `src/app/orchestra/data-source/bridge.ts`
- Modify: `src/app/orchestra/models.ts`

- [ ] **Step 1: Add the command + event**

In `bridge.ts`, add to `Commands` (after `SystemMetrics: 'system_metrics',`):

```ts
  SystemCost: 'system_cost',
```

Add to `Events` (after `SystemMetrics: 'system://metrics',`):

```ts
  /** Global Claude cost total from ccusage (pushed every 60s). */
  SystemCost: 'system://cost',
```

- [ ] **Step 2: Add the model**

In `models.ts`, after the `SystemMetrics` interface, add:

```ts
// A cost snapshot pushed on `system://cost` (~every 60s). `available` is false
// when ccusage could not run — the status bar then hides the readout.
export interface CostSnapshot {
  totalCost: number;
  currency: string;
  available: boolean;
}
```

- [ ] **Step 3: Typecheck**

Run: `pnpm exec tsc --noEmit -p tsconfig.app.json` (or `pnpm build`)
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/app/orchestra/data-source/bridge.ts src/app/orchestra/models.ts
git commit -m "feat(cost): bridge command/event + CostSnapshot model"
```

---

## Task 6: CostStore (TDD)

**Files:**
- Create: `src/app/orchestra/metrics/cost.store.ts`
- Create: `src/app/orchestra/metrics/cost.store.spec.ts`

- [ ] **Step 1: Write the failing test**

Create `src/app/orchestra/metrics/cost.store.spec.ts` (mirror `metrics.store.spec.ts` setup — read it for the BRIDGE-mock pattern used in this repo):

```ts
import { TestBed } from "@angular/core/testing";
import { BRIDGE, Bridge, Events } from "../data-source/bridge";
import { CostSnapshot } from "../models";
import { CostStore } from "./cost.store";

function mockBridge(): { bridge: Bridge; fire: (e: string, p: unknown) => void } {
  const handlers: Record<string, (p: unknown) => void> = {};
  const bridge: Bridge = {
    invoke: async () => ({ totalCost: 0, currency: "USD", available: false }) as never,
    on: async (event, handler) => {
      handlers[event] = handler as (p: unknown) => void;
      return () => {};
    },
    pickDirectory: async () => null,
  };
  return { bridge, fire: (e, p) => handlers[e]?.(p) };
}

describe("CostStore", () => {
  it("mirrors the latest system://cost payload into the signal", async () => {
    const { bridge, fire } = mockBridge();
    TestBed.configureTestingModule({ providers: [CostStore, { provide: BRIDGE, useValue: bridge }] });
    const store = TestBed.inject(CostStore);
    await Promise.resolve(); // let init() subscribe

    const snap: CostSnapshot = { totalCost: 42.5, currency: "USD", available: true };
    fire(Events.SystemCost, snap);
    expect(store.cost()?.totalCost).toBe(42.5);
    expect(store.cost()?.available).toBe(true);
  });
});
```

- [ ] **Step 2: Run it — verify it fails**

Run: `pnpm test -- cost.store`
Expected: FAIL — `CostStore` does not exist / cannot import.

- [ ] **Step 3: Implement CostStore**

Create `src/app/orchestra/metrics/cost.store.ts` (mirror `MetricsStore`):

```ts
import { inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { CostSnapshot } from "../models";

/**
 * Global Claude cost total from ccusage. The backend pushes a fresh
 * `system://cost` payload every 60s; this store mirrors the latest into a signal
 * the status bar reads. Null until the first push / initial fetch. When the
 * payload's `available` is false (ccusage couldn't run) the status bar hides it.
 */
@Injectable({ providedIn: "root" })
export class CostStore {
  private bridge = inject(BRIDGE);
  readonly cost = signal<CostSnapshot | null>(null);

  constructor() {
    void this.init();
  }

  private async init() {
    try {
      await this.bridge.on<CostSnapshot>(Events.SystemCost, (c) => this.cost.set(c));
    } catch {
      // backend unavailable — readout stays hidden
    }
    try {
      const initial = await this.bridge.invoke<CostSnapshot>(Commands.SystemCost);
      if (this.cost() === null) this.cost.set(initial);
    } catch {
      // optional command — fine to skip
    }
  }
}
```

- [ ] **Step 4: Run the test — verify it passes**

Run: `pnpm test -- cost.store`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/orchestra/metrics/cost.store.ts src/app/orchestra/metrics/cost.store.spec.ts
git commit -m "feat(cost): CostStore subscribing to system://cost"
```

---

## Task 7: Status-bar readout

**Files:**
- Modify: `src/app/orchestra/status-bar/status-bar.component.ts`

- [ ] **Step 1: Read the component**

Read `src/app/orchestra/status-bar/status-bar.component.ts` to find where the CPU/RAM metrics readout is rendered (it injects `MetricsStore`). The cost readout sits beside it.

- [ ] **Step 2: Inject CostStore + render the value**

Add the import and inject:

```ts
import { CostStore } from "../metrics/cost.store";
// ...in the class:
  readonly costStore = inject(CostStore);
```

In the template, next to the CPU/RAM readout, add (match the surrounding readout's inline-style/markup conventions — copy a sibling readout's wrapper):

```html
@if (costStore.cost()?.available) {
  <span class="tnum" style="font-size:10px;color:var(--ink-3)" title="Total Claude cost (ccusage)">
    ${{ costStore.cost()!.totalCost.toFixed(2) }}
  </span>
}
```

- [ ] **Step 3: Build to typecheck the template**

Run: `pnpm build`
Expected: clean (pre-existing bundle-budget warning is OK).

- [ ] **Step 4: Commit**

```bash
git add src/app/orchestra/status-bar/status-bar.component.ts
git commit -m "feat(cost): show total cost in the status bar"
```

---

## Task 8: Manual verification

- [ ] **Step 1: Run the app**

Run: `pnpm dev`
Expected: within ~60s (or immediately, via the one-shot prime) a `$X.XX` appears in the status bar next to CPU/RAM. With ccusage uninstalled/unavailable, the readout is absent and the app is otherwise unaffected.

---

## Self-review notes

- **Spec coverage:** global total from `ccusage daily --json` (Task 2), 60s push on `system://cost` (Task 4), no persistence (no DB writes anywhere), hidden-when-unavailable (Tasks 2/6/7), pinned dependency (Task 1). ✓
- **Deferred (per spec):** per-agent cost, katrix-only scoping, time windows, non-Claude tools — none implemented here, by design.
- **Type consistency:** Rust `CostSnapshot { total_cost, currency, available }` (camelCase serde) ↔ TS `CostSnapshot { totalCost, currency, available }`. ✓
