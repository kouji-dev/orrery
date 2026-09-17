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

- `harness/core/crates/orrery-surface/src/{lib,store,diff,hash,validate,view,sink}.rs`
- `harness/core/crates/orrery-surface/tests/{diff,seal,view}.rs`
- `harness/extensions/crates/orrery-ext-views-default/{Cargo.toml,orrery.toml,README.md,src/lib.rs}`
- fixtures appended to `harness/clients/conformance/`

---

## Tasks

### Task 1 · Validation

Files: `src/validate.rs`

- [ ] **Failing test first.** `validate::custom_kind_is_namespaced` — `kind: "flamegraph"` is rejected; `kind: "buildgraph.flamegraph"` is accepted.
- [ ] `validate::fallback_must_not_be_trivial` — a fallback that is `text` with fewer than N characters, or whose text matches a deny-list of phrases like "open the", produces a **load-time warning** (not an error — we cannot judge content, but we can nudge).
- [ ] `validate::nesting_depth_is_capped` — a 1000-deep stack is rejected before it reaches a renderer.
- [ ] Implement.

### Task 2 · Hashing and the differ

Files: `src/{hash,diff}.rs`, `tests/diff.rs`

- [ ] **Failing test first.** `diff::append_fast_path` — a markdown surface growing by one word produces exactly one `Append` carrying only the new word.
- [ ] `diff::unchanged_subtree_is_skipped` — a stack of 100 children where one changes produces one `Set`, and an instrumented hash counter proves the other 99 subtrees were not walked.
- [ ] `diff::discriminant_change_replaces`.
- [ ] `diff::stack_children_match_by_id` — reordering children with stable ids produces moves, not wholesale replacement.
- [ ] `diff::cost_guard_collapses` — a table whose rows are all reordered produces one `Replace`, not N `Set`s. Assert on patch count and byte size.
- [ ] `diff::patches_reconstruct` (proptest) — for arbitrary `prev`/`next`, applying `diff(prev,next)` to `prev` yields `next`. **This is the property that matters most.**
- [ ] Implement.

### Task 3 · The store and sealing

Files: `src/store.rs`, `tests/seal.rs`

- [ ] **Failing test first.** `seal::patch_after_settle_is_refused` — emit, seal, emit again ⇒ `Err(SurfaceError::Sealed)` returned **to the extension**, and no frame produced.
- [ ] `seal::new_surface_in_the_current_turn_is_fine` — an extension with something to add after its turn ends emits a new surface in the current turn.
- [ ] Implement per-turn storage and `seal`.

### Task 4 · The surface sink for extensions

Files: `src/sink.rs`

- [ ] **Failing test first.** `sink::describes_never_draws` — the API exposes no way to write bytes to a terminal; a doc test shows `ctx.ui.table(..)`.
- [ ] Implement `SurfaceSink` with builders for each core surface (`text`, `table`, `tree`, `diff`, `progress`, `stream`, `task`, `question`, `form`, `stack`, `markdown`, `custom`).

### Task 5 · View bindings

Files: `src/view.rs`, `tests/view.rs`

- [ ] **Failing test first.** `view::unbound_is_hidden` — an event with no binding produces no surface.
- [ ] `view::floor_is_always_bound` — with zero configuration, assistant text, tool started/settled, consent and errors all render.
- [ ] `view::placement_from_profile` — a `[views.*]` table moves a binding to the footer.
- [ ] `view::json_ignores_placement`.
- [ ] `view::when_predicate` — a binding with `when` only fires on matching events.
- [ ] Implement `ViewBinding`, `Placement`, the registry, profile overrides.

### Task 6 · Default views extension

Files: `orrery-ext-views-default/*`

- [ ] Implement the floor bindings as a real extension with an `orrery.toml` — proving the mechanism on the thing that most tempts a built-in shortcut.
- [ ] **Failing test first.** `views::ships_as_an_extension` — the bundle loads through `orrery-host` and appears in the ledger.

### Task 7 · Conformance fixtures

Files: `harness/clients/conformance/*`

- [ ] Add one scenario per core surface: emit, mutate, assert the patch sequence and the resulting store state. These are what plans 09b and 09c run.
- [ ] Add `table-then-resort` (cost guard) and `streaming-markdown` (complete flag) explicitly — they are the two cases renderers get wrong.

---

## Done when

- `cargo test -p orrery-surface -p orrery-ext-views-default` green, including the reconstruct proptest.
- The conformance fixtures cover every core surface.
- An extension can produce every core surface without importing a drawing library.
- A sealed surface's patch is reported to the extension, never sent.

## Open questions

1. **How far may a client re-style a surface before output stops being comparable across clients?** §7 names this as the one unsettled line. It matters for evals (two runs must be comparable) and for docs. Propose: themes and spacing are free; content, ordering and which surface was used are not. Write the rule here.
2. **`form` degradation.** §6.3 says `form` → sequential prompts in a constrained renderer. Is that a renderer's choice or a documented rule? Make it a rule, or two TUIs will differ.
3. **Trivial-fallback detection.** A warning based on a phrase deny-list is crude. Better idea: require the fallback to be a different variant than `text`, unless the custom surface is itself textual? Probably too strict. Keep the warning, revisit after phase 4's porting exercise.
4. **Surface ids.** Who mints them — the extension or the kernel? Extension-minted is simpler for re-emission; kernel-minted prevents collisions across extensions. Suggest extension-minted, namespaced by ext id at the kernel.
