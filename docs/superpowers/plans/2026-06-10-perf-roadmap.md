# Performance Roadmap for Katrix

> **For agentic workers:** This is a ROADMAP, not an executable implementation plan. Each technique below states current state, what changes, impact, and effort. Expand a phase into a full bite-sized TDD plan (superpowers:writing-plans) before implementing.

**Goal:** Apply proven terminal-pipeline, memory-bounding, and perf-guardrail techniques to Katrix so the app stays responsive and memory-bounded with many parallel agents producing PTY floods.

**Architecture:** Katrix is Tauri 2 (Rust: `portable_pty` per agent, raw `app.emit("agent://output")` per 4KB read) + Angular 20 (one persistent xterm per agent via `TerminalService`, direct writes, WebGL per terminal). The roadmap turns this naive pipe into a budgeted pipeline: batch in Rust → seq numbers → ACK backpressure → shared renderer scheduler with lossy hidden caps → Rust-owned snapshot recovery, plus laziness, no-op-publish elimination, and CI perf gates.

**Tech Stack:** Rust (portable_pty, tauri), Angular 20 signals, @xterm/xterm 6 + WebGL addon, vitest, existing DevConsole perf panel.

---

## Current-state facts (verified 2026-06-10)

- `src-tauri/src/runtime/mod.rs:143-157` — reader thread emits one `agent://output` Tauri event per ≤4KB read, JSON `{id, chunk}`. No batching, no seq, no backpressure.
- `src/app/terminal.service.ts:158-162` — every chunk is written immediately into the agent's xterm, **even when hidden**; hidden terminals burn CPU parsing everything.
- `src/app/terminal.service.ts:53-55` — every parsed chunk bumps a shared `revision` signal with a **new map object**, waking every `rev()` consumer on every chunk of every agent.
- WebGL addon loaded once per terminal at attach; context loss → dispose (DOM fallback). No context-count cap, no software-GL policy, no post-attach refresh.
- xterm scrollback capped at 5000 rows (good). File watcher already debounced (`watch/mod.rs`: 200ms settle, 1s max burst). CodeMirror already lazy (`workspace/code-lang.ts`). Startup already collapsed to sub-100ms (perf-global-fix, merged 2026-06-10).
- No e2e perf harness; vitest only. DevConsole perf panel ships in prod.

---

## Phase 1 — The pipeline core (biggest wins)

### 1. Batch PTY output at the source (Rust) — **DONE 2026-06-10 (feat/perf-pipelines)**
- **Current:** one event per 4KB read → under a TUI flood that's hundreds–thousands of IPC events/sec per agent, each with JSON serialization and a webview wakeup.
- **Change:** in `runtime/mod.rs`, the reader thread accumulates into a pending `String` and flushes on either **8ms elapsed** or **16KB pending**, whichever first; flush remaining bytes before exit so `agent://exit` never overtakes output. One new `output_batcher.rs` module with unit tests (flush-on-size, flush-on-time, flush-on-drop ordering).
- **Impact:** event rate drops to ≤~125/sec/agent regardless of throughput; this is the single biggest CPU saving for multi-agent load. Adds ≤8ms latency to echo (mitigated by #2 if noticeable).
- **Effort:** S. **Files:** `src-tauri/src/runtime/mod.rs`, new `src-tauri/src/runtime/output_batcher.rs`.

### 2. Sequence numbers on every batch — **DONE 2026-06-10 (emitted as cumulative bytes; frontend consumption lands with #8)**
- **Current:** none — restore/dedup impossible.
- **Change:** per-agent monotonic `seq` (bytes written so far) added to the batch payload `{id, chunk, seq}`. Frontend stores last-seen seq per agent.
- **Impact:** zero on its own, but it is the foundation for snapshot restore (#8) and lossy caps (#4) without duplication. Do it inside #1 — nearly free.
- **Effort:** XS (bundled with #1).

### 3. Shared renderer write scheduler — **DONE 2026-06-10 (visibility via term.element.isConnected, not the workspace store)**
- **Current:** `TerminalService.write()` calls `term.write()` directly for every agent. Each xterm schedules its own parse work; N flooding agents starve the focused terminal; hidden terminals parse everything at full cost.
- **Change:** new `src/app/terminal-output-scheduler.ts`:
  - **Visible terminal:** write immediately (latency path).
  - **Hidden terminals:** queue; one global drain loop (16ms tick) writes max 2×16KB chunks per tick across all hidden terminals, round-robin.
  - `TerminalService.write()` routes through the scheduler; visibility comes from the workspace store (active agent id).
- **Impact:** typing/echo in the focused terminal stays smooth no matter how many background agents flood; hidden parse cost is spread instead of immediate. This is the core frontend win.
- **Effort:** M. **Files:** new `terminal-output-scheduler.ts` (+ spec), `terminal.service.ts`, workspace store read.

### 4. Lossy hidden-output cap (2MB) with warning — **DONE 2026-06-10 (drops the middle, keeps warning + newest tail)**
- **Current:** hidden queue from #3 would grow unbounded if an agent dumps 100MB while you look elsewhere; today the equivalent failure is unbounded synchronous parsing.
- **Change:** in the scheduler, cap each hidden queue at **2MB chars / 4096 chunks**. On overflow: drop the backlog, replace with one warning line (`[skipped hidden output >2MB]`), mark the terminal **stale** (recovery flag for #8). Counters for drops (#6).
- **Impact:** hard memory bound per terminal; the app can no longer be stalled or OOMed by a noisy background agent.
- **Effort:** S (inside #3). **Files:** `terminal-output-scheduler.ts`.

### 5. Kill no-op / per-chunk signal fanout — **DONE 2026-06-10 (per-agent revision signals + 80ms liveLogs coalescer)**
- **Current:** `revision.update((m) => ({ ...m, [id]: n+1 }))` per parsed chunk — new object identity wakes **every** subscriber (overview mini-terms, etc.) on **every** chunk of **every** agent.
- **Change:** replace the map signal with per-agent `WritableSignal<number>` instances (`Map<string, WritableSignal<number>>`, created lazily); consumers subscribe to only their agent's signal. Coalesce bumps to at most once per drain tick for hidden terminals. Audit the split stores for the same pattern (return same reference when nothing changed).
- **Impact:** Angular effect/CD churn during floods collapses from O(chunks × consumers) to O(visible consumers); cheap and immediate.
- **Effort:** S. **Files:** `terminal.service.ts`, `stores/*.store.ts` audit, overview mini-term consumer.

### 6. Debug counters in the hot paths — **DONE 2026-06-10 (`agent_output_emit`/`agent_scan` perf rows; scheduler counters in dev-panel footer; backend-only rows now rate from exec `calls10s`)**
- **Current:** DevConsole perf panel exists but has no terminal-pipeline visibility.
- **Change:** counters on both sides, surfaced in the existing perf panel:
  - Rust: events emitted/sec, pending bytes, peak pending, batches flushed by size vs time.
  - Frontend scheduler: queued chars (current/peak), drain writes per tick, dropped backlogs, per-agent queue depth.
- **Impact:** makes every other item verifiable ("did batching actually reduce event rate?") and turns future regressions into a 2-minute diagnosis.
- **Effort:** S. **Files:** `output_batcher.rs`, `terminal-output-scheduler.ts`, `src/app/perf/*`.

**Phase 1 verification:** E2E test that spawns a synthetic flood agent (script printing MBs of ANSI), asserts focused-terminal input echo stays interactive, hidden queue ≤2MB, drop counter behaves. Unit tests per module.

---

## Phase 2 — Safety under load

### 7. ACK-based backpressure (Rust ⇄ webview)
- **Current:** Rust emits unconditionally. If the webview is throttled/hidden, events pile up in the bus with no bound.
- **Change:** frontend ACKs consumed bytes after xterm's `write` callback (one `runtime_ack(id, bytes)` Tauri command, coalesced per drain). Rust tracks in-flight bytes per agent (cap 512KB) + global (cap 8MB); the reader thread pauses reading the PTY when over the cap (the kernel PTY buffer then backpressures the agent process itself). ACK in `finally`-equivalent so a frontend error can't wedge a PTY.
- **Impact:** bounded worst case end-to-end; a hidden window can no longer accumulate unbounded event-bus memory. Pausing the read also sheds work at the producer.
- **Effort:** M. **Files:** `runtime/mod.rs`, `agents/commands.rs`, `terminal-output-scheduler.ts`.

### 8. Rust-owned terminal state as recovery source
- **Current:** terminal truth lives only in renderer xterms. After #4 drops a backlog, that output is gone; a webview reload loses every terminal.
- **Change:** Rust keeps a bounded **raw-byte ring buffer** per agent (e.g. last 1MB) fed by the same reader thread, tagged with seq. New command `runtime_snapshot(id) -> {bytes, end_seq}`. Frontend recovery: when a stale (#4) terminal becomes visible — or after webview reload — `term.clear()`, replay snapshot, then drop queued live chunks with `seq ≤ end_seq` and resume. (A VT-aware headless terminal model is a later upgrade; a raw ring buffer re-seeds xterm fine since the TUI repaints.)
- **Impact:** makes the lossy cap user-invisible in practice; terminals survive reloads; foundation for any future remote/mobile view.
- **Effort:** M. **Files:** new `src-tauri/src/runtime/scrollback.rs`, `runtime/mod.rs`, `agents/commands.rs`, `terminal.service.ts`.

### 9. WebGL context discipline
- **Current:** one WebGL context per agent terminal, loaded at first attach, kept while hidden. Browsers cap contexts (~8–16); with many agents the oldest contexts get reclaimed → silent context-loss → permanent DOM fallback (your handler disposes and never retries).
- **Change:** load the WebGL addon only for **visible** terminal(s); dispose it on detach/hide (keep the xterm instance); re-attach + `term.refresh(0, rows-1)` on show (a freshly attached WebGL canvas starts blank until repainted). Keep context-loss → DOM fallback as the terminal-scoped escape hatch.
- **Impact:** no context-loss storms at high agent counts; GPU memory scales with visible panes, not total agents.
- **Effort:** S-M. **Files:** `terminal.service.ts` (`attach`/detach + `loadWebgl`/`disposeWebgl`).

### 10. Push-based git scans — make `agent_changes`/`agent_commits` *called less*, not cached — **DONE 2026-06-10 (feat/perf-pipelines; bonus: gitdir watch makes agent-driven commits refresh the UI — they never did before)**
- **Current (verified):** `watch/mod.rs` emits a dumb ping (`agent://changed` {id}) after debounce; the frontend reacts by **pulling** `agent_changes` (full git2 status with line counts) for that agent — every burst, every agent, regardless of visibility or whether anything user-visible actually changed. Commits are refreshed optimistically after actions even when HEAD didn't move. Startup pulls `loadChanges` for every agent at once. The waste is the ping→pull architecture, not IPC or missing caches.
- **Change (Rust — the watcher becomes the producer):** in the existing per-agent debounce thread (`watch/mod.rs:68-72`), instead of emitting a ping:
  1. **Scan once at the source.** Run the git2 status scan in Rust right there (global semaphore ~2-3 scans, active agent first; per-worktree serialization is free since each agent already has exactly one debounce thread).
  2. **Suppress no-op results.** Fingerprint the scan result (hash of `(path, status, +/-)` rows + HEAD oid). If identical to the last emitted fingerprint → **emit nothing**. Build noise in ignored dirs stops waking the UI entirely.
  3. **Push the result, split by visibility.** Emit `agent://changes` carrying the data itself: full file list (with line counts) for the **active** agent; cheap summary (file count, +/- totals — status without per-file diffs) for background agents. A tiny `set_active_agent` command tells Rust which agent is on screen.
  4. **HEAD oid in every push.** Reading `head().target()` costs microseconds. The frontend refreshes commits **only when the oid moved** (and only if the feed was ever opened). Post-action optimistic `refreshCommits` is deleted — a real commit moves HEAD, so the next push triggers the refresh naturally.
  5. **Startup = initial scan push.** Watch registration triggers one initial scan per agent through the same semaphore (active agent first). The frontend issues zero startup `agent_changes` calls.
- **Change (frontend):** `agent-work.store.ts` consumes `agent://changes` pushes instead of calling `loadChanges` on watcher events; background agents' full file lists are marked **dirty** and fetched once on panel open (the only remaining pull path, plus user-driven commit paging). Gen guards stay as safety net.
- **Commits initial load** (the one cost pushes can't remove): make the first page cheaper by deferring per-commit diff stats — return the revwalk list fast, fill stats lazily per visible row. No caching involved.
- **Impact:** steady-state `agent_changes` calls drop from (bursts × agents) to **zero** — data arrives with the notification, and only when it actually changed. `agent_commits` runs only on real HEAD movement plus user paging. Background agents cost a cheap summary scan instead of a full line-count status. Startup stops stampeding the disk.
- **Effort:** M. **Files:** `src-tauri/src/watch/mod.rs` (scan + fingerprint + push), `src-tauri/src/git/service.rs` (summary scan, stats-deferred commits page), `src-tauri/src/agents/commands.rs` (`set_active_agent`), `src/app/agents/agent-work.store.ts` + `agent-runtime.service.ts` (consume pushes, dirty-on-view), `data-source/bridge.ts` (new event).

### 10b. Bounded-buffer audit (everything else)
- **Current:** watcher already debounced ✓; commits feed already paged ✓; xterm scrollback capped ✓. Remaining: hooks transcript tails, cost/ccusage payloads, metrics history, notification queues.
- **Change:** sweep each accumulation site; give every buffer a cap and a defined overflow behavior (drop-oldest, collapse-to-refresh-signal, or page). Document the cap with a `// Why:` comment.
- **Impact:** no slow-leak surprises in long sessions (Katrix runs for days).
- **Effort:** S per site.

---

## Phase 3 — UX polish (do when symptoms appear)

### 11. Interactive fast lane (Rust batch bypass)
- **When:** only if the 8ms batch from #1 makes typing feel sluggish (measure first via #6/#14).
- **Change:** record `last_input_at` per agent when `input()` writes to the PTY; output within 100ms of input that is ≤16KB and contains `\x1b[` (or ≤1KB of anything) bypasses the batcher, within a 32KB per-burst budget.
- **Impact:** keystroke echo latency ≈ raw, floods still batched. **Effort:** S-M.

### 12. Spread restores/wakes across frames
- **When:** after #8/#13 exist. **Change:** small frontend queue: restore the visible terminal immediately, then one hidden replay per 16ms frame (a ~60-line restore queue module).
- **Impact:** switching back to Katrix after agents ran for an hour doesn't jank. **Effort:** S.

### 13. Sleep idle agents
- **Current:** every agent's xterm (≤5000 rows + DOM/canvas) lives until agent deletion.
- **Change:** "sleep" = dispose the xterm handle (Rust ring buffer #8 keeps the truth); wake = recreate + replay snapshot. Manual action on the agent card + auto-sleep policy (e.g. exited agents after 10 min). Show a per-agent memory hint next to the action.
- **Impact:** memory scales with *active* agents; matters once users run 10+ parallel agents. **Effort:** M (needs #8).

### 14. Input-quiet + idle deferral
- **Change:** a small `scheduleAfterInputQuiet` util (delay + no keyboard/pointer/wheel for N ms + `requestIdleCallback`); use it for non-urgent heavy work: cost refreshes, overview tail recomputes, terminal re-attach on workspace switch.
- **Impact:** fewer dropped frames right after the user interacts. **Effort:** S.

### 15. Synchronized-output frame holding
- **When:** only if you observe TUI flicker (half-drawn Claude Code/Codex frames). **Change:** in the scheduler, detect DEC 2026 begin/end (`\x1b[?2026h/l`); hold mid-frame chunks until the end marker, with a 250ms safety timer (32ms when latency-sensitive) so a missing marker can't freeze a pane.
- **Impact:** no transient half-frames rasterized. **Effort:** M — defer until observed.

---

## Phase 4 — Guardrails (lock the wins in)

### 16. Lazy-boundary regression test
- **Current:** startup is fast and CodeMirror is lazy, but nothing stops a future static import from regressing it.
- **Change:** a vitest that reads source files and asserts the boundaries (`expect(src).toContain('import("@codemirror')`, route-level `loadComponent`/`@defer` markers, no static `@codemirror/*` import outside `code-lang.ts`).
- **Impact:** sub-100ms startup can't silently regress. **Effort:** XS.

### 17. CI perf budgets
- **Change:** a `perf-smoke` harness: launch the app in dev mode with a flag that spawns a synthetic flood agent, sample the #6 counters + an echo-latency proxy (timestamp in Rust emit → xterm parse callback), write a JSON report; a node script asserts budgets (suggested: median echo ≤75ms, worst ≤300ms, hidden queue peak ≤2MB, dropped backlogs = 0 under normal load) and fails CI.
- **Impact:** every Phase 1–2 win becomes a permanent invariant instead of a one-time fix. **Effort:** M.

### 18. Virtualize long lists
- **Current:** commits feed paged ✓. Sidebar agent/project lists are small today.
- **Change:** adopt CDK `cdk-virtual-scroll-viewport` only when a list can realistically exceed ~100 rows (e.g. agent history). Skip otherwise — virtualization has its own complexity tax (it tends to demand custom scroll anchoring).
- **Impact:** conditional. **Effort:** S when needed.

### 19. Why-comments + profiling lab notes
- **Current:** the codebase already does why-comments well; perf captures exist.
- **Change:** make it a convention for the new pipeline: every magic constant (8ms, 16KB, 2MB, 512KB…) carries a `// Why:`; each profiling session gets a dated note in `docs/perf/` with findings → fixes → validation (lab-notebook style: scope, live evidence, findings, changes, remaining risk).
- **Impact:** the constants stay tunable by someone who isn't you-today. **Effort:** trivial, ongoing.

---

## Suggested order & dependencies

```
Phase 1: 1+2 (Rust batch+seq) → 3+4 (scheduler+cap) → 5 (fanout) → 6 (counters)
Phase 2: 7 (ACK, needs 1) → 8 (snapshot, needs 2) → 9 (WebGL) → 10 (command pipeline, independent — can run parallel to Phase 1) → 10b (audit)
Phase 3: 11,12,13,14,15 as symptoms/needs appear (13 needs 8)
Phase 4: 16 now (cheap), 17 after 6, 18/19 ongoing
```

Phase 1 alone removes the two worst behaviors: per-chunk IPC storms and focused-terminal starvation. Phase 2 makes the system safe under pathological load. Everything after is polish and insurance.
