# Orrery Roadmap — Performance/Efficiency + IDE DX

> **For agentic workers:** This is a ROADMAP, not an executable implementation plan.
> Each item states current state, the change, impact, effort, and touched files.
> Expand a phase into a bite-sized TDD plan (`superpowers:writing-plans`) before implementing.

**Positioning:** Orrery is the AGent IDE that is light on **RAM**, light on **CPU**, and light on
**tokens** — with IntelliJ-grade git as the wedge. Every git operation implemented natively is work
the LLM no longer pays tokens to do. Both paths (native + AI) stay available for every applicable
operation; the AI path always shows its price before it runs.

**Architecture:** Tauri 2 (Rust: `git2`, `portable_pty`, `notify`, `ignore`) + Angular 20 standalone
+ signals, CodeMirror 6 (lazy, Lezer), `@xterm/xterm` 6 + WebGL. Data flows through a typed
`BRIDGE`/`Commands` invoke layer plus `*://*` Tauri events.

**Tech stack:** Rust (git2, portable_pty, notify, ignore, tauri 2, sysinfo, toml_edit), Angular 20,
CodeMirror 6, xterm 6, vitest, cargo test, Playwright, GitHub Actions.

---

## Table of contents

- [Current-state ground truth](#current-state-ground-truth)
- [Phase 0 — Platform unblock](#phase-0--platform-unblock)
- [Section A — Performance & Efficiency](#section-a--performance--efficiency)
  - [A0. Process model & interest subscription](#a0-process-model--interest-subscription)
    - *(A0.7 — emit telemetry now · prioritized scheduler later)*
  - [A1. Terminal pipeline — finish it](#a1-terminal-pipeline--finish-it)
  - [A2. Watcher & git-scan efficiency](#a2-watcher--git-scan-efficiency)
  - [A3. Native git engine](#a3-native-git-engine)
  - [A4. Dual-path git actions (native + AI, always both)](#a4-dual-path-git-actions-native--ai-always-both)
  - [A5. Token efficiency](#a5-token-efficiency)
  - [A6. Token & cost telemetry](#a6-token--cost-telemetry)
  - [A7. Guardrails](#a7-guardrails)
    - *(A7.7 — recursive process tree)*
- [Section B — IDE DX](#section-b--ide-dx)
  - [B1. Editing](#b1-editing)
  - [B2. Navigation](#b2-navigation)
  - [B3. Search](#b3-search)
  - [B4. Git UX](#b4-git-ux)
  - [B5. Terminal](#b5-terminal)
  - [B6. Workspace](#b6-workspace)
  - [B7. Integrations](#b7-integrations)
- [Sequencing](#sequencing)
- [Acceptance budgets](#acceptance-budgets)
- [Non-goals](#non-goals)

---

## Current-state ground truth

Verified against the repo before writing this roadmap. **Do not re-plan what is already done.**

**Already landed (perf):**
- PTY output batched in Rust (8ms / 16KB flush), `seq` emitted — `runtime/output_batcher.rs`
- Shared frontend write scheduler with hidden-terminal cap (2MB chars / 4096 chunks), lossy drop
  + `[hidden output skipped…]` marker — `terminal-output-scheduler.ts`
- Per-agent `WritableSignal<number>` revisions (no map-identity fanout), 80ms `liveLogs` coalescer
- `perf::timed` + `perf://stats` (exec vs overhead columns live), scheduler counters in dev panel
- Blanket async commands + `spawn_blocking` heavy set; startup collapsed to sub-100ms
- Push-based git scans: watcher runs status + HEAD oid itself, fingerprint-suppresses no-ops,
  watches the linked worktree's gitdir so agent-side commits fire
- Lazy `AgentWorkStore` (`Loadable` maps, generation guards), paged commits
- CodeMirror lazy-loaded behind `code-lang.ts`; xterm scrollback capped at 5000 rows
- Watcher debounce: 200ms settle, 1s max burst
- **Output mux with focus-aware cadence** — `runtime/output_mux.rs`: one process-wide drain thread
  emits a single array-payload `agent://output` per ~16ms frame for *all* agents. `set_focus(id)`
  puts the focused agent on the per-frame path and holds everyone else to a ~150ms cadence,
  coalescing losslessly. A never-drained agent's first output always ships immediately.
- Per-agent writer/master `Arc<Mutex<…>>` so a wedged ConPTY write can't stall the procs map
- `core/proc.rs` is the only sanctioned `Command` constructor (`CREATE_NO_WINDOW`), enforced by
  clippy `disallowed-methods`

**Process-model facts (verified, and load-bearing for A0):**
- Agents are spawned **directly** into a ConPTY — `CommandBuilder::new("claude")`, no shell.
- **Except** when argv[0] resolves to a shim: `wrap_for_ext` wraps `.cmd`/`.bat` in
  `cmd.exe /c call` and `.ps1` in `powershell_program() -NoProfile -File`. `prefers_pwsh()` ranks
  `.ps1` **above** `.cmd` when PowerShell 7 is present — so an npm-installed agent on a machine
  with pwsh gets a long-lived `pwsh.exe` parent per agent.
- Windows Terminal is **not** in the pipeline at any point. It is a terminal emulator application;
  Orrery's emulator is xterm.js. It is irrelevant to this roadmap and should stop being considered.
- ConPTY allocates a `conhost.exe`/`OpenConsole.exe` per PTY. Unavoidable *while PTYs are used*
  (see A0.5).

**Already landed (git):**
- `init`/detect · paged `log` · `status` · `file_diff` · `commit` (per-file selection) · `discard`
- `merge` (FF or merge commit, **bails on conflict**) · `push` (origin) · `branches` (list only)
- `head_info`/`head_oid` · worktree `create`/`remove` · `ensure_main_branch`
- `diff_treeish` · `commit_files` · `commit_file_diff` · `range_file_diff`
- `blame` (with IntelliJ-style age fade gutter) · `file_history`

**Already landed (product):**
- Worktree-per-agent, 4 adapters (claude/codex/cursor/gemini) behind `AgentAdapter`
- Global hook install merged non-destructively into the user's real configs; `ORRERY_*` env gate
- Permission cards, auto-approve policy, resume-by-session-id
- Backlog/tickets with dispatch → spawn, inline diff review comments → agent
- Auto-updater (minisign), landing site + changelog

**Known holes this roadmap closes:** Windows-only; read-only editor; no fetch/pull/remotes; no
branch ops; conflict resolution absent; file-level discard only; no project search; no command
palette; no commit graph; rebase/merge are AI-delegated with no native path and no cost disclosure;
no token accounting.

---

## Phase 0 — Platform unblock

> Nothing in Section A or B produces market value while the app runs on one OS.
> This is not optional and it is not last.

### 0.1 macOS build (arm64 + Intel)
- **Current:** `release.yml` runs `windows-latest` only; `make-latest-json.mjs` writes one
  `platforms.*` entry; macOS/Linux explicitly out of scope in the release plan.
- **Change:** add a `strategy.matrix` over `windows-latest` / `macos-14` (arm64) / `macos-13`
  (x64); extend `make-latest-json.mjs` to emit `darwin-aarch64` and `darwin-x86_64` entries from
  the flat-staged artifacts. Handle the `.app.tar.gz` + `.sig` updater pair.
- **Blockers to resolve:** Apple Developer ID signing + notarization (unsigned macOS builds are
  effectively undistributable, unlike the Windows SmartScreen warning we accept today).
- **Path handling audit:** every `replace('\\', "/")` and `Path` join in `git/service.rs`,
  `watch/mod.rs`, `runtime/mod.rs` — Windows-specific assumptions must be exercised in CI.
- **Effort:** M. **Files:** `.github/workflows/release.yml`, `scripts/release/make-latest-json.mjs`,
  `src-tauri/tauri.conf.json`, path audit across `src-tauri/src/**`.

### 0.2 Linux build
- **Change:** add `ubuntu-22.04` to the matrix (AppImage + `.deb`); `linux-x86_64` entry in
  `latest.json`. Verify `portable_pty` and `notify` inotify limits (raise the watch-descriptor
  guidance in docs; inotify caps out around 8k watches by default — relevant at 100 projects).
- **Effort:** S-M once 0.1 exists.

### 0.3 Cross-platform E2E in CI
- **Change:** run the Playwright suite on all three OSes. Add one test per platform-sensitive
  surface: worktree create/remove, watcher fires, PTY spawn + echo, terminal resize.
- **Effort:** S. **Files:** `.github/workflows/ci.yml`, `e2e/*`.

---

# Section A — Performance & Efficiency

## A0. Process model & interest subscription

> **Upstream of everything else in Section A.** A0.1–A0.4 change *what work exists at all*;
> A1 only makes the remaining work cheaper. A0.5 is a fork in the road that, if taken, deletes most
> of A1 outright — decide it before investing further in the PTY pipeline.

### A0.1 Kill the per-agent shell wrapper (Windows)
- **Current:** direct ConPTY spawn for real `.exe` installs. For npm-style installs the launch
  becomes `cmd.exe /c call claude.cmd …` or, when PowerShell 7 is present, `pwsh -NoProfile -File
  claude.ps1 …` — a wrapper process that lives for the agent's entire run.
- **Cost (approximate — measure with the existing `pids()` sysinfo sampler before acting):**
  pwsh ~60–100MB RSS, cmd.exe ~3–5MB, conhost ~5–10MB. Ten pwsh-wrapped agents is most of a
  gigabyte of pure wrapper overhead, and it is invisible in Orrery's own RSS.
- **Change, in order of preference:**
  1. **Resolve the shim.** `claude.cmd` is a few lines of batch ending in `node <path>/cli.js`.
     Parse it once, cache the resolution per tool version, and spawn `node cli.js` directly. This
     deletes the wrapper process entirely, shortens the kill chain (fewer layers between
     `child.kill()` and the real process), and yields the node process you were always going to get.
  2. Rank a real `.exe` above every shim **globally**, not only within a single PATH directory.
  3. If a shim must be used, prefer `cmd.exe` over pwsh on RAM grounds — and re-test the
     "pwsh runs npm's ps1 shim more smoothly" note in `resolve_program`, which may have been a
     quoting problem already solved by the `/c call` fix.
- **Also:** detect a shim-based install and surface a one-line hint recommending the native
  installer, with the measured RAM difference shown.
- **Impact:** removes one process per agent on the most common Windows install path.
- **Effort:** S-M. **Files:** `runtime/mod.rs` (`resolve_program`, `wrap_for_ext`),
  `agents/adapters/mod.rs` (`prefers_pwsh`, `launch_prefix`).

### A0.2 Interest subscription (supersedes single-focus cadence)
- **Current:** `output_mux::set_focus(Option<id>)` — exactly one agent on the fast path, everyone
  else held to ~150ms but **still shipped**. That is cadence throttling, not interest.
- **Reality it doesn't model:** terminal A open, mini-previews for B and C, diff view on D,
  nothing at all for E–T. Four different needs, one boolean.
- **Change:** the frontend publishes an **interest set** with per-agent modes; the backend diffs it
  against the current set on each call.

  ```
  runtime_subscribe([
    { id: "a", mode: "stream" },   // full output, per-frame path
    { id: "b", mode: "digest" },   // last N lines @ 1Hz, tiny payload
    { id: "c", mode: "digest" },
    { id: "d", mode: "none"   },   // diff view is open — PTY output is irrelevant
  ])
  ```

  Anything absent from the set is `none`. Recompute on tab switch, pane layout change, overview
  scroll, and window blur (a blurred window is `digest` at most).
- **THE CONSTRAINT — do not get this wrong:** `none` means *do not emit*. It must **never** mean
  *do not read*. If the reader thread stops draining the PTY, the kernel buffer fills and the agent
  process **blocks on write** — you would stall the agent itself, not merely its display. The reader
  and batcher threads keep running for every agent regardless of mode.
- **Where the bytes go instead:** the bounded ring buffer (A1.2). This is why A1.2 is a hard
  prerequisite, not a nice-to-have — today an unheld agent's output accumulates in the mux's
  `pending` String, bounded only by the 150ms cadence. Under `none` with no drain, that String is
  unbounded. The ring is what makes `none` safe.
- **On resubscribe:** replay from the ring (dedup by `seq`, exactly the A1.2 recovery path), so
  switching to a background agent shows its recent history rather than a blank pane.
- **`digest` mode:** Rust keeps the last N rendered lines per agent and pushes a small payload at
  1Hz. This is what the overview mini-terminals actually need — they currently consume the full
  stream to display five lines.
- **Status is unaffected:** blocked / needs-permission / done arrive over hooks, not PTY, so hidden
  agents stay fully observable. **Exception:** gemini's PTY-parsing fallback — move that parsing
  into Rust (A0.3) so it works without shipping bytes to the renderer.
- **Impact:** renderer work becomes proportional to *what is on screen*, flat in total agent count.
  This is the structural version of the win A1.5 approximates.
- **Effort:** M. **Files:** `runtime/output_mux.rs`, `runtime/mod.rs`, `agents/commands.rs`,
  `terminal.service.ts`, workspace/pane layout → subscription derivation.
- **Supersedes:** A1.5 (focus-aware cadence). Build A0.2 instead; do not build both.

### A0.3 Move PTY-derived heuristics into Rust
- **Current:** gemini has no permission hook, so its status is derived by parsing PTY text in the
  renderer. Under A0.2, a `none`-mode agent ships no text, which would silently break it.
- **Change:** move the fallback parsing next to the batcher in Rust, emitting a status event rather
  than requiring the renderer to see the stream. Same for the `promptTail` heuristics.
- **Bonus:** this is the natural home for the A5.4 loop detector — it already has the raw stream.
- **Effort:** S-M. **Files:** `runtime/`, `hooks/`, retire the equivalent renderer logic.

### A0.4 One watcher, N registered roots
- **Current:** per-agent watchers rooted at each worktree, plus the linked gitdir. Worktrees created
  outside the project folder (`git worktree add ../foo`, or a configured `worktreeRoot` elsewhere)
  are not covered by any "watch the project directory" scheme.
- **Change:** **one `notify` watcher instance per project with multiple registered paths** — `notify`
  supports N watch roots on a single watcher. Compute the root set from `repo.worktrees()` + the main
  workdir + the common gitdir (`.git/worktrees/<n>/`, where linked-worktree commits actually land —
  already handled today). Route events to the owning agent by longest-prefix match.
- **Subscription applies here too:** a project with no visible surface is unwatched entirely and
  re-scans on reveal. A diff view open on agent D does not require watching agents E–T.
- **Impact:** watchers scale with **projects**, not agents — decisive on Linux, where inotify
  descriptors are a hard per-user limit and 100 projects × N worktrees will exhaust them.
- **Also:** bound the event queue. A build writing 100k files must coalesce, not accumulate.
- **Effort:** M. **Files:** `watch/mod.rs`. **Replaces:** A2.1.

### A0.5 Headless mode — the fork in the road
> **Decide this before spending further effort on A1.** If headless becomes the default, A1.2,
> A1.3, A1.4, A1.6, A1.10 and A1.11 largely evaporate for headless agents.

- **Current cost per PTY agent:** a conhost process, possibly a shell wrapper (A0.1), a
  reader + batcher thread pair, an xterm.js instance (~4–8MB JS heap at 5000 rows), a WebGL
  context, and full ANSI parsing on every byte.
- **Observation:** most of these CLIs support non-interactive structured output — Claude Code's
  `--output-format stream-json`, and equivalents in the other tools. Verify per adapter; this is
  the first task, not an assumption.
- **Change:** run the agent over **plain pipes** in a `headless` launch mode. This deletes, per
  agent: conhost, the shell wrapper, the xterm instance, the WebGL context, and ANSI parsing —
  and replaces screen-scraping with **structured events**.
- **What you render instead:** a native chat / tool-call timeline built from the event stream.
  Arguably a better review surface than a TUI, and it composes with the existing inline-review and
  permission-card surfaces rather than fighting them.
- **Keep PTY mode** as a per-agent toggle for tools that need it, for debugging, and for anything
  whose structured mode is incomplete. The `AgentAdapter` trait is the right place for a
  `supports_headless()` + `headless_args()` pair, mirroring `supports_hooks()`.
- **Risks to check before committing:** does structured mode still fire hooks (permission cards
  depend on it)? Does resume-by-session-id work in headless? Does anything require a TTY to
  behave? Is the structured schema stable enough to depend on?
- **Impact:** the single largest change to the memory curve available — larger than all of A1
  combined.
- **Effort:** L. **Files:** `agents/adapters/*`, `runtime/`, new renderer timeline component.

### A0.6 Store eviction & data-shape fixes
Small, unglamorous, and collectively worth more than several A1 items.

- **Blame interning.** `BlameLine` carries a full author string and sha **per line**. A 50k-line
  file is megabytes of duplicated strings. Store a small commit table and per-line indices into it.
- **LRU the keyed stores.** `GitInspectStore` and `AgentWorkStore` maps grow for the process
  lifetime. Evict to the last 3–5 agents; entries reload lazily by design, so eviction is free.
- **Dispose CodeMirror on tab close**, and cap concurrent editor instances.
- **Flatten the file tree.** Parallel arrays with parent indices, not nested objects with
  `children[]`. At 100k nodes the object graph alone is the problem.
- **SQLite `cache_size`** — set it modestly rather than taking the default.
- **Verify no source maps ship in prod** (webview heap).
- **Surface agent RSS separately** in the status bar: `Orrery 210MB · agents 1.4GB`. You already
  sample agent pids for metrics. Users blame the IDE for the agent's memory; showing the split is
  both honest and good positioning.
- **Effort:** S each. **Files:** `git/service.rs` (blame shape), `agents/*.store.ts`,
  `workspace/*`, `core/database.rs`, status bar.

### A0.7 Emit transport — instrument now, schedule later

> **Split into two phases deliberately.** Phase 1 ships; Phase 2 is designed but **blocked on
> Phase 1's data**. Quanta and class boundaries picked without an inventory would be guesses
> dressed as engineering.

---

#### Phase 1 — Emit telemetry & inventory **(build this)**

- **Current:** every Rust→frontend push is a raw `app.emit(...)` at its own call site. Nobody knows
  how many events are sent, of what kind, at what size, or at what rate. `perf::record_io` covers
  exactly one channel (`agent_output_emit`).
- **Change — one funnel, no behavior change:**

  ```rust
  // core/emit.rs — records, then forwards. That is all it does in Phase 1.
  pub fn emit_tracked<R: Runtime, S: Serialize>(
      app: &AppHandle<R>, name: &str, payload: &S,
  ) -> tauri::Result<()>
  ```

  Every existing `app.emit` call site is rewritten to go through it, and raw `app.emit` is banned
  repo-wide via clippy `disallowed-methods` — the same enforcement trick already used for
  `Command::new` in `core/proc.rs`.

- **Why this ordering is the whole point:** the funnel and the ban ARE the scheduler's plumbing.
  Phase 2 becomes a change *inside one function*, not a second repo-wide refactor. The
  instrumentation is not throwaway work — it is the scheduler, minus the scheduling.

**Two recording modes, because they have opposite costs:**

| Mode | What it records | Cost | Default |
|---|---|---|---|
| **Aggregate** | per event name: count, total bytes, max payload, p50/p95 size, calls/10s | in-memory counters, flushed periodically | **always on** |
| **Raw trace** | one line per emit: `ts · name · key · bytes` | one append per emit | **off**, opt-in, bounded window |

The raw trace must **not** be always-on. Under the exact flood you are trying to measure it would
write hundreds of MB — you would be instrumenting the pathology by amplifying it.

**Where it lands.** `app_data_dir()/telemetry/`, separate from the normal log so it never pollutes
`core/logger.rs` output:

```
%APPDATA%/orrery/telemetry/emits-2026-08-03.ndjson     # raw trace, opt-in
%APPDATA%/orrery/telemetry/emit-summary-2026-08-03.json # aggregate, always on
```

NDJSON so it is appendable, streamable, and analyzable with `jq`/DuckDB without a parser. Daily
rotation, total cap ~50MB / 7 days, oldest pruned first.

**Privacy — non-negotiable.** Log **names, keys and byte counts only. Never payload contents.**
Payloads carry source code, prompts, and diffs. These files live on disk in app data and users
attach app-data folders to bug reports. If a payload sample is ever genuinely needed, it goes
behind a separate explicit flag with redaction, and it is never the default.

**Cardinality.** Aggregate by event *name*, not by key — keys are per-agent and would produce an
unbounded counter map. The raw trace may carry keys; the aggregate may not.

**Correlate with what was on screen.** Also record subscription-state transitions (A0.2). Emit
volume is uninterpretable without knowing that three agents were in `stream` mode during the
window — the same numbers mean opposite things at different subscription states.

**Ship the analysis with it.** `scripts/telemetry/summarize.mjs` reads a day's NDJSON and prints
the inventory table: event name · count · total bytes · p50/p95 payload · peak rate · suggested
class. **That table is the deliverable** — the point of Phase 1 is to produce the input to
decision 8, not to have logging for its own sake.

- **Effort:** S-M. **Files:** new `src-tauri/src/core/emit.rs`, `clippy.toml`, every current
  `app.emit` call site (mechanical), new `scripts/telemetry/summarize.mjs`, settings toggle for the
  raw trace, perf panel reads the aggregate.

---

#### Phase 2 — Prioritized scheduler **(designed; blocked on Phase 1 data)**

- **Change:** `emit_tracked` grows into `emit_scheduled(class, key, payload)` — the call sites and
  the clippy ban are already in place from Phase 1, so this touches one module.

**Three classes of handling — not three priorities.** The distinction matters: byte quotas are
correct for bulk streams and actively wrong for small urgent messages.

| Class | Examples | Handling | Why |
|---|---|---|---|
| **Bypass** | permission request, `agent://exit`, errors, command replies | emitted immediately, never queued | tiny and latency-critical; throttling 200 bytes buys nothing and costs responsiveness |
| **Coalesced state** | agent status, git scan result, cost, metrics | keyed map, **last write wins**, fixed cadence | delivering 50 superseded status updates in order is pure waste; only the latest is true |
| **Bulk weighted** | PTY output, search results, blame, large diffs | weighted **deficit round-robin** with byte quanta | this is where the priority-share idea belongs |

**Bulk scheduling detail.** Per drain window, each bulk class receives a byte quantum — starting
proposal `stream: 1MB · secondary: 512KB · background: 256KB`, tunable at runtime from the perf
panel. **Deficit round-robin, not strict weighting:** unused quantum carries forward as a deficit,
which guarantees a low-share class eventually drains instead of starving behind a permanently
saturated high-share one. Strict proportional sharing has no such guarantee and will starve
class 3 under sustained load.

**Memory bounds — the part that is easy to get wrong.** Holding low-priority payloads in Rust
moves the OOM from the renderer to the backend; it does not remove it. Every bulk class carries:
- a per-key cap and a per-class cap, both in bytes;
- an explicit drop policy — **drop-oldest** for PTY (the A1.2 ring is the source of truth, so
  dropping is recoverable), **drop-whole-queue-and-mark-stale** for regenerable results like search
  or blame (cheaper to recompute than to buffer);
- a counter per drop, surfaced in the perf panel next to the existing scheduler counters.

**Relationship to A0.2.** Orthogonal and complementary: subscription decides *whether* something
ships at all; priority decides *in what order and at what rate* the survivors ship. **Do A0.2
first** — it removes most of the traffic this scheduler would otherwise have to arbitrate, and
scheduling traffic nobody is looking at is wasted work.

**Measure before tuning.** The mux already caps emits at ~62/s regardless of agent count, so raw
event *count* is probably not the constraint — payload size and renderer parse cost are. Phase 1's
inventory table settles this. **Do not pick quanta before that table exists.**

- **Impact:** one bulk channel can no longer degrade another; urgent messages stop queueing behind
  bulk; backend memory under load becomes a stated bound rather than an emergent property.
- **Effort:** M (down from M-L — Phase 1 already did the repo-wide part). **Files:**
  `src-tauri/src/core/emit.rs` only, plus `runtime/output_mux.rs` (becomes the bulk-stream producer
  rather than its own scheduler) and the perf panel.
- **Entry condition:** at least one week of aggregate telemetry across a realistic session
  (5+ agents, mixed idle and flooding), plus one raw-trace window during a known flood.

---

## A1. Terminal pipeline — finish it

Phase 1 of the existing perf roadmap is done. These are the remaining items, restated with
current numbers.

### A1.1 ACK-based backpressure (Rust ⇄ webview)
- **Current:** Rust emits unconditionally. A throttled or hidden webview lets events pile up in
  the Tauri bus with no bound.
- **Change:** frontend ACKs consumed bytes after xterm's `write` callback via a single
  `runtime_ack(id, bytes)` command, coalesced per drain tick. Rust tracks in-flight bytes per agent
  (cap **512KB**) and globally (cap **8MB**); the reader thread stops reading the PTY above the cap
  so the kernel PTY buffer backpressures the agent process itself. ACK must fire in a
  `finally`-equivalent so a renderer exception cannot wedge a PTY forever.
- **Impact:** bounded worst case end-to-end; sheds work at the producer, not just the consumer.
- **Effort:** M. **Files:** `runtime/mod.rs`, `agents/commands.rs`, `terminal-output-scheduler.ts`.

### A1.2 Rust-owned scrollback ring as recovery source
- **Current:** terminal truth lives only in renderer xterms. When the hidden cap drops a backlog
  that output is gone; a webview reload loses every terminal.
- **Change:** bounded raw-byte ring buffer per agent (**1MB**) fed by the same reader thread, tagged
  with `seq`. New `runtime_snapshot(id) -> {bytes, endSeq}`. Recovery path: a stale terminal becoming
  visible (or a reload) does `term.clear()` → replay snapshot → drop queued live chunks with
  `seq <= endSeq` → resume.
- **Impact:** makes the lossy cap invisible in practice; terminals survive reloads; prerequisite for
  A1.4 (sleep) and for any future remote view.
- **Effort:** M. **Files:** new `src-tauri/src/runtime/scrollback.rs`, `runtime/mod.rs`,
  `agents/commands.rs`, `terminal.service.ts`.

### A1.3 WebGL context discipline
- **Current:** one WebGL context per agent terminal, created at first attach, kept while hidden.
  Browsers cap contexts (~8–16). At high agent counts the oldest contexts get reclaimed → silent
  context loss → the handler disposes and never retries → permanent DOM fallback.
- **Change:** load the WebGL addon only for **visible** terminals; dispose on hide (keep the xterm
  instance); re-attach + `term.refresh(0, rows-1)` on show, because a freshly attached canvas starts
  blank until repainted. Keep context-loss → DOM fallback as the per-terminal escape hatch.
- **Impact:** GPU memory scales with visible panes, not total agents.
- **Effort:** S-M. **Files:** `terminal.service.ts`.

### A1.4 Sleep idle agents
- **Current:** every agent's xterm (≤5000 rows + DOM/canvas) lives until the agent is deleted.
- **Change:** "sleep" disposes the xterm handle — the Rust ring (A1.2) holds the truth. Wake
  recreates and replays. Manual action on the agent card + an auto-sleep policy (exited agents after
  10 min; idle agents after 30 min, configurable). Show a per-agent memory hint next to the action.
- **Impact:** **memory scales with active agents, not total agents.** This is the single item that
  makes the "20 agents open" number defensible.
- **Effort:** M (needs A1.2). **Files:** `terminal.service.ts`, agent card, settings.

### A1.5 Focus-aware cadence — ~~PLANNED~~ **PARTLY DONE, THEN SUPERSEDED**
- **Done:** `output_mux::set_focus` already gives the focused agent the per-frame path and holds
  everyone else to ~150ms, losslessly.
- **Superseded by A0.2.** The remaining gap is not a faster cadence for hidden agents — it is not
  shipping their bytes at all. Build the interest subscription instead. Do not build both.

### A1.6 Kill the per-char folding path
- **Current:** `appendPtyTail` folds output per character to maintain `promptTail`, which is only
  read by gemini-only heuristics and exit handling.
- **Change:** store raw tail chunks in a bounded ring; run the fold **lazily**, only when
  `promptTail` is actually read. Rewrite `writeAt` to batch plain-text runs instead of rebuilding
  lines per character.
- **Impact:** largest remaining pure-JS win under flood. No behavior change for hook-driven tools.
- **Effort:** M.

### A1.7 Frontend micro-churn
- **Change:** gate `bumpStats` behind a dev-panel-open signal; stop patching the agents array from
  the 800ms elapsed tick — derive elapsed in the component from one shared `now` signal.
- **Effort:** S.

### A1.8 Background load
- **Change:** reuse the long-lived sysinfo sampler for the one-shot command (no cold double sweep);
  scope process refresh to known PIDs (agents + self) rather than `ProcessesToUpdate::All`; dedupe
  the ccusage startup one-shot against the push-loop's first run.
- **Effort:** S.

### A1.9 Procs-lock hazard
- **Change:** move PTY write/resize out from under the procs-map mutex (per-agent writer handle
  behind its own lock).
- **Impact:** removes a head-of-line stall where one slow agent blocks input to another.
- **Effort:** S.

### A1.10 Input fast lane *(conditional)*
- **When:** only if measured typing latency exceeds budget. The 8ms batch window is likely
  imperceptible — measure with A7.1 before building this.
- **Change:** bypass the batcher for small chunks arriving within N ms of a keystroke.

### A1.11 DEC 2026 frame holding *(conditional)*
- **When:** only if half-drawn TUI frames are observed. Detect `\x1b[?2026h/l`, hold mid-frame
  chunks until the end marker, with a 250ms safety timer so a missing marker can't freeze a pane.

---

## A2. Watcher & git-scan efficiency

### A2.1 One watcher per repo, not per worktree — **MOVED TO [A0.4](#a04-one-watcher-n-registered-roots)**
A naive "watch the project folder" version of this misses worktrees created outside the repo, so it
belongs with the subscription work rather than the scan-cost work. See A0.4.

### A2.2 Status-only scans for non-focused agents
- **Change:** the debounce thread already runs status + HEAD. For non-focused agents, skip the
  per-file line-count diffing and emit counts only; compute full deltas on reveal.
- **Effort:** S.

### A2.3 Index-aware status cache
- **Change:** cache `status()` keyed by `(index mtime, HEAD oid, worktree dirty-fingerprint)`. A
  scan whose key is unchanged returns the cached `Vec<FileChange>` without touching libgit2.
- **Impact:** repeated scans during a burst cost near-zero on large repos.
- **Effort:** S-M. **Files:** `git/service.rs`.

### A2.4 Ignore-aware scan cost
- **Change:** confirm the `ignore` crate walker is used for the tree scan with `.gitignore` +
  `node_modules`/`target` pruning, and that the walker is shared between the tree scan and the
  search index (B3) rather than run twice.
- **Effort:** S.

---

## A3. Native git engine

Every operation here removes a reason to spend tokens. Each lands as backend service method +
`Commands` entry + store + UI, with `#[cfg(test)]` temp-repo fixtures following the existing
`commit_content` helper pattern.

### A3.1 Remote sync
- `fetch(remote, refspec)` with progress callbacks → `git://fetch-progress` events
- `pull` = fetch + FF-or-merge, reusing the merge path
- `remotes_list` / `remote_add` / `remote_remove` / `remote_set_url`
- Credentials: `git2::Cred` chain — SSH agent → default key → credential helper → prompt.
  This is the fiddliest part; budget real time for it and test on all three OSes.
- **Effort:** M-L. **Files:** `git/service.rs`, `git/commands.rs`, new `git/credentials.rs`.

### A3.2 Branch operations
- `branch_create(name, from)` · `checkout(ref)` · `branch_rename` · `branch_delete(force)`
- `branch_set_upstream` · ahead/behind counts per branch (`graph_ahead_behind`)
- **Effort:** S-M.

### A3.3 Commit surgery
- `amend(message, paths)` · `revert(oid)` · `cherry_pick(oid)` · `reset(oid, mode)`
- `tag_create` / `tag_delete` / `tag_list`
- `stash_save` / `stash_list` / `stash_apply` / `stash_pop` / `stash_drop`
- **Effort:** M.

### A3.4 Native rebase
- **Current:** rebase is AI-delegated only — the agent shells out and the model reasons about it.
- **Change:** `git2::Rebase` driven natively: init → iterate operations → commit each → finish.
  On conflict, pause the rebase and surface the session state (A3.6) rather than aborting.
  Expose `rebase_continue` / `rebase_skip` / `rebase_abort`.
- **Impact:** the single most expensive AI-delegated operation becomes free and deterministic.
- **Effort:** L. **Files:** `git/service.rs`, new `git/rebase.rs`.

### A3.5 Merge without bailing
- **Current:** `merge()` returns `Err("merge conflicts — resolve manually")` and calls
  `cleanup_state()` — the conflict is thrown away.
- **Change:** on conflict, **keep** the merge state, write the conflicted index, and return a
  `MergeSession { conflicts: Vec<ConflictFile> }`. `cleanup_state()` moves behind an explicit
  `merge_abort`.
- **Effort:** M.

### A3.6 Conflict session model
- `ConflictFile { path, ours, theirs, base, resolved }` read from index stages 1/2/3
- `conflict_resolve(path, content)` writes the resolution and stages it
- `session_state()` reports whether a merge / rebase / cherry-pick is in progress and how far
- Powers the 3-way UI in B4.2 **and** the AI resolution path in A4.
- **Effort:** M.

### A3.7 Hunk-level operations
- `hunks(path)` from the existing diff engine
- `apply_hunks(path, indices, reverse)` via `git2` patch apply
- **Partial discard** = reverse-apply selected hunks to the working tree
- **Partial commit** = apply selected hunks to a transient index/tree and commit only those
  (no permanent staging-area concept — worktree-per-agent already subsumes changelists)
- **Effort:** M-L.

### A3.8 Commit graph
- `log_graph(limit, offset, filters)` returning `{oid, parents, refs, author, when, summary}` with
  lane assignment computed **in Rust**, not in the renderer.
- Filters: branch, author, path, date range, free-text over summaries.
- **Effort:** M.

---

## A4. Dual-path git actions (native + AI, always both)

**Principle:** native is the default and is always offered. The AI variant is *also* always offered
for every operation where a model could plausibly do better — not only on conflict — and it always
states its price before it runs. Nothing spends tokens without an explicit press.

### A4.1 The control
Split button, everywhere a git action appears:

```
[ Rebase onto main ]  [ ⌄ ]
                       ├─ Rebase with AI          ~8k tok · ≈$0.12
                       └─ Rebase with AI (verbose) ~14k tok · ≈$0.21
```

- Primary press = native. Instant, free, deterministic.
- Dropdown = AI, labelled with an estimate.
- The estimate is never hidden behind a hover — it is on the row.

### A4.2 Coverage matrix

| Operation | Native | AI variant | AI is worth it when |
|---|---|---|---|
| Commit | ✓ default | generate message from the staged diff | you want a real message, not `wip:` |
| Push | ✓ default | — (nothing to reason about) | never |
| Fetch / pull | ✓ default | — | never |
| Branch create/checkout | ✓ default | name a branch from the task | trivial, but cheap |
| Rebase | ✓ A3.4 | agent drives the rebase in its own PTY | tangled history, semantic conflicts |
| Merge | ✓ A3.5 | agent drives the merge | same |
| Conflict resolution | ✓ 3-way UI | per-file or per-hunk resolution | semantic conflicts across restructures |
| Cherry-pick | ✓ A3.3 | agent adapts the patch to a diverged base | patch doesn't apply cleanly |
| Revert | ✓ A3.3 | agent writes a semantic revert | the mechanical revert breaks things |
| Squash / reword history | ✓ A3.3 | agent composes the combined message | multi-commit cleanup |
| Discard | ✓ file + hunk | — | never |

### A4.3 Cost estimator service
- **New:** `src/app/cost/estimate.service.ts`
- Inputs: operation kind, file count, total diff bytes, conflict count, the agent's model + the
  provider rate table.
- Output: `{ tokensLow, tokensHigh, usdLow, usdHigh, confidence }`.
- Rate table ships in settings and is user-editable (rates change faster than releases).
- Estimates are calibrated against **actuals** from A6 — after N runs, the estimator uses the
  observed median for that op/model pair instead of the static heuristic.

### A4.4 Disclosure surfaces
- **Before:** estimate on the dropdown row.
- **During:** live token counter in the status bar, per running agent.
- **After:** actual cost written to the agent's ledger, and a toast when actual exceeds estimate
  by >50% (that's an estimator bug worth seeing).
- **Guard:** per-project budget cap and a "confirm above $X" threshold in settings. Hitting the cap
  disables AI variants (native stays fully usable) until the user raises it.

### A4.5 Migration of existing code
- `AgentActionsService.aiAction()` already drives commit/push/rebase/merge by typing into the PTY.
  **Keep it** — it becomes the dropdown branch verbatim.
- What is new: the native implementations (A3), the estimator (A4.3), and a
  `GitActionButtonComponent` that owns the split-button + disclosure contract so no call site
  reimplements it.

---

## A5. Token efficiency

### A5.1 Orrery MCP server
- **New:** `src-tauri/src/mcp/` exposing Orrery's own indexed knowledge to the agent as tool calls,
  served over the existing loopback bridge with the `ORRERY_TOKEN` gate.
- Tools: `git_status`, `git_log`, `git_blame`, `git_file_history`, `git_diff(from,to)`,
  `search(query, scope)`, `file_tree(path, depth)`, `read_file(path, range)`, `symbol_lookup`.
- **Impact:** an answer in ~200 tokens instead of the agent `cat`-ing five files to reconstruct it.
  This is the highest-leverage token item on the list, because it converts *discovery* — which is
  most of an agent's wasted context — from file reads into structured calls.
- **Effort:** L. Depends on B3 for `search` and A3.8 for `git_log`.

### A5.2 Repo map injection
- **Change:** on spawn, inject a compact repo map (directory skeleton + top-level exports per file,
  budgeted to ~2k tokens) into the agent's initial context via the existing skill/instruction
  injection path.
- **Effort:** M.

### A5.3 Turn deltas
- **Change:** the watcher already knows exactly what changed. Between turns, hand the agent a
  "changed since your last turn" delta instead of letting it re-read files to find out.
- **Effort:** M. Reuses the push-scan fingerprint.

### A5.4 Loop detector
- **Change:** from the hook event stream, detect the same tool failing ≥3× with a similar payload,
  or N turns with no diff change. Pause the agent, raise an inbox card, surface to the human.
- **Impact:** kills the most expensive failure mode — an agent burning a full context window
  retrying the same broken command.
- **Effort:** S-M. **Files:** `hooks/`, `runtime/`, inbox.

### A5.5 One-send review batching
- **Current:** `ReviewStore.assemble()` composes all comments into one message — verify at the
  call site that `SendReviewButton` does one send, not one per comment.
- **Change:** assert it in a test so it cannot regress.
- **Effort:** XS.

### A5.6 Prefer resume over relaunch
- **Change:** default every start of a previously-run agent to resume-by-session-id where the
  adapter supports it (claude/codex/cursor do; gemini falls back). Keeps the provider's prompt cache
  warm, which is a direct cost reduction on cached-input pricing.
- **Effort:** S. Mostly a default flip plus UI copy.

### A5.7 Model/effort routing
- **Change:** per-ticket-type defaults (already stored per tool in settings) — mechanical tasks
  downshift automatically; a "cheap mode" toggle on the spawn modal.
- **Effort:** S.

### A5.8 Context-fill awareness
- **Change:** read context usage from hook events where available; show a fill gauge per agent;
  offer manual compact-and-continue **before** the tool auto-compacts badly.
- **Effort:** M.

---

## A6. Token & cost telemetry

### A6.1 Ledger
- **New:** SQLite table `token_events { agent_id, ticket_id, project_id, ts, kind, model,
  input, cached_input, output, usd }` written from hook events + the ccusage sampler.
- **Effort:** M.

### A6.2 Surfaces
- Status bar: live tokens + $ for the focused agent
- Agent card: cumulative cost
- Ticket page: cost to date across all agents attached
- Project dashboard: cost per day, per model, per operation kind

### A6.3 Tokens per accepted line of diff
- **Definition:** `tokens_spent / lines_of_diff_merged_to_main` per ticket.
- **Why this one:** it is the only metric that captures *efficiency* rather than *volume*, and it is
  the number the landing page should lead with.
- Requires attributing merges back to the originating agent — the ticket link already exists.

### A6.4 Comparative benchmark
- **Deliverable:** a reproducible harness that runs the same N tickets against a fixed repo in
  Orrery and in a delegate-everything competitor, reporting tokens, wall-clock, and accepted lines.
- Publish it. The claim must be falsifiable or it isn't worth making.

---

## A7. Guardrails

### A7.1 Perf harness
- **Change:** launch the app in dev with a flag that spawns a synthetic flood agent; sample the
  existing counters + an echo-latency proxy (timestamp at Rust emit → xterm parse callback);
  write a JSON report.
- **Effort:** M.

### A7.2 CI perf budgets
- Fail CI on regression against [Acceptance budgets](#acceptance-budgets).

### A7.3 Lazy-boundary regression test
- **Change:** a vitest that reads source and asserts the boundaries — `import("@codemirror` stays
  dynamic, no static `@codemirror/*` import outside `code-lang.ts`, route-level `loadComponent`
  markers present.
- **Effort:** XS. Cheapest insurance on this list.

### A7.4 Memory budget test
- **Change:** spawn 20 synthetic agents, sleep 15, assert RSS below budget. Guards A1.4.
- **Effort:** M.

### A7.5 Why-comments convention
- Every magic constant (8ms, 16KB, 2MB, 512KB, 1MB ring, 200ms settle…) carries a `// Why:`.
  Each profiling session gets a dated note in `docs/perf/`.

### A7.6 Public benchmark page
- Landing-site page fed by the CI report: cold start, RSS at 1/5/20 agents, memory per worktree,
  tokens per accepted line. Auto-updated per release so it can't go stale.

### A7.7 Recursive process tree in the perf panel
> Build this **early**, alongside A7.1. Like the harness, it is the instrument that makes every
> other memory claim in this document checkable — and it settles arguments about whose memory it is.

- **Current:** the OS reports "Orrery: 1GB" and users — reasonably — read that as *our code*. In
  reality that figure spans the Rust backend, the WebView2 process family (browser, GPU, renderer,
  utility), one `conhost` per ConPTY, any shell wrapper (A0.1), each agent CLI, and **everything
  the agent itself spawned**. The single number blames our code for all of it. `pids()` +
  the sysinfo sampler already exist, but they produce flat per-agent numbers, not a tree.
- **Change:** an expandable process tree in the perf panel, rooted at Orrery's own pid and at each
  agent's PTY child, with per-node CPU, private working set, and RSS, plus rolled-up subtree totals.
  Annotate known node kinds — `webview2 · not our code`, `conhost · ConPTY host`,
  `pwsh · shim wrapper (see A0.1)` — so the panel is self-explaining.

**Two correctness details that decide whether the numbers mean anything:**

1. **Process discovery on Windows must not rely on parent-pid walking alone.** WebView2 children can
   be reparented, and an agent's grandchildren (a dev server it forgot to kill, a vitest runner, an
   E2E browser) may detach. Use the **Job Object** already in `runtime/jobobj.rs` — it exists as the
   agent kill backstop, and job accounting gives aggregate memory for everything in the job even when
   the pid tree lies. Assign the webview family to its own job at startup for the same reason.
2. **Do not sum RSS for a subtree total.** RSS double-counts shared pages, so a naive tree total
   overstates — sometimes badly, with many similar node processes. Report **private working set**
   (Windows) / **PSS** from `smaps_rollup` (Linux) / `phys_footprint` (macOS) as the headline
   number and keep RSS as a secondary column. sysinfo may not expose these uniformly; budget for a
   small platform-specific shim.

**Sampling cost.** Walking the full process table is exactly the `ProcessesToUpdate::All` problem
called out in A1.8. Refresh only known pids plus their discovered descendants, rediscover the
descendant set at a slower cadence than the metrics themselves, and gate the whole sampler on the
perf panel being open (the A1.7 pattern).

**Why this earns its place beyond debugging.** In an agent IDE the runaway process is usually not
the agent — it is the dev server, watcher, or test runner the agent started and never cleaned up.
Per-agent subtree totals make that visible, and a threshold alert ("agent `stripe-retry` subtree:
2.1GB — 1.8GB is a `next dev` it started 40 minutes ago") turns a mysterious slowdown into a
one-click kill. That is a real DX feature, not just instrumentation.

- **Feeds:** the status-bar split in A0.6 (`Orrery 210MB · agents 1.4GB`) — the tree is its
  drill-down.
- **Effort:** M. **Files:** `src-tauri/src/metrics/` (tree builder + platform memory shim),
  `runtime/jobobj.rs` (job assignment for the webview family), `src/app/dev-tools/dev-panel.*`.

---

# Section B — IDE DX

The test for every item: *a JetBrains user should reach for the shortcut without thinking, and it
should work.*

## B1. Editing

### B1.1 Writable editor
- **Current:** `file-view.component.ts` renders working-tree text read-only through
  `UnifiedCodeComponent`; CodeMirror is configured `EditorState.readOnly`.
- **Change:** editable mode with dirty state, `Cmd/Ctrl+S`, and a `file_write(path, content)`
  backend command that writes through the worktree. Guard the `MAX_CHARS` ceiling.
- **Effort:** S-M — the hard part (CM6 mounted, lazy, themed, lang-detected) already exists.

### B1.2 Autosave
- Debounced save on blur / after N seconds idle, off by default, per-project setting.

### B1.3 Editing affordances
- Multi-cursor, block selection, bracket matching, auto-indent, code folding — all CM6 extensions,
  add them to the shared extension set rather than per-component.

### B1.4 Preview
- Markdown preview already exists in `file-view`. Add image and PDF preview so repo docs don't
  require leaving the app.

### B1.5 Drag files into an agent prompt
- Drag from the file tree or the OS into the agent's prompt box → inserts the path (and for images,
  attaches). `FileDropService` already exists in the shell — extend it.

---

## B2. Navigation

### B2.1 Search Everywhere (`⇧⇧`)
- One overlay over files, symbols, agents, tickets, commands, git refs. Ranked, fuzzy, keyboard-only.
- **This is the single highest-DX-per-effort item in Section B.**

### B2.2 Command palette (`⌘⇧P` / `⌃⇧A`)
- Every action in the app registered in one command registry with an id, label, keybinding, and
  enablement predicate. Menus and context menus render *from* the registry so nothing can exist as a
  button without being reachable by keyboard.
- **Effort:** M, and it pays for itself — every later feature registers instead of wiring a button.

### B2.3 Recent files (`⌃E`) and Go to line (`⌘L`)

### B2.4 Go to definition / find usages
- **Approach:** LSP client in Rust, one server process per language per project (not per worktree),
  lazily started on first request and shut down when idle.
- **Caution:** this is the one item in this roadmap that genuinely threatens the memory pitch — a
  `rust-analyzer` or `tsserver` instance dwarfs Orrery itself. Ship it **opt-in per project**, show
  the server's RSS in the status bar, and make shutdown aggressive.
- Consider tree-sitter-only symbol indexing as the default (fast, tiny, no cross-file resolution)
  with full LSP as the opt-in upgrade.
- **Effort:** L.

### B2.5 Structure view + breadcrumbs
- Tree-sitter symbol outline for the current file. Cheap once B2.4's tree-sitter layer exists.

### B2.6 Navigation stack
- Back / forward across files, diffs, and commits — including across panes.

---

## B3. Search

### B3.1 Find in files
- **Change:** ripgrep invoked from Rust (or `grep-searcher` as a library to avoid a bundled binary),
  streaming results to the frontend as they arrive. Scope selector: this worktree / this project /
  all worktrees.
- **Impact:** serves both the human and — via A5.1 — the agent.
- **Effort:** M.

### B3.2 Find and replace across files
- Preview-then-apply, with per-match toggles. Reuses the hunk-apply machinery from A3.7.

### B3.3 Search in the diff / in blame
- Scoped search inside the current review surface — the thing you actually want during a review.

---

## B4. Git UX

### B4.1 Commit graph view
- Renders A3.8. Lanes, refs, filters, search. Virtualized (this list genuinely exceeds 100 rows).

### B4.2 3-way conflict view
- Renders A3.6. Ours / base / theirs columns with accept-side and accept-hunk actions, plus the
  **AI resolve** dropdown per A4 (per-file and per-hunk, each with its estimate).
- **This is the flagship DX moment** — it's where "IntelliJ-grade git" is either true or isn't.

### B4.3 Editor gutter change markers
- Changed-line markers in the editor gutter with click-to-revert-hunk, exactly like IntelliJ's.
  Data already exists from the push-scan; the interaction is A3.7's reverse-apply.

### B4.4 Local History
- Periodic snapshots of the worktree (content-addressed, in the app data dir, bounded by size and
  age) so an agent's destructive edit is recoverable even without a commit.
- The watcher already knows what changed — this is mostly a bounded store plus a timeline UI.
- **Effort:** M. High trust value in an agentic tool specifically.

### B4.5 Already shipped, keep visible in the UI
- Blame gutter with age fade · file history · range diff · per-commit diff · inline review comments.
  These are differentiators competitors don't advertise — make sure the UI surfaces them prominently
  rather than burying them behind a toggle.

---

## B5. Terminal

### B5.1 Splits
- Arbitrary horizontal/vertical splitting within an agent's terminal pane.

### B5.2 Scrollback persistence across restart
- Falls out of A1.2's ring buffer once it is persisted to disk on shutdown.

### B5.3 Scrollback search
- `@xterm/addon-search`, with match highlighting and next/prev.

---

## B6. Workspace

### B6.1 Split anything
- The pane manager already tiles agents. Extend the node kinds so diffs, files, terminals, and the
  commit graph are all splittable into the same tree.

### B6.2 Keymap presets
- IntelliJ and VS Code presets over the B2.2 command registry, plus per-command rebinding.
- The IntelliJ preset is a positioning statement, not just a convenience.

### B6.3 Per-project settings
- Model/effort defaults, auto-approve policy, budget cap, autosave, LSP opt-in — all overridable per
  project, stored alongside the project record.

### B6.4 Desktop notifications
- On blocked / needs-permission / finished. Native notifications, respecting focus state.

---

## B7. Integrations

### B7.1 GitHub
- PR list, PR diff review with inline comments, approve/request-changes, CI check status, open a
  worktree from a PR. Via the GitHub REST/GraphQL API with the user's token.
- Reuses the entire review surface already built for agent diffs.
- **Effort:** M-L.

### B7.2 Issue tracker → worktree
- Generic adapter (GitHub Issues first, Linear/Jira behind the same interface) that imports an issue
  as an Orrery ticket, so the existing backlog → dispatch → agent loop absorbs it unchanged.

### B7.3 Orrery CLI expansion
- **Current:** `orrery hook` brokers hook events with the `ORRERY_*` env gate.
- **Change:** add `orrery worktree create|list|remove`, `orrery checkpoint`, `orrery note`,
  `orrery status` so agents can drive the IDE — and so scripts can too.
- Keep the env-presence gate as the authorization model.

---

## Sequencing

```
Phase 0  (blocking)     0.1 macOS → 0.2 Linux → 0.3 cross-platform E2E
                         │
         ┌───────────────┴───────────────┐
         │                               │
Track A (perf/efficiency)        Track B (IDE DX)
         │                               │
A7.1 harness + A7.7 process tree    B2.2 command registry   ← foundations: instrument
  + A7.3 lazy test + A0.7p1 emit      │                        first, then build
    telemetry                         │
         │                            │
A0.5 HEADLESS DECISION ◄── gate: answer before investing further in A1
         │
A0.1 shell wrapper  ·  A0.6 store eviction   ← independent, do them now
         │
A1.2 ring ──────┬───────────────► B5.2 scrollback persist
         │      │
A0.2 subscription (needs A1.2)
         │
A0.7p2 scheduler (needs A0.2 + ≥1wk of A0.7p1 data)
         │
A0.3 Rust-side heuristics  ·  A0.4 one watcher, N roots
         │
A1.1 ACK  ·  A1.3 WebGL  ·  A1.6 folding  ·  A1.7-A1.9 micro
         │                               │
A1.4 sleep (needs A1.2)          B1.1 editable editor
         │                               │
         │                        B3.1 find in files ──┐
         │                               │             │
         │                        B2.1 search everywhere│
         │                               │             │
A3.1 fetch/pull ─────────────────► B4.1 commit graph   │
A3.2 branch ops                          │             │
A3.5 merge session ──────────────► B4.2 conflict view  │
A3.6 conflict model              (flagship)            │
A3.4 native rebase                       │             │
A3.7 hunks ──────────────────────► B4.3 gutter revert  │
A3.8 log graph                           │             │
         │                        B4.4 local history   │
A4 dual-path + estimator                 │             │
         │                        B1.3-B1.5, B5, B6    │
A5.1 MCP server ◄────────────────────────────────────┘
A5.2-A5.8 token work             B7 integrations
         │                               │
A6 telemetry ──────────────────► A7.6 public benchmark page
A7.2 CI budgets, A7.4 memory test
```

**Rules of thumb:**
- `A7.1` + `A7.3` + `B2.2` before anything else in their tracks. Harness first, registry first.
- **`A0.5` is a gate, not a task.** Answer "headless or PTY by default?" before building A1.3,
  A1.4, A1.6, A1.10 or A1.11 — a yes deletes most of them for headless agents. Timebox the
  investigation to one adapter (claude) and decide.
- `A1.2` unlocks `A0.2` (subscription), `A1.4` (memory) and `B5.2` (DX) — highest fan-out item in
  Track A, and it is the safety precondition for `none`-mode.
- `A0.1` and `A0.6` are independent of everything and cheap. Ship them while A0.5 is being decided.
- **`A0.7` Phase 1 now, Phase 2 later.** The telemetry funnel + clippy ban ship immediately and are
  cheap; the scheduler waits for its own data. Because Phase 1 installs the call-site plumbing,
  Phase 2 is a one-module change rather than a second refactor — nothing is wasted by waiting.
- **`A7.7` belongs with `A7.1`, not at the end.** Every memory number in this document is an
  assertion until the process tree exists to check it — including the A0.1 pwsh figures, which are
  currently estimates.
- `A3.5` + `A3.6` unlock `B4.2`, which is the flagship. Prioritize that chain over `A3.4`.
- `A5.1` (MCP) depends on `B3.1` and `A3.8` — the token win is downstream of the DX work, not
  parallel to it. Don't start it early.

---

## Acceptance budgets

Enforced by A7.2 in CI. Numbers are starting proposals — calibrate against the first harness run,
then freeze.

| Metric | Budget |
|---|---|
| Cold start to interactive | ≤ 300ms (currently sub-100ms — hold it) |
| Focused-terminal echo, p50 | ≤ 50ms |
| Focused-terminal echo, p95 | ≤ 75ms |
| Focused-terminal echo, worst | ≤ 300ms |
| `agent_input` round-trip p95 | ≤ 25ms |
| PTY-origin UI-thread jobs | ≤ 75/s total, **flat from 1 → 10 agents** |
| JS long tasks attributable to PTY | none > 100ms |
| Hidden terminal queue peak | ≤ 2MB |
| Dropped backlogs under normal load | 0 |
| In-flight bytes per agent / global | ≤ 512KB / ≤ 8MB |
| RSS, idle, 1 project | ≤ 250MB |
| RSS, 5 active agents | ≤ 600MB |
| RSS, 20 agents (15 slept) | ≤ 900MB |
| Watchers | ≤ 1 per open project |
| Wrapper processes per agent (Windows) | 0 (conhost only, and 0 in headless mode) |
| Renderer bytes/sec for `none`-mode agents | 0 |
| Renderer work vs total agent count | flat — proportional to visible surfaces only |
| Blame RSS, 50k-line file | ≤ 5MB |
| Raw `app.emit` call sites outside the funnel | 0 (clippy-enforced, from A0.7 Phase 1) |
| Aggregate telemetry overhead per emit | ≤ 1µs, no allocation on the hot path |
| Telemetry disk footprint | ≤ 50MB / 7 days, daily rotation |
| Payload contents in telemetry files | **0 bytes, ever** |
| Bypass-class emit latency (permission, exit, error) | ≤ 1 frame, never queued *(Phase 2)* |
| Backend queued bytes, all bulk classes | ≤ 16MB, hard cap *(Phase 2)* |
| Bulk-class starvation | none — every class drains within 10 windows *(Phase 2)* |
| `git status` on a 50k-file repo, cached | ≤ 10ms |
| Native git op tokens | **0** |

---

## Non-goals

Explicitly out of scope. Each is listed with why, so it doesn't get re-litigated quarterly.

- **Fan-out (one prompt → N agents → compare → cherry-pick).** Solving one task with three agents
  spends 3× the tokens to discard two results. It is the direct opposite of Orrery's efficiency
  pitch. Not a capability gap — a deliberate difference.
- **Embedded Chromium per worktree / design mode.** The heaviest thing a competitor in this space
  does, and irreconcilable with the memory budget above. Ship system-browser handoff plus an
  element-picker script instead.
- **Mobile companion.** E2EE pairing, a relay, and two app stores is a product, not a feature. It
  does not serve the wedge.
- **SSH remote worktrees.** Real value, high cost. Revisit after Section A and B land.
- **Compilation / full IntelliSense.** Orrery highlights and navigates; it does not build. B2.4 is
  deliberately capped at symbol navigation, and even that is opt-in.
- **A permanent staging-area UI.** Worktree-per-agent already subsumes changelists; A3.7's partial
  commit uses a transient tree, not a persisted index.

---

## Open decisions

1. **macOS code signing** — Developer ID + notarization is a hard prerequisite for 0.1. Who owns
   the certificate, and does it gate the Linux build too?
2. **Open source or not.** Competing on feature velocity against a daily-shipping MIT project from
   a private Windows-only beta is the hardest version of this fight. Decide before Section B, since
   it changes what "differentiator" means.
3. **B2.4 default** — tree-sitter-only (tiny, no cross-file resolution) vs LSP opt-in vs both.
   Recommendation: tree-sitter default, LSP opt-in, RSS shown in the status bar either way.
4. **A5.1 MCP transport** — reuse the existing loopback bridge + `ORRERY_TOKEN`, or stand up a
   separate stdio MCP server per agent? Loopback reuses the auth model already proven by hooks.
5. **Estimator calibration window** — how many observed runs before switching from heuristic to
   observed median in A4.3?
6. **Headless vs PTY default (A0.5)** — the highest-leverage open question in this document, and
   the one that most changes what Section A even contains. Requires a per-adapter audit of
   structured-output support, plus confirmation that hooks, resume, and permission cards survive
   without a TTY. **Decide before A1.3/A1.4/A1.6.**
7. **`digest` mode payload shape** — last N rendered lines (needs a minimal VT model in Rust) vs
   last N raw bytes (cheap, but ANSI-noisy in the preview). Affects whether A0.3 needs a real
   terminal model or just a line splitter.
8. **A0.7 class taxonomy — deferred by design, not undecided.** The three-class split
   (bypass / coalesced state / bulk weighted) is a refinement of the original numbered-priority
   idea, on the grounds that byte quotas are wrong for small urgent messages and that state wants
   last-write-wins rather than a queue. **Do not settle it from first principles.** Ship A0.7
   Phase 1, run it for a week across a realistic session, then let the inventory table assign the
   classes. The quanta are easy to retune afterwards; the class boundaries are not, which is
   exactly why they should be chosen from data.
9. **Raw-trace default window** — how long does an opt-in trace run before auto-disabling, and does
   hitting the size cap stop it or rotate it? Stopping is safer (a trace left on during a week of
   floods is its own perf problem); rotating captures more. Recommendation: auto-disable after
   30 minutes or 200MB, whichever comes first, with a visible indicator while it is on.
