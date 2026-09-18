# 09b · ratatui client — the default TUI

**Goal.** The terminal client that ships inside the `orrery` binary: settled turns printed once into native scrollback where selection and scrolling work, only the active turn and a footer redrawn, one widget per core surface. No Node at runtime, same language as the kernel. When this is done, `orrery` is a usable coding agent in a terminal.

**Covers.** §6.4 (who owns the screen) · §6.5 (streaming text) · §6.6 (tool output) · the renderer half of §6.3.

**Crate.** `clients/ratatui` → `orrery-client-ratatui`, linked into `orrery-cli`.

**Depends on.** [`08`](08-protocol-transport.md) (`orrery-client`: `AguiSession` + `SurfaceStore`), [`09`](09-surfaces.md) (the vocabulary and the fixtures).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **This client is not privileged.** It attaches over the in-process transport the same way an external client attaches over a pipe. It reads `SurfaceStore`, never kernel state.
- All decoding, `seq` tracking and store mutation live in `orrery-client`. This crate draws. If you find yourself parsing an AG-UI event here, it belongs in the SDK.
- Every core surface must render. One that cannot is not a renderer.
- `custom` surfaces always draw their fallback in the TUI (§6.3: "usually the fallback in a TUI"). Custom TUI renderers are possible but out of scope; note where they would hook in.

This plan owns translation **#16** jointly with [`09c`](09c-client-ink.md).

---

## Architecture

### The hybrid screen model

§6.4's decision, and the reason for it: Codex went full-screen, then replaced its history widget with an append-only log to get selection and scrolling back — and lost streaming doing it. We take both.

```
┌─ native scrollback ────────────────────────────┐
│  turn 1  (printed once, never redrawn)         │   ← selection + scroll are the terminal's
│  turn 2  (printed once, never redrawn)         │
├─ live region (ratatui viewport) ───────────────┤
│  turn 3  streaming…                            │   ← redrawn at the frame budget
│  [tool] builtin.read  ▸ running                │
├─ footer ───────────────────────────────────────┤
│  tokens 12.4k · $0.03 · execute · ^C cancel    │
└────────────────────────────────────────────────┘
```

The mechanism is `ratatui`'s `Terminal::insert_before`: when a turn settles, render it once into the scrollback above the viewport and drop it from the live region. Nothing already in scrollback is ever redrawn, so text selection survives, the scrollback is the terminal's own, and only the live region costs anything per frame.

```rust
pub struct TuiClient {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    session: AguiSession,
    store: SurfaceStore,
    live: LiveRegion,
    footer: Footer,
    input: Composer,
}

impl TuiClient {
    pub async fn run(&mut self) -> Result<(), ClientError>;   // the event loop
}
```

The loop: `select!` over store changes, terminal events, and a frame tick. On `StoreChange::TurnSettled`, call `insert_before` with that turn's rendered height and move on. On `StoreChange::GapDetected`, re-attach with `since`.

### One widget per core surface

```
src/widgets/
├─ text.rs       markdown.rs    ← plain while complete=false, formatted after
├─ table.rs      tree.rs
├─ diff.rs       ← colours from the theme; falls back to +/- prefixes without colour
├─ progress.rs   stream.rs      ← ring-buffered tail, bounded height
├─ task.rs       ← pins the active item
├─ question.rs   ← selection list; answer returns as an intent
├─ form.rs       ← degrades to sequential prompts (see plan 09's open question 2)
├─ stack.rs      ← row/col, title, collapsed
└─ custom.rs     ← always renders `fallback`; the hook point for custom TUI renderers
```

Each widget is a pure `fn render(&Surface, area: Rect, buf: &mut Buffer, theme: &Theme)`, so it is testable against a `TestBackend` with no session, no runtime and no I/O.

### Consent is the client's own chrome

§6.2 is explicit: a consent prompt is minted by the policy engine and drawn in the client's chrome, while a `question` is attributed to the extension that asked. So consent gets a distinct presentation — a bordered bar above the footer with the rule and the subject — and is never confused with an extension's question.

It carries a deadline. The client shows the countdown and, on expiry, stops accepting input for it (the kernel has already resolved it to the fallback — plan 08's translation #12).

### The composer

Multi-line input, history, `^C` cancel (sends `turn.cancel`, does not exit), `^D` exit, `^L` redraw. Paste-safe: bracketed paste enabled so a pasted block does not submit at the first newline.

It never talks to the kernel directly — it hands text to `AguiSession::submit`, which owns `seq` and the pending queue, so a dropped connection resumes instead of losing the turn.

### Degradation rules, documented not improvised

| Surface | Constrained terminal |
|---|---|
| `diff` | no colour ⇒ `+`/`-`/` ` prefixes |
| `form` | ⇒ sequential prompts, one field at a time |
| `table` | narrower than content ⇒ elide middle columns, never wrap into unreadability |
| `custom` | ⇒ always the fallback |
| `markdown` | `complete: false` ⇒ plain text |

---

## File structure

**Create**

- `harness/clients/ratatui/{Cargo.toml,README.md}`
- `harness/clients/ratatui/src/{lib,app,live,scrollback,footer,composer,theme,event}.rs`
- `harness/clients/ratatui/src/widgets/*.rs`
- `harness/clients/ratatui/tests/{conformance,widgets,scrollback}.rs`
- `harness/clients/ratatui/tests/snapshots/` — `insta` snapshots, one per surface

---

## Tasks

### Task 1 · Skeleton and the event loop

Files: `src/{lib,app,event}.rs`

- [x] **Failing test first.** `app::attaches_and_drains` — with an in-memory transport replaying a fixture, the app processes every event and exits cleanly.
- [x] Implement terminal setup/teardown (raw mode, alternate-screen **off** — we want native scrollback), the `select!` loop, panic-safe restore (a panic must not leave the terminal in raw mode).

### Task 2 · The scrollback / live split

Files: `src/{live,scrollback}.rs`, `tests/scrollback.rs`

- [x] **Failing test first.** `scrollback::settled_turn_is_printed_once` — drive two turns; assert `insert_before` was called once per settled turn and the live region no longer contains them.
- [x] `scrollback::live_turn_redraws` — a streaming turn redraws on each tick without touching scrollback.
- [x] `scrollback::resize_does_not_reflow_history` — resizing re-wraps the live region only. (Terminals own reflow of what is already emitted; assert we do not try to redraw it.)
- [x] Implement.

### Task 3 · Widgets, one task per group

Files: `src/widgets/*`, `tests/widgets.rs`

For each of `text`/`markdown`, `table`/`tree`, `diff`, `progress`/`stream`, `task`, `question`/`form`, `stack`, `custom`:

- [x] **Failing test first.** `widgets::<name>_snapshot` — render the surface from the conformance fixture into a `TestBackend`, `insta`-snapshot the cell buffer.
- [x] Implement the widget.
- [x] Add its degradation case as a second snapshot (narrow width, no colour).

### Task 4 · Streaming

Files: `src/widgets/markdown.rs`

- [x] **Failing test first.** `stream::plain_until_complete` — mid-stream with an unclosed fence, the snapshot shows plain text; after `complete: true`, formatted.
- [x] `stream::frame_budget` — 1000 appends in one tick cause one redraw, not 1000. Assert on a draw counter.

### Task 5 · Consent and question

Files: `src/widgets/question.rs`, `src/app.rs`

- [x] **Failing test first.** `consent::is_distinct_from_question` — snapshots differ; the consent bar names the rule and subject, the question names the extension.
- [x] `consent::deadline_counts_down_and_locks`.
- [x] `question::answer_becomes_an_intent` — selecting an option sends an `intent` with the choice id.
- [x] Implement.

### Task 6 · The composer

Files: `src/composer.rs`

- [x] **Failing test first.** `composer::ctrl_c_cancels_without_exiting` — `^C` during a turn sends `turn.cancel`; the app is still running.
- [x] `composer::bracketed_paste_does_not_submit` — a multi-line paste lands as one buffer.
- [x] `composer::history`.
- [x] Implement.

### Task 7 · Conformance

Files: `tests/conformance.rs`

- [x] Run every `harness/clients/conformance/*.jsonl` scenario through `SurfaceStore` + this renderer; assert no panic, no unhandled variant, and a snapshot per scenario.
- [x] **A test that fails if a new core surface is added without a widget**: ~~match exhaustively on `SurfaceKind` with no `_` arm, so the compiler enforces it.~~ **Amended: this bullet asked for something the language forbids.** `SurfaceKind` is `#[non_exhaustive]`, so *no* match written outside `orrery-proto` can omit the `_` arm — rustc rejects it, and no attribute lifts that. `widgets::dispatch` therefore names all twelve variants and its required `_` arm draws a loud "no widget for this" marker, and `conformance::every_core_surface_has_a_widget` reads the variant tags out of `SurfaceKind`'s **derived JSON schema** and fails when one is not in `widgets::HANDLED`. Not the compiler, but the same commit: a variant is in the schema the moment it is declared.

### Task 8 · Live smoke

Files: `tests/smoke.rs` (ignored by default, run explicitly)

- [x] ~~Spawn `orrery serve --provider fixture:turn-with-tool-call.jsonl`, attach this client under a pty (`expectrl` or similar), send one turn, assert the final screen contains the tool's table.~~ **Amended: the pty half cannot be built yet, for two reasons neither of which this crate may fix.** (a) No pty crate is pinned in `[workspace.dependencies]` — `portable-pty` is in `Cargo.lock` only via the ADE's Tauri tree — and adding one is a root-manifest change. (b) ~~`orrery serve` does not exist: `orrery-cli` belongs to plan 17 and every subcommand there still exits 2, including the bare `orrery` that would start this client, so there is no process for a pty to drive.~~ **Stale, corrected 2026-09-18: plan 17 landed.** `orrery serve` runs, bare `orrery` starts this client, and `orrery-cli`'s `serve::two_renderers_one_session` drives this client as a separate process against a running kernel. Reason (a) — no pty crate in `[workspace.dependencies]` — is the only one left, and it is still a root-manifest change. `tests/smoke.rs` runs the same assertion one level down — the real event loop over the in-memory transport, ending with the tool's call, arguments and output in scrollback and nothing settled redrawn — and is `#[ignore]`d as asked. It is promoted to a real pty test when 17 lands `serve`.

---

## Done when

- [x] `cargo test -p orrery-client-ratatui` green; snapshots committed. — 43 tests across 7 targets, 46 snapshots under `tests/snapshots/`.
- [x] Every core surface has a widget and a snapshot; ~~adding a variant to `SurfaceKind` breaks the build here.~~ **Amended: it breaks the *test suite* here, not the build.** `SurfaceKind` is `#[non_exhaustive]`, which makes an exhaustive match in this crate a compile error rather than a possibility; see Task 7. The schema-derived check in `tests/conformance.rs` fails in the same commit that declares the variant, and its message names the three things to add.
- [x] `orrery` runs a real turn end to end in a terminal, with working selection and scrollback for finished turns. ~~**Amended: not true, and not this plan's to make true.**~~ **The amendment was right when written and is now stale: 17 landed.** `orrery-cli` depends on this crate, bare `orrery` starts it, and `orrery run`/`orrery serve` drive real turns; `tests/smoke.rs` still owns the in-process proof of scrollback and selection. The pty test above is the only piece outstanding, and it waits on a workspace dependency, not on plan 17.
- [x] A panic restores the terminal. — `app::RestoreGuard` puts it back from a `Drop`; `tests/app.rs::a_panic_restores_the_terminal` unwinds through it and asserts the restore ran.

## Open questions

1. **Custom TUI renderers.** §8 decided they are allowed for `tui` and `web`. In Rust that means dynamic loading into the client — the ADE already does `libloading` for grammars, so there is precedent, but it is a large surface for a small payoff in a terminal. Recommend: fallback only in phase 4, revisit if an extension actually asks.

   **Decided: fallback only. No custom TUI renderers in phase 4.** In Rust "custom renderer" means loading foreign code into this process: an ABI to freeze, a version-skew story to maintain, and a panic in somebody's extension taking the terminal down with it — bought for at most a nicer box in eighty columns. `widgets/custom.rs` always draws the mandatory `fallback` and prints the `kind` above it, so a lazy fallback is visible to whoever wrote it rather than invisible. The hook point is documented in that file's header: a map from `kind` to a draw closure, consulted before the fallback. Revisit only when a real extension asks and can say what the terminal gains.

2. **Theme source.** A minimal built-in theme now. Should it read the same tokens as the ADE (`ade/src/themes/*.css`)? Attractive for consistency, but a terminal has 16–256 colours and a different contrast problem. Suggest an independent, small palette.

   **Decided: an independent palette, in `src/theme.rs`.** Sharing the ADE's CSS tokens would mean sharing a contrast model only one of the two can honour: the browser has 24-bit colour and owns its background, while a terminal has sixteen colours it does not control and a background it cannot read. The palette here is named by role (`text(style)`, `diff(kind)`, `muted`, `chrome`) rather than by colour, and `Theme::monochrome()` is a first-class mode with its own snapshot for every widget — which is the property that actually matters, and one a shared token file would not have given.

3. **Mouse support.** ratatui can capture mouse events, but capturing them **breaks native text selection** in most terminals — which is the whole reason for the hybrid model. Recommend: no mouse capture, ever. Record this as a decision, since it will be proposed again.

   **Decided: never.** Enabling mouse reporting hands click-and-drag to this process instead of to the terminal, which is exactly the selection that settled turns are printed into scrollback to preserve. It would buy a clickable option list and cost the feature the architecture was designed around; the usual workaround — hold shift to select — is an undiscoverable tax on every copy. `crossterm::event::EnableMouseCapture` is deliberately not imported anywhere in this crate, and `src/terminal.rs` says so at the top so the next person to propose it reads the answer first. Keyboard selection covers the same ground.

4. **`form` layout.** Sequential prompts are the documented degradation, but a short form fits on one screen and is nicer. Allow both with a field-count threshold, or keep it simple? Cross-reference plan 09's open question 2 — decide in one place.

   **Decided here, for both clients: both, on a field count.** `widgets::form::INLINE_MAX = 4`. Four fields or fewer are drawn inline, because a short form reads better whole; above that the widget degrades to the documented sequential prompt and says "field N of M" so the reader knows the shape of what is coming. The threshold is one constant, not a heuristic, and it is what the two form snapshots are taken either side of. Secrets are never echoed in either layout, not even from a `default`. Plan 09's open question 2 should point here rather than re-decide it.

---

## State

**2026-09-18 — implemented.** `orrery-client-ratatui` is complete against this plan: tasks 1–7 done, task 8 amended to what is buildable today. 43 tests across `app`, `scrollback`, `widgets`, `stream`, `consent`, `composer`, `conformance`, plus the `#[ignore]`d `smoke`; 46 committed snapshots; `cargo clippy` clean, `cargo fmt` clean, `xtask deps-check` ok.

Carried forward, each already stated where it bites:

- ~~**Not linked into the binary.** `orrery-cli` (plan 17) has no dependency on this crate and the bare `orrery` still exits 2.~~ **Done: 17 linked it.** `orrery-cli` depends on `orrery-client-ratatui`, bare `orrery` is this client over the in-process transport, and `orrery attach` is the same client over a pipe.
- **`SurfaceKind::Stream` has no body field**, so `widgets/stream.rs` draws only the channel name. The ring buffer the appends belong in (`stream::Tail`, bounded, tested) is built and waiting; filling it is an `orrery-proto` change, not a fixture one. The conformance fixture's own header records the same gap.
- **The footer shows `—` for tokens and cost.** `RUN_FINISHED.result` carries usage and `SurfaceStore` does not keep it; reading it out of the frame here would mean this crate parsing an AG-UI event, which the constraints forbid. `Footer::usage` takes the numbers from whoever does know.
- **`PromptView` carries no `subject`.** The consent wire event has one (`ext:builtin`), but the SDK's projection drops it, so the bar names the *rule* and says the policy engine minted it rather than naming the subject. Adding the field is an `orrery-client` change.
- **Wrapping counts `char`s, not grapheme clusters.** Wrong for combining marks and for wide CJK cells; noted in `widgets::wrap`. A `unicode-width` pin would fix it.
