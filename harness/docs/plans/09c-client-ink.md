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

- `harness/clients/ink/{package.json,tsconfig.json,README.md}`
- `harness/clients/ink/src/{index.tsx,app.tsx,composer.tsx,footer.tsx,consent.tsx,theme.ts,registry.ts}`
- `harness/clients/ink/src/surfaces/*.tsx`
- `harness/clients/ink/test/{conformance.test.tsx,surfaces.test.tsx}`

---

## Tasks

### Task 1 · Package and connection

Files: `package.json`, `src/index.tsx`, `src/app.tsx`

- [ ] **Failing test first.** `app.connects_and_drains` — with a fixture-backed fake transport, the app processes every event and unmounts cleanly.
- [ ] Set up the package: `react`, `ink`, `@orrery/client`, `@orrery/protocol`, vitest, `ink-testing-library`. **No other harness dependency.**
- [ ] `--endpoint` flag and `ORRERY_ENDPOINT`; friendly error when neither is set.
- [ ] Wire `AguiSession` + `SurfaceStore` from `@orrery/client`; the app holds them, components receive plain data.

### Task 2 · The Static / live split

Files: `src/app.tsx`

- [ ] **Failing test first.** `app.settled_turns_go_static` — two turns; assert the settled ones are inside `<Static>` and that re-rendering does not re-emit them (assert on `lastFrame()` stability and render counts).
- [ ] `app.gap_triggers_reattach` — a `seq` gap causes a re-attach with `since`.
- [ ] Implement.

### Task 3 · Surface components

Files: `src/surfaces/*`, `test/surfaces.test.tsx`

For each surface, in the same groups as plan 09b:

- [ ] **Failing test first.** `surfaces.<name>` — render the conformance fixture, snapshot `lastFrame()`.
- [ ] Implement the component.
- [ ] Add the degradation snapshot (narrow width).

### Task 4 · Streaming markdown

Files: `src/surfaces/Markdown.tsx`

- [ ] **Failing test first.** `markdown.plain_until_complete` — an unclosed fence renders plain; `complete: true` renders formatted.
- [ ] Implement, with a small markdown renderer (`marked` + a terminal formatter, or hand-rolled — **decide and record**; a heavy dependency here is not worth much).

### Task 5 · Question, consent, composer

Files: `src/{consent,composer}.tsx`, `src/surfaces/Question.tsx`

- [ ] **Failing test first.** `question.answer_is_an_intent` — selecting a choice calls `session.intent` with the choice id.
- [ ] `consent.is_distinct` — the consent bar is visually and structurally distinct from a question.
- [ ] `composer.ctrl_c_cancels` — `^C` sends `turn.cancel` and does not exit.
- [ ] Implement.

### Task 6 · Exhaustiveness

Files: `src/surfaces/index.ts`

- [ ] **Failing test first.** `surfaces.every_kind_has_a_component` — iterate the `SurfaceKind` discriminants from `@orrery/protocol` and assert a component exists for each.
- [ ] Implement the switch with a `never`-typed default arm, so adding a variant to the generated types is a TypeScript error here. The TS equivalent of plan 09b's exhaustive `match`.

### Task 7 · Conformance

Files: `test/conformance.test.tsx`

- [ ] Run **the same** `harness/clients/conformance/*.jsonl` files the Rust SDK runs; assert the store state at each checkpoint, then snapshot the rendered frame.
- [ ] A test asserting this package's `package.json` dependencies contain nothing from the harness except `@orrery/client` and `@orrery/protocol`. That is the "third-party proof" made mechanical.

### Task 8 · Custom renderer hook

Files: `src/registry.ts`, `src/surfaces/Custom.tsx`

- [ ] **Failing test first.** `custom.falls_back_without_a_renderer`, `custom.uses_a_registered_renderer`.
- [ ] Implement `registerRenderer`; leave loading third-party bundles as `TODO(plan-15)`.

### Task 9 · Launch integration

Files: `orrery-cli` (plan 17 owns the command; this task owns the contract)

- [ ] **Failing test first.** `smoke.ink_renders_a_fixture_turn` — spawn `orrery serve --provider fixture:…`, run this client against the endpoint, assert the final frame contains the tool table.
- [ ] Document the `ORRERY_ENDPOINT` contract in the README so a third-party client can copy it.

---

## Done when

- `pnpm -C harness/clients/ink test` green; snapshots committed.
- The **same** conformance fixtures pass here and in the Rust SDK.
- `package.json` depends on nothing from the harness but the two published packages.
- `orrery --ui ink` and a standalone run against `orrery serve` both render a real turn.
- ratatui and Ink attached to **one** session simultaneously both show the same turn.

## Open questions

1. **Markdown library.** `marked` plus a small terminal formatter, `ink-markdown`, or hand-rolled? Hand-rolled is ~150 lines for the subset a terminal can show and avoids a dependency that will want to render HTML. Lean hand-rolled; record the decision.
2. **Node version floor.** Ink 5+ needs a recent Node. Pick one, put it in `engines`, and make sure `orrery --ui ink` fails with a clear message rather than a stack trace when it is not met.
3. **Is `--ui ink` worth shipping in the binary at all**, given it requires Node on the user's machine? Argument for: it is one flag and it is how custom renderers become possible. Argument against: a failed spawn is a bad first experience. Suggest: ship it, detect Node first, and say plainly what is missing.
4. **Bundling.** Shipping the Ink client as a single bundled `.js` beside the binary vs. requiring a pnpm install. Bundling is friendlier; it also means the "third party built this" story is slightly less honest. Revisit at packaging time.
