# 09c · Ink client — React in the terminal, and the third-party proof

**Goal.** A React/Ink terminal client that talks to the kernel over **stock AG-UI** and knows nothing else — no Rust, no private path, nothing from the repo but the published `@orrery/protocol` types and the `@orrery/client` SDK. It exists for two reasons: React components are the right tool for rich terminal UI and for custom renderers, and building it against the published spec is the only honest test that §5.6's promise holds.

**Covers.** §6.4 (the React half of the hybrid) · §5.6 (AG-UI adoption, verified) · §6.3 (renderer contract, second implementation).

**Package.** `clients/ink` → `@orrery/client-ink`.

**Depends on.** [`08`](08-protocol-transport.md) (`@orrery/client`, the HTTP/SSE listener, the conformance fixtures), [`09`](09-surfaces.md).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **This client may not import anything from the Rust tree.** Its only dependencies on the harness are `@orrery/protocol` (generated types) and `@orrery/client` (the SDK). If it needs something else, the *protocol* is missing something — fix that, not this.
- Same conformance fixtures as the Rust SDK. Same assertions.
- Every core surface renders. `custom` may load a renderer under a `render` grant; without one, the fallback.
- AG-UI's shared state is bidirectional; ours is not. `StateSnapshot`/`StateDelta` flow outward only; a component edit becomes an `intent`. Anyone porting an AG-UI component that writes state directly needs to know.

This plan owns translation **#16** jointly with [`09b`](09b-client-ratatui.md).

---

## Architecture

### How it is launched

| Invocation | What happens |
|---|---|
| `orrery --ui ink` | The kernel starts, listens on a pipe and an SSE endpoint, and spawns `node clients/ink` with `ORRERY_ENDPOINT` in the environment. Kernel outlives the Node process. |
| `pnpm -C harness/clients/ink start --endpoint http://127.0.0.1:PORT/…` | Standalone, against a running `orrery serve`. This is the mode that proves a third party can do it. |

Both attach to a session; neither is privileged.

### The component tree

```tsx
<App>                          // owns AguiSession + SurfaceStore, subscribes once
  <Static items={settledTurns}>   // Ink's <Static>: printed once, never re-rendered
    {turn => <Turn turn={turn} />}
  </Static>
  <LiveTurn turn={activeTurn} /> // the only thing that re-renders
  <ConsentBar prompt={pending} />
  <Footer usage={usage} mode={mode} />
  <Composer onSubmit={session.submit} />
</App>
```

`<Static>` is Ink's equivalent of ratatui's `insert_before`, and the reason the same §6.4 hybrid works in both: finished turns go to native scrollback and are never redrawn, so selection and scrolling stay the terminal's.

### One component per core surface

```
src/surfaces/
  Text.tsx  Markdown.tsx  Table.tsx  Tree.tsx  Diff.tsx
  Progress.tsx  Stream.tsx  Task.tsx  Question.tsx  Form.tsx
  Stack.tsx  Custom.tsx
  index.ts   // the exhaustive switch — see task 6
```

Each is a pure function of a `Surface`. No component calls the session; interaction bubbles up to `<App>`, which is the only thing holding it — the same discipline as ratatui's widgets.

### Custom renderers

The one thing this client can do that the ratatui client will not: load an extension's own React component for a `custom` kind.

```ts
registerRenderer("buildgraph.flamegraph", FlamegraphComponent);
```

Under a `render` grant (§4.8), sandboxed, and recorded in the ledger. Without a matching renderer, `<Custom>` draws `surface.fallback`. One deny rule degrades every custom surface everywhere.

Phase 4 scope: the registration API and the fallback path. Actually loading third-party bundles is phase 8 territory (it needs the signed registry) — leave the hook and a `TODO(plan-15)`.

---

## File structure

**Create**

- `harness/clients/ink/{package.json,tsconfig.json,vitest.config.ts,README.md}`
- `harness/clients/ink/src/{index.tsx,app.tsx,turn.tsx,focus.ts,composer.tsx,footer.tsx,consent.tsx,theme.ts,registry.ts,devtools-stub.ts}`
- `harness/clients/ink/src/surfaces/*.tsx` (twelve components, `kinds.ts`, `index.tsx`)
- `harness/clients/ink/test/*.test.tsx` — **amended:** the two files the plan named grew into nine, one per task rather than two catch-alls (`app`, `entry`, `surfaces`, `markdown`, `interaction`, `exhaustive`, `conformance`, `custom`, `smoke`), plus `fake.ts`, the fixture-backed fake transport they share. `turn.tsx` and `focus.ts` are likewise splits the plan did not name: `<Turn>` had to be its own module for the `<Static>` render-count test to be able to see it, and focus arbitration had to be one function because two components listening to one keypress is how a TUI starts eating input.

---

## Tasks

### Task 1 · Package and connection

Files: `package.json`, `src/index.tsx`, `src/app.tsx`

- [x] **Failing test first.** `app.connects_and_drains` — with a fixture-backed fake transport, the app processes every event and unmounts cleanly.
- [x] Set up the package: `react`, `ink`, `@orrery/client`, `@orrery/protocol`, vitest, `ink-testing-library`. **No other harness dependency.**
- [x] `--endpoint` flag and `ORRERY_ENDPOINT`; friendly error when neither is set.
- [x] Wire `AguiSession` + `SurfaceStore` from `@orrery/client`; the app holds them, components receive plain data.

### Task 2 · The Static / live split

Files: `src/app.tsx`

- [x] **Failing test first.** `app.settled_turns_go_static` — two turns; assert the settled ones are inside `<Static>` and that re-rendering does not re-emit them (assert on `lastFrame()` stability and render counts).
- [x] `app.gap_triggers_reattach` — a `seq` gap causes a re-attach with `since`.
- [x] Implement.

### Task 3 · Surface components

Files: `src/surfaces/*`, `test/surfaces.test.tsx`

For each surface, in the same groups as plan 09b:

- [x] **Failing test first.** `surfaces.<name>` — render the conformance fixture, snapshot `lastFrame()`.
- [x] Implement the component.
- [x] Add the degradation snapshot (narrow width).

### Task 4 · Streaming markdown

Files: `src/surfaces/Markdown.tsx`

- [x] **Failing test first.** `markdown.plain_until_complete` — an unclosed fence renders plain; `complete: true` renders formatted.
- [x] Implement, with a small markdown renderer (`marked` + a terminal formatter, or hand-rolled — **decide and record**; a heavy dependency here is not worth much).

### Task 5 · Question, consent, composer

Files: `src/{consent,composer}.tsx`, `src/surfaces/Question.tsx`

- [x] **Failing test first.** `question.answer_is_an_intent` — selecting a choice calls `session.intent` with the choice id.
- [x] `consent.is_distinct` — the consent bar is visually and structurally distinct from a question.
- [x] `composer.ctrl_c_cancels` — `^C` sends `turn.cancel` and does not exit.
- [x] Implement.

### Task 6 · Exhaustiveness

Files: `src/surfaces/index.ts`

- [x] **Failing test first.** `surfaces.every_kind_has_a_component` — iterate the `SurfaceKind` discriminants from `@orrery/protocol` and assert a component exists for each.
- [x] Implement the switch with a `never`-typed default arm, so adding a variant to the generated types is a TypeScript error here. The TS equivalent of plan 09b's exhaustive `match`.

### Task 7 · Conformance

Files: `test/conformance.test.tsx`

- [x] Run **the same** `harness/clients/conformance/*.jsonl` files the Rust SDK runs; assert the store state at each checkpoint, then snapshot the rendered frame.
- [x] A test asserting this package's `package.json` dependencies contain nothing from the harness except `@orrery/client` and `@orrery/protocol`. That is the "third-party proof" made mechanical.

### Task 8 · Custom renderer hook

Files: `src/registry.ts`, `src/surfaces/Custom.tsx`

- [x] **Failing test first.** `custom.falls_back_without_a_renderer`, `custom.uses_a_registered_renderer`.
- [x] Implement `registerRenderer`; leave loading third-party bundles as `TODO(plan-15)`.

### Task 9 · Launch integration

Files: `orrery-cli` (plan 17 owns the command; this task owns the contract)

- [x] **Failing test first.** `smoke.ink_renders_a_fixture_turn` — ~~spawn `orrery serve --provider fixture:…`~~, run this client against the endpoint, assert the final frame contains the tool table. **Amended — and the amendment's premise is now stale: `orrery serve` landed with plan 17 and no longer exits 2.** What the test does today is stand a loopback HTTP listener up in-process that speaks the two endpoints `@orrery/client` uses (`POST /control`, `POST /run` as SSE) and replays `conformance/tool-call.jsonl` + the table delta down it. Nothing on the client side is mocked: `AguiSession` → `sessionConnection` → `<App>`, the exact path `orrery --ui ink` takes. When `serve` lands, the one line that changes is the one that starts the server.
- [x] Document the `ORRERY_ENDPOINT` contract in the README so a third-party client can copy it.

---

## Done when

- `pnpm -C harness/clients/ink test` green; snapshots committed. **True:** 67 tests, 9 files, 41 committed snapshots.
- The **same** conformance fixtures pass here and in the Rust SDK. **True:** both suites read one directory — `conformance::fixtures_dir()` in `sdk-rs` is `<manifest>/../conformance` and `conformance.fixturesDir()` in `sdk-ts` is `<src>/../../conformance`, the same sixteen files — and this package asserts the store state at every checkpoint of every one of them before it draws anything.
- `package.json` depends on nothing from the harness but the two published packages. **True, and mechanical:** `test/conformance.test.tsx` reads the manifest, fails on any other `@orrery/*`, fails on a `file:`/`link:` range for anything else, and walks `src/**` refusing an import that climbs out of the package.
- ~~`orrery --ui ink` and a standalone run against `orrery serve` both render a real turn.~~ **Amended, then half-corrected on 2026-09-18.** `orrery serve` and `--ui ink` dispatch have **landed**: the spawn exists, and `orrery-cli`'s `serve::the_ink_client_reaches_the_kernel` asserts this client attaches to a running kernel over the endpoint `serve` printed, with nothing but `ORRERY_ENDPOINT` and `ORRERY_SESSION`. What is still not true is *rendering a turn it did not start*: AG-UI's HTTP transport carries frames on the body of a `POST /run` and has no passive subscribe route, so that waits on a `GET /events` route in `orrery-transport` (plan 08), not on plan 17. What *is* true: `pnpm -C harness/clients/ink start` builds a bundle and runs it, a real turn renders over a real socket in `smoke.ink_renders_a_fixture_turn`, and the two failure modes a person hits first are sentences rather than stack traces (no endpoint → exit 2; unreachable kernel → exit 1 naming the endpoint). The remaining gap is the spawn in `orrery-cli`, which plan 17 owns.
- ~~ratatui and Ink attached to **one** session simultaneously both show the same turn.~~ **Amended — not true, and not checkable yet.** It needs a kernel that listens (plan 17's `serve`) and a finished ratatui client (plan 09b, in flight beside this one). The part this plan can hold up alone is the part that would make the two agree: both clients project the *same* `SurfaceStore` from the same fixtures, and this package asserts that projection checkpoint by checkpoint. The simultaneous-attach test belongs in plan 17, where both ends exist; it is not silently dropped.

## Open questions

All four are decided. The decision is written under the question.

1. **Markdown library.** `marked` plus a small terminal formatter, `ink-markdown`, or hand-rolled?

   **Decided: hand-rolled** (`src/surfaces/Markdown.tsx`, ~150 lines). `marked` and `ink-markdown` exist to produce HTML and carry a parser this client would ship without using; neither knows the one rule that actually matters here — that a half-arrived document must be drawn plain, because a fence that has not closed yet would flip the rest of an answer into code styling and then flip it back, which reads as a bug. The subset a terminal can show (headings, emphasis, code spans, fenced code, bullet and numbered lists, rules, quotes) is small, and it is tested: `markdown.plain_until_complete` plus a parser test over each construct.

2. **Node version floor.**

   **Decided: `>=20.19`, in `engines`.** It is the floor Ink 5 and vitest 4 already impose; picking anything lower would be a number nobody had run. It is enforced twice on purpose: `engines` for the install, and `checkNode()` at startup, which throws a `Misconfigured` carrying the found version, the required version and the sentence "run `orrery` without `--ui ink` for the built-in TUI". `orrery --ui ink` must do the *first* check — whether a `node` exists at all — before spawning, because a missing binary never reaches our code; that probe is plan 17 task 2's, and this plan's contribution is that once we are running, the message is ours.

3. **Is `--ui ink` worth shipping in the binary at all?**

   **Decided: ship it, detect Node first, say plainly what is missing.** The suggestion in the question is the right one. One flag buys the React component model and the custom-renderer path (§6.3), which is the whole reason this client exists; the cost is one probe and one sentence. `--ui ratatui` stays the default on a tty, so nobody meets Node unless they asked for it, and the failure that worried the question — a failed spawn as a first experience — is a message naming what to install rather than a stack trace.

4. **Bundling.**

   **Decided for now: bundle, and keep the source honest.** `pnpm -C harness/clients/ink build` is one `esbuild` call producing a single `dist/index.js` (~2.6 MB) that runs on a bare Node with no install, which is what `orrery --ui ink` will spawn. The "third party built this" story survives it, because the bundle is only ever built from this package's public dependencies and `test/conformance.test.tsx` fails the build if that stops being true — a third party clones, `pnpm install`, and gets the same thing from the same two published packages. Two things stay open for packaging (plan 15/17), and they are packaging questions rather than client ones: signing the shipped bundle, and whether `--ui ink` prefers a bundle beside the binary or a `node_modules` the user already has. The bundle aliases `react-devtools-core` to a stub (`src/devtools-stub.ts`): Ink imports it unconditionally and only uses it under `DEV=true`, and a terminal client has no business shipping a browser devtools bridge.

---

## State

**2026-09-19 — phase 4.** The three ported extensions (plan 09, task 8) are snapshotted
here in `test/ported.test.tsx`, read from `clients/conformance/ported/*.jsonl` — which
`cargo test -p orrery-ported` generates by loading the real extensions through
`orrery-host`, so this suite and the ratatui one draw the same frames rather than two
hand-written imitations of them. Nothing in this package changed: it drew all three,
including the custom surface's fallback, as it stood.

**Done** — 2026-09-18, all nine tasks, on `feat/harness_claude-0917`.

- `harness/clients/ink` is a real package: `@orrery/client-ink`, 9 test files, 67 tests, 42 snapshots, `tsc --noEmit` clean, `cargo run -p xtask -- deps-check` clean.
- Twelve surface components, one per `SurfaceKind`, each snapshotted at 72 and at 24 columns; the narrow case asserts no line exceeds the budget, so degradation is checked rather than hoped for.
- Exhaustiveness is enforced three ways: `COMPONENTS` is a mapped type over the union, the switch's default arm is typed `never`, and a test reads the discriminants out of the generated `protocol.schema.json` so the list cannot be quietly retyped.
- The dependency rule is a test, not a promise (see the Done-when bullet).
- Two things this plan could not finish are recorded as amendments to their own Done-when bullets rather than as prose: the `orrery --ui ink` / `orrery serve` launch, and the simultaneous ratatui+Ink attach. Both wait on plan 17.
- Known gap carried from the fixtures, not introduced here: `SurfaceKind::Stream` names a channel and carries no body, so `Stream.tsx` draws the channel and its state and nothing else. A body field would be an `orrery-proto` change.
