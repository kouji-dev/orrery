# 09 · Surfaces — the vocabulary, the differ, the view bindings

**Goal.** Extensions never draw. They emit a surface tree; the kernel validates it, diffs it against the last one, and sends only what changed; the attached client renders it in its own idiom. Plus the projection that makes the loop legible: every loop event can be bound to a surface without the vocabulary growing a variant per concept. When this is done, three ported extensions render in both TUIs with no drawing code of their own.

**Covers.** §6.1 (pipeline) · §6.2 (vocabulary) · §6.3 (renderer contract) · §6.5 (streaming text) · §6.6 (tool output) · §6.7 (every loop event is a view).

**Crates.** `core/crates/orrery-surface` · `extensions/crates/orrery-ext-views-default`.

**Depends on.** [`01`](01-proto-shared-types.md) (the types), [`08`](08-protocol-transport.md) (the wire and the conformance fixtures).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **The kernel diffs, not the client.** An extension re-emits its whole surface; the kernel sends only what changed. That keeps extension code trivial and the wire small.
- Per-node blake3 so an unchanged subtree is skipped in O(1).
- `append` fast path when `next.starts_with(prev)` — the reason the op exists.
- Cost guard: accumulated patch bytes over ~60% of a full `replace` ⇒ emit `replace`.
- **Surfaces seal at `turn.settled`.** A patch for a settled surface is refused at the kernel and reported to the extension, not sent to a client that cannot apply it.
- Every renderer implements every core surface. One that cannot is not a renderer.

---

## Architecture

### Two tiers

The core surfaces are a **standard library**: typed, shipped, implemented by every renderer — which is what makes them usable as a fallback. Most extensions need nothing else. `custom` carries an extension's own `kind` and `payload` **plus a mandatory fallback** built from core surfaces.

The types are plan 01's. This plan owns their behaviour.

### The pipeline

```
extension ctx.ui.*  →  validate + policy  →  diff vs last surface  →  SurfacePatch  →  renderer
                                                                                    ↘ intent ↗
```

### The store and the differ

```rust
pub struct SurfaceStore { /* per-turn: IndexMap<SurfaceId, (Surface, NodeHashes)> */ }

impl SurfaceStore {
    pub fn emit(&mut self, turn: TurnId, id: SurfaceId, next: Surface)
        -> Result<Vec<SurfacePatch>, SurfaceError>;      // Err(Sealed) after turn.settled
    pub fn seal(&mut self, turn: TurnId);
}

pub fn diff(prev: &Surface, next: &Surface, id: SurfaceId, out: &mut Vec<SurfacePatch>) -> DiffCost;
```

Rules, in order:

1. Discriminant changed ⇒ `Replace`.
2. `text` / `stream` / `markdown` where `next.value.starts_with(&prev.value)` ⇒ `Append` with the suffix.
3. `stack` ⇒ match children by explicit `id` first, then by index; recurse.
4. Row and item collections ⇒ diff at row granularity into `Set { path }`.
5. Leaf scalar change ⇒ `Set { path }`.
6. **Cost guard** ⇒ if `out`'s serialised size exceeds `0.6 × size_of(Replace)`, discard `out` and emit one `Replace`.

The cost guard is what stops a re-sorted table producing a hundred `set` ops.

### Streaming text (§6.5)

Partial markdown is unparseable — an unclosed fence or half a table renders as garbage. Hence `markdown.complete`: the renderer shows plain text while `false`, formats once `true`. Deltas are coalesced to a frame budget (~30 fps) in the transport (plan 08), not here.

### Tool output (§6.6)

Child processes get pipes, never the frame. Output is read incrementally, truncated at the declared ceiling by the broker, and rendered as a `stream` surface. A subprocess writing straight to the terminal would corrupt it — so nothing in the surface path ever hands a child an inherited stdout.

### `question` flows backwards

The answer returns as an `intent`, so **the asking step pauses rather than the kernel**. Unattended, `default` resolves it; a missing `default` fails the step. It is never a consent prompt — those are minted by the policy engine in the client's own chrome, while a `question` is attributed to the extension that asked.

### View bindings (§6.7)

The catalogue already exists: the `Event` frames in §5.2 and the three streams in §4.12 are the full list of what happens in a loop. Making the loop legible is not inventing a surface per concept — it is binding what exists to the primitives already defined.

```rust
pub struct ViewBinding {
    pub event: EventKind,
    pub when: Option<Predicate>,          // e.g. only when the outcome is an error
    pub placement: Placement,             // Inline | Footer | Hidden
    pub render: Box<dyn Fn(&LoopEvent) -> Surface + Send + Sync>,   // code ⇒ an extension
}
```

A profile moves or silences them without touching code:

```toml
[views."router.decided"]  placement = "inline"
[views."tool.settled"]    placement = "footer"
[views."skill.loaded"]    placement = "hidden"
```

**Unbound is hidden, with one floor.** An event nobody bound does not render. The floor — assistant text, tool started and settled, consent, errors — ships with default bindings in `orrery-ext-views-default`, because a client showing nothing until configured is broken rather than minimal. **The json renderer ignores `placement` entirely**: hiding is a human-client concern.

### Custom surfaces and the fallback bargain

A client with a matching renderer draws the rich version; every other draws the fallback. Same bargain as Jupyter display data. Two rules with teeth:

- The fallback must be informative, not `text("open the web UI")`. `--json` emits it beside the payload, so a lazy one shows up in CI.
- Third-party drawing code in someone's client takes a `render` grant, runs sandboxed, and appears in the ledger; **one deny rule degrades every custom surface everywhere.**

---

## File structure

**Create**

- `harness/core/crates/orrery-surface/src/{lib,store,diff,hash,validate,sink}.rs` (and `view` — **amended:** it landed in `orrery-ext-api/src/view.rs`, see Task 5)
- `harness/core/crates/orrery-surface/tests/{diff,seal,view}.rs`
- `harness/extensions/crates/orrery-ext-views-default/{Cargo.toml,orrery.toml,README.md,src/lib.rs}`
- fixtures appended to `harness/clients/conformance/`

---

## Tasks

### Task 1 · Validation

Files: `src/validate.rs`

- [x] **Failing test first.** `validate::custom_kind_is_namespaced` — `kind: "flamegraph"` is rejected; `kind: "buildgraph.flamegraph"` is accepted.
- [x] `validate::fallback_must_not_be_trivial` — a fallback that is `text` with fewer than N characters, or whose text matches a deny-list of phrases like "open the", produces a **load-time warning** (not an error — we cannot judge content, but we can nudge). `N = MIN_FALLBACK_CHARS = 16`, and the phrase check runs *first* because "open the web UI" is also short and naming the phrase is the more useful thing to say.
- [x] `validate::nesting_depth_is_capped` — a 1000-deep stack is rejected before it reaches a renderer. `MAX_DEPTH = 64`.
- [x] Implement.

These three live in `src/validate.rs` as a `#[cfg(test)] mod tests`, not in `tests/`, because they exercise a private walk and the plan's own file list gives `validate.rs` no test file.

### Task 2 · Hashing and the differ

Files: `src/{hash,diff}.rs`, `tests/diff.rs`

- [x] **Failing test first.** `diff::append_fast_path` — a markdown surface growing by one word produces exactly one `Append` carrying only the new word.
- [x] `diff::unchanged_subtree_is_skipped` — a stack of 100 children where one changes produces one `Set`, and an instrumented hash counter proves the other 99 subtrees were not walked. `DiffCost` carries `nodes_walked` (2: the root and the child) and `subtrees_skipped` (99).
- [x] `diff::discriminant_change_replaces`.
- [x] `diff::stack_children_match_by_id` — reordering children with stable ids produces moves, not wholesale replacement. **Amended:** the vocabulary has no `Move` op, so a move is expressed as one `Set` at the slot that took a different child — two ops for a three-child reversal, and neither subtree is walked. That is "not wholesale replacement" within the ops that exist; adding a `Move` op is an `orrery-proto` change, not this plan's.
- [x] `diff::cost_guard_collapses` — a table whose rows are all reordered produces one `Replace`, not N `Set`s. Assert on patch count and byte size.
- [x] `diff::patches_reconstruct` (proptest) — for arbitrary `prev`/`next`, applying `diff(prev,next)` to `prev` yields `next`. **This is the property that matters most.** 256 cases over all twelve variants, stacks and custom fallbacks nested three deep. Its inverse, `diff::apply`, is public — clients have their own copy in their own language, and this is the one the property is checked against.
- [x] Implement.

**Amended: the guard needed a floor.** Every op carries a surface id and a path, which on a
two-row table costs more than the table does — so a bare `patch_bytes > 0.6 × replace_bytes`
collapsed one-cell edits into whole-surface resends and took the append fast path with them.
`COST_GUARD_FLOOR = 512` bytes: below that a surface is too small for the guard to be worth
having, and the guard exists to stop a re-sorted *table*, not to punish a one-word edit.

### Task 3 · The store and sealing

Files: `src/store.rs`, `tests/seal.rs`

- [x] **Failing test first.** `seal::patch_after_settle_is_refused` — emit, seal, emit again ⇒ `Err(SurfaceError::Sealed)` returned **to the extension**, and no frame produced. `remove` is refused on the same rule.
- [x] `seal::new_surface_in_the_current_turn_is_fine` — an extension with something to add after its turn ends emits a new surface in the current turn.
- [x] Implement per-turn storage and `seal`.

**Amended: `SurfaceError` is this crate's, not `orrery-proto`'s.** `orrery_proto::SurfaceError`
describes what is wrong with a surface's *shape* and is `#[non_exhaustive]` in a crate this
plan does not own. `Sealed` needs a turn and a store to be wrong about, so
`orrery_surface::SurfaceError` wraps the proto one (`Malformed`) and adds `Sealed` and
`TooDeep`.

### Task 4 · The surface sink for extensions

Files: `src/sink.rs`

- [x] **Failing test first.** `sink::describes_never_draws` — the API exposes no way to write bytes to a terminal; a doc test shows `ctx.ui.table(..)`. Two halves: every builder returns a `Surface` and takes no terminal, buffer or width, and a source scan refuses `std::io`, `Stdout`, `print!`, `crossterm`, `ratatui` and friends in this file.
- [x] Implement `SurfaceSink` with builders for each core surface (`text`, `table`, `tree`, `diff`, `progress`, `stream`, `task`, `question`, `form`, `stack`, `markdown`, `custom`).

**Amended: an extension trait, not a second sink.** `orrery_ext_api::SurfaceSink` already
exists and is what `ctx.ui` *is* — it carries `text`, `table` and `markdown`. Defining a
second `SurfaceSink` here would mean two `ctx.ui` types and two places a surface is emitted.
So `src/sink.rs` adds the other nine as `trait SurfaceBuilders for SurfaceSink`. The cost is
one `use orrery_surface::SurfaceBuilders;` in an extension that wants them.

### Task 5 · View bindings

Files: `src/view.rs`, `tests/view.rs`

- [x] **Failing test first.** `view::unbound_is_hidden` — an event with no binding produces no surface.
- [x] `view::floor_is_always_bound` — with zero configuration, assistant text, tool started/settled, consent and errors all render. **Amended:** "zero configuration" means "once the floor extension has loaded, and with no profile" — a bare `ViewRegistry::new()` binds nothing, because unbound-is-hidden has no exceptions. The test asserts against `orrery_surface::floor()`, the binding *content*; `orrery-ext-views-default`'s own `views::ships_as_an_extension` asserts the *loading path*.
- [x] `view::placement_from_profile` — a `[views.*]` table moves a binding to the footer. An override for an event nothing binds is kept, not refused: a profile is written once and extensions come and go.
- [x] `view::json_ignores_placement`.
- [x] `view::when_predicate` — a binding with `when` only fires on matching events.
- [x] Implement `ViewBinding`, `Placement`, the registry, profile overrides.

**Amended: the view vocabulary moved to `orrery-ext-api`.** The plan's file list
puts `view.rs` in `orrery-surface`, and that made `orrery-ext-views-default` — an
extension — depend on a `publish = false` core crate, which `xtask deps-check`
rule 2 refuses and was right to. The diagnosis is that the types were in the
wrong crate, not that the rule was too strict: contributing a *view* is an
extension's job exactly as contributing a *tool* is, so `EventKind`, `LoopEvent`,
`Placement`, `Predicate`, `ViewBinding`, `Placed`, `ViewRegistry` and `floor()`
now live in `orrery-ext-api::view` beside `ToolDef` and `CallCtx`.
`orrery-surface` keeps what the kernel owns — the differ, the hashes, the
per-turn store, sealing, validation — and re-exports the view names, so
kernel-side code and `tests/view.rs` read exactly as before.

**Amended: `EventKind` and `LoopEvent` are defined here.** Neither existed. `EventKind` is a
newtype over the dotted name a profile writes rather than an enum, because an extension
contributes its own loop events and a closed enum would grow a variant per concept — which
is what §6.7 exists to avoid. `LoopEvent` wraps `orrery_proto::Event` (the wire catalogue),
plus `AssistantText` (prose arrives as deltas, not as a frame) and `Other { kind, payload }`
(an extension's own). `render: Box<dyn Fn>` is an `Arc<dyn Fn>` so a registry can be cloned.

### Task 6 · Default views extension

Files: `orrery-ext-views-default/*`

- [x] Implement the floor bindings as a real extension with an `orrery.toml` — proving the mechanism on the thing that most tempts a built-in shortcut. `DefaultViews` implements `NativeExtension`, contributes five `views` and **no tools**, and asks for no capabilities, so it loads clean under `Grant::nothing()`.
- [x] **Failing test first.** `views::ships_as_an_extension` — the bundle loads through `orrery_ext_api::testing::load_for_test` and appears in the ledger entry a real session would show. Its manifest is parsed by the same parser a third party is held to, and a second test asserts the manifest's `views` list and the code's bindings are the same list in the same order.

**Amended: it loads through `orrery-ext-api::testing`, not `orrery-host`.** The
test dev-depended on `orrery-host`, which is `publish = false`, and
`deps-check` rule 3 refuses that for the reason the rule exists: an extension's
tests use the published mock-broker harness, not kernel internals. That harness
is the one `orrery ext test` runs and the one plan 18 points a community author
at, so loading the floor through it is the proof that the path we recommend
works. The crate now has no dev-dependencies at all.

### Task 7 · Conformance fixtures

Files: `harness/clients/conformance/*`

- [x] Add one scenario per core surface: emit, mutate, assert the patch sequence and the resulting store state. These are what plans 09b and 09c run. Six were missing and are new: `tree-surface`, `diff-surface`, `progress-surface`, `stream-surface`, `task-surface`, `form-surface`. The other six were already covered by plan 08's fixtures, and the conformance README now carries a surface-to-scenario table so a gap is visible rather than inferred.
- [x] Add `table-then-resort` (cost guard) and `streaming-markdown` (complete flag) explicitly — they are the two cases renderers get wrong. Both existed from plan 08; they are now named as such in the README, with *why* each is the one renderers get wrong.

**Amended: `stream-surface` cannot assert a stream's body.** `SurfaceKind::Stream` names a
channel and carries no text field, so there is nowhere in a client's store for a child
process's bytes to land. The scenario pins everything else — the channel is named, the
surface opens `running`, it can be re-pointed with one op, the turn closes it — and the file
and the README both say plainly what is missing. Closing it is an `orrery-proto` change.

### Task 8 · Phase 4 — the porting exercise

Files: `harness/extensions/examples/{workspace-census,patch-review,release-train}/*` ·
`harness/clients/ported/*` · `harness/clients/conformance/ported/*` · the three
renderers' test suites

§8 calls this "the honest test of the schema": three real extensions, with an
`orrery.toml`, loaded through `orrery-host`, emitting real surfaces and drawing none of
them — and *"they needed escape hatches"* is the finding that matters.

- [x] **Failing test first.** `ported::draws_nothing` — a source scan over the three
  extensions for `std::io`, `Stdout`, `print!`, `crossterm`, `ratatui`, `ansi`, an escape
  byte, and anything that knows a client's `width`.
- [x] Three extensions covering different surface kinds: `workspace-census` (section,
  markdown, table, styled text), `patch-review` (diff, question), `release-train` (task,
  progress, custom + fallback, emitted twice so the second is a re-emission).
- [x] `orrery-ported` runs them the real way — `NativeRegistry` → `ExtensionTable::load`
  under `Grant::nothing()` → `Registry::dispatch` → `ctx.ui` → `SurfaceStore` → differ →
  `agui::Encoder` — and the `clients/conformance/ported/*.jsonl` fixtures are **written
  from that run** and asserted against it, which is how Ink snapshots the same frames.
- [x] ratatui: a snapshot each, plus `the_custom_surface_falls_back`.
- [x] Ink: a snapshot each, plus the fallback named stage by stage.
- [x] json: the payload and the mandatory fallback beside it, with every stage the payload
  carries asserted present in the fallback (§6.2).

**Three findings, in the order they cost something.**

1. **Nine of the twelve builders were unreachable from a publishable extension.**
   `SurfaceBuilders` lived in `orrery-surface`, which is `publish = false`, so a community
   extension could describe `text`, `table` and `markdown` and nothing else — while this
   plan's own "an extension can produce every core surface" was asserted from a crate no
   extension may depend on. Same diagnosis as the view vocabulary, same fix: the trait is
   now in `orrery-ext-api`, re-exported here. **This is the escape hatch §8 asked about,
   and it was ours rather than theirs.**
2. **`with_id` did not exist.** Open question 4 decided ids are extension-minted and
   re-emission is the whole API; there was no way to say an id from inside `ctx.ui`, so
   every re-emission looked like a new surface. Added, beside `with_status`.
3. **The vocabulary itself needed no escape hatch.** Twelve surfaces covered three
   unrelated extensions, and the one thing that genuinely was not in it — a timeline — is
   exactly what `custom` is for. No extension wanted a variant that does not exist.

**And two client bugs the exercise found**, neither of them schema problems:

- `--json` emitted a custom surface's fallback only when the custom surface was the *whole*
  surface. Extensions compose, so a nested one never reached CI — the one thing §6.2 asks
  of that renderer. It now walks stacks and fallbacks.
- The ratatui markdown widget printed `**4**` where Ink printed `4`. Weight is free under
  open question 1; the characters are not, and that is the incomparability the rule
  forbids. It now parses inline emphasis into styled runs.

**One cost worth writing down.** A `custom` payload is opaque to the differ — `diff.rs`
emits the *whole* payload as one `Set` when any part of it changes. In `release-train` that,
plus a fallback that mirrors the payload, put the patch over the cost guard's 60%, so a
re-emission that changed one stage sent the whole surface again. That is the guard working
as designed, and it means a *streaming* custom surface re-sends everything on every tick.
Fixing it means either a JSON-level differ for payloads or a `SurfacePatch` that can
address inside one — an `orrery-proto` change, not this plan's, and nothing needs it yet.

---

## Done when

- [x] `cargo test -p orrery-surface -p orrery-ext-views-default` green, including the reconstruct proptest. 32 tests in `orrery-surface` (11 unit, 9 diff, 5 seal, 6 view, 1 doc), 3 in `orrery-ext-views-default`.
- [x] The conformance fixtures cover every core surface — **with one stated exception**: `stream-surface` covers the stream surface but not a stream's *body*, which the type cannot carry. The gap is documented in the fixture and in the conformance README rather than left to be discovered.
- [x] An extension can produce every core surface without importing a drawing library. `sink::describes_never_draws` builds all twelve through `ctx.ui` and then scans this crate's source for anything that could write a byte.
- [x] A sealed surface's patch is reported to the extension, never sent. `SurfaceStore::emit` into a sealed turn returns `Err(SurfaceError::Sealed)` and produces no patch at all.

- [x] **The goal line, run.** "Three ported extensions render in both TUIs with no drawing
  code of their own" is no longer a claim about a future phase: `workspace-census`,
  `patch-review` and `release-train` each load through `orrery-host` from an `orrery.toml`,
  emit surfaces through `ctx.ui`, and are snapshotted in ratatui, in Ink and in `--json`.
  The "no drawing code" half is a source scan that fails on `std::io`, `print!`, a terminal
  crate or a width. See Task 8 for what it found.

- [ ] **Open, named in round 5: the shipped binary does not link this crate.**
  `orrery-surface` is absent from `cargo tree -p orrery-cli`, with or without
  `--all-features`, and the reason is structural rather than an oversight.
  `ExtensionTable` holds **one session-wide** `SurfaceSink` and hands every call
  a clone of it, while `SurfaceEmit::emit(&self, surface: &Surface)` — the
  published signature in `orrery-ext-api` — carries neither a `TurnId` nor a
  `SurfaceId`. A kernel-side `SurfaceStore` therefore has nothing to key a diff
  on: it cannot tell a re-emission of one surface from a second surface, which
  is the one distinction the differ exists to make. The consequence today is
  that an extension's `ctx.ui.*` output is described, returned in the tool's
  `Outcome`, and **discarded** by the table's `SurfaceSink::discarding()`; the
  only surfaces a client sees are the ones `orrery-cli`'s own `Publisher` mints
  by hand for assistant text and tool arguments.

  The fix is an addition, not a break, and it already has a model in this
  codebase: mirror `BrokerSource`. A `SurfaceSource` on `orrery-ext-api` with
  `fn for_call(&self, call: CallId) -> SurfaceSink`, a second field on
  `ExtensionTable` beside `brokers`, and a `SurfaceSource` implementation in
  `orrery-harness` holding `Mutex<SurfaceStore>` and keying each call's surface
  off its `CallId` — which is what `orrery-cli` already does for tool arguments
  (`SurfaceId::from_uuid(*call.as_uuid())`). Not attempted in round 5 because it
  touches the published extension API across three crates and deserves its own
  failing test rather than a drive-by.

## State

Amended 2026-09-19 (phase 4, the porting exercise): three ported extensions live in
`harness/extensions/examples/`, `harness/clients/ported` runs them through `orrery-host`
for real, and all three renderers snapshot what they emit. `SurfaceBuilders` moved to
`orrery-ext-api` and gained `with_id`; the `json` renderer now walks nested custom
surfaces; the ratatui markdown widget now renders inline emphasis instead of showing it.
`cargo test -p orrery-surface -p orrery-ext-api -p workspace-census -p patch-review
-p release-train -p orrery-ported -p orrery-client-ratatui -p orrery-client-json` is green,
`pnpm -C harness/clients/ink test` is green, and `cargo run -q -p xtask -- deps-check`
prints `ok`.

Amended 2026-09-18 (wave-3 audit repair): the view vocabulary moved from
`orrery-surface` to `orrery-ext-api`, and `orrery-ext-views-default` now depends
on `orrery-ext-api` alone and loads through its published test harness.
`cargo run -q -p xtask -- deps-check` prints `ok`. Test counts are unchanged: 32
in `orrery-surface`, 3 in `orrery-ext-views-default`.

Landed 2026-09-18 on `feat/harness_claude-0917`. `orrery-surface` is implemented end to end
— `validate`, `hash`, `diff` (+ its inverse `apply`), `store`, `sink`, `view` — and
`orrery-ext-views-default` ships the floor through `orrery-host`. Six conformance scenarios
added, one per previously-uncovered core surface; all 16 pass under `orrery-client`'s
runner. Every "Amended" note above is a place the plan and the code disagreed and the code
won for a stated reason. Open questions 1–4 are decided below.

## Open questions

1. **How far may a client re-style a surface before output stops being comparable across clients?** §7 names this as the one unsettled line. It matters for evals (two runs must be comparable) and for docs. Propose: themes and spacing are free; content, ordering and which surface was used are not. Write the rule here.

   **Decided: the comparability rule.** A client may change *how* a surface looks. It may
   not change *what* was said, *in what order*, or *which surface said it*.

   **Free — a client may do any of this without becoming incomparable:**
   - colour, including mapping `TextStyle` to whatever its palette says (`style` is a hint
     about meaning, never a colour);
   - typeface, weight, spacing, padding, borders, box-drawing, indentation and wrapping;
   - glyphs and iconography — a bullet, a spinner, a check mark, nothing at all;
   - **progressive disclosure**: collapsing a `stack`, paginating a long `table`, folding a
     `diff` hunk, truncating with an affordance that reveals the rest. What is hidden must
     be reachable without re-running the turn;
   - layout: a `stack` with `dir: row` laid out as a column on a narrow terminal;
   - `placement` — inline, footer or hidden, per the profile. This is the one *content*
     omission that is free, and it is why **the json renderer ignores `placement`**: the
     comparable rendering is the one that omits nothing.

   **Not free — any of this makes two runs incomparable, and is a renderer bug:**
   - changing, summarising, reordering, re-sorting or reformatting the *values* in a
     surface. A table's rows are drawn in the order they arrived unless the person sorts
     them; a re-sort is the person's, never the renderer's;
   - changing the order surfaces are drawn in relative to their arrival order;
   - substituting a different surface kind for the one the extension chose — drawing a
     `table` as prose, a `question` as a `form`, a `diff` as a `stream`;
   - drawing a `custom` payload when the client has no renderer for that `kind`, or drawing
     *neither* the payload nor the fallback;
   - dropping a surface that is not `Hidden` by placement.

   **The one legitimate substitution is degradation**, and it is a documented rule rather
   than a choice — see question 2. A renderer that cannot draw a surface falls back by the
   published rule for that surface, not by inventing one.

   **How it is enforced.** The conformance fixtures assert store *state*, which is content
   and ordering, and say nothing about pixels — so the line above is exactly the line
   between "the fixtures check it" and "the fixtures do not". An eval compares two runs'
   json rendering, which ignores placement and omits nothing. Drawing snapshots live with
   each client and are a second layer on top.
2. **`form` degradation.** §6.3 says `form` → sequential prompts in a constrained renderer. Is that a renderer's choice or a documented rule? Make it a rule, or two TUIs will differ.

   **Decided: a rule.** A renderer that cannot draw a whole `form` asks one field at a time:

   - **in declaration order** — `fields` as the extension wrote them, never sorted, never
     "required first";
   - **each as that renderer's own `question`**, with the field's `label` as the prompt. A
     `choice` field offers its `choices`; `bool` offers yes/no; `text`, `number` and
     `secret` are free-text, and `secret` is not echoed, not logged and not stored;
   - **`default` pre-fills**, and an empty answer takes it;
   - **an optional field may be skipped**; a required field with no answer and no default
     re-asks, and cancelling the sequence cancels the whole form — never a partial submit;
   - **submit once the last field is answered**, with `submit` as the confirmation label.
     One submission, carrying every field, exactly as a whole-form renderer would send it.

   The rule behind this and every other degradation: *degrade to another core surface by a
   published rule, never by invention.* `form-surface` in the conformance directory carries
   this rule in its header, so a renderer author meets it where they meet the fixture.

3. **Trivial-fallback detection.** A warning based on a phrase deny-list is crude. Better idea: require the fallback to be a different variant than `text`, unless the custom surface is itself textual? Probably too strict. Keep the warning, revisit after phase 4's porting exercise.

   **Decided: keep the warning exactly as proposed, and do not make it an error.**
   Implemented as `MIN_FALLBACK_CHARS = 16` plus an eight-phrase list, checked phrase-first
   so the more useful message wins; only `text` fallbacks are judged, since a `table` or
   `tree` fallback is by construction saying something. The "different variant than `text`"
   idea is rejected: the honest fallback for a flamegraph *is* a line of text saying where
   the time went, and a rule that refused it would be a rule people route around.

   It is a warning and not an error for one reason worth writing down: a bad fallback makes
   a transcript worse, while refusing to load makes the extension useless. The nudge belongs
   on the cheaper side of that trade.

   **Revisited after phase 4, with a real fallback to look at. The warning stays, and it
   never fired.** `release-train`'s fallback is a `stack` — a headline plus a table — so
   `validate` did not judge it at all: only `text` fallbacks are. That is the honest result,
   and it says the load-time check is not where this is enforced. What actually held the
   fallback to account was a *test*: `the_fallback_is_informative` asserts that every stage
   the payload names appears in the rendered fallback, which is a rule about **this**
   extension that only its author could write. So the answer to "the phrase list is crude"
   is not a better list; it is that an extension proves its own fallback in its own tests,
   and `--json` printing the fallback beside the payload is what makes a missing proof
   visible to a reviewer. If a lint is ever added to `orrery ext test`, this is the shape:
   compare the fallback's text against the payload's own strings, not against a deny-list.

4. **Surface ids.** Who mints them — the extension or the kernel? Extension-minted is simpler for re-emission; kernel-minted prevents collisions across extensions. Suggest extension-minted, namespaced by ext id at the kernel.

   **Decided: extension-minted, scoped by the kernel — and the scope is the turn, not the
   extension.** An extension mints a `SurfaceId` and re-emits under it; that is what makes
   re-emission the whole API and the diff the store's job. The kernel is what keeps ids
   apart: `SurfaceStore` is keyed `(TurnId, SurfaceId)`, so the same id in two turns is two
   surfaces and nothing leaks across a session — `seal::new_surface_in_the_current_turn_is
   _fine` pins exactly that.

   Per-extension namespacing on top is unnecessary *while* ids are uuid v7: a collision
   between two extensions would be a uuid collision. If ids ever become human-chosen strings,
   the kernel prefixes with the ext id inside `emit` — a change in one function, and it is
   one function precisely because the kernel already owns the keying.
