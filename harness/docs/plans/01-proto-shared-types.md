# 01 · `orrery-proto` — the wire and the shared vocabulary

**Goal.** One crate that holds every type two other crates need to agree on, with no async, no I/O and no dependency on anything but serde and schemars. When this is done, `cargo xtask typegen` emits a JSON Schema and a committed `protocol.d.ts`, the frame fixtures round-trip in both JSON and CBOR, and `Grant::intersect` is property-tested. Nothing else in the harness can be built before it.

**Covers.** §4.15 (shared types) · §5.2 (frames) · §6.2 (surface vocabulary, types only — the differ is plan 09) · the id, `Grant`, `Budget` and `Usage` shapes assumed throughout §4.

**Crates.** `core/crates/orrery-proto` (published), `harness/xtask`, `harness/protocol`.

**Depends on.** Nothing. Everything depends on this.

---

## Constraints

From [`00-overview.md`](00-overview.md), binding here:

- No async, no I/O, no `tokio`. Dependencies: `serde`, `serde_json` (tests), `schemars`, `uuid`, `thiserror`. Nothing else.
- `#[serde(tag = "t")]` with **explicit per-variant `rename`** — dotted names are not a `rename_all` rule.
- `#[non_exhaustive]` on every enum that crosses the wire.
- CBOR must work, which is why every tagged enum variant is a **struct variant** (internal tagging rejects newtype variants around non-maps).
- No floats for money: `max_micro_usd: u64` (translation #7).
- `Grant::intersect` is explicit, with `Option` fields (translation #11).

This crate owns translations **#7** and **#11** and the type half of **#2** (`Verdict<P>`).

---

## Architecture

### Module layout

```
orrery-proto/src/
├─ lib.rs          # re-exports; crate-level docs; #![deny(missing_docs)]
├─ ids.rs          # opaque newtypes over uuid v7 + Seq
├─ grant.rs        # Capability, Aspect, Grant, Consent, intersect
├─ budget.rs       # Budget, BudgetKind, Usage, TokenBudget, micro-USD
├─ scope.rs        # AgentScope, Role, Layer, Subject
├─ message.rs      # Message, ContentBlock, ToolUse, ToolResultBlock
├─ surface.rs      # Surface, SurfacePatch, Status, TaskItem, Choice, Field…
├─ frame.rs        # Request, Event, UserInput, ConsentPrompt, Outcome, ErrorDetail
├─ expr.rs         # Expr, Predicate, Literal
├─ load.rs         # LoadOutcome, Contribution, ExtId
├─ tool.rs         # ToolDescriptor — the one copy (task 11)
└─ verdict.rs      # Verdict<P> (the phase-payload enum; the Phase trait is plan 05)
```

### Ids

```rust
/// uuid v7, not v4 and not ulid: already in the ADE's tree, and time-sortable,
/// so the primary key IS the insertion order the turn tree wants.
macro_rules! opaque_id { ($name:ident) => { /* newtype + Display + FromStr + serde + schemars */ } }

opaque_id!(SessionId);  opaque_id!(TurnId);    opaque_id!(BranchId);
opaque_id!(CallId);     opaque_id!(RuleId);    opaque_id!(SurfaceId);
opaque_id!(PromptId);   opaque_id!(RunId);

/// Per-session monotonic ordering. NOT a uuid — clients detect gaps by arithmetic.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct Seq(pub u64);

/// Extension namespace: "buildgraph", "mcp.jira", "builtin".
pub struct ExtId(String);   // validated: [a-z0-9-]+ ('.' allowed only for the mcp. prefix)
```

Every id serialises as a plain string. `SessionRef { session, branch, turn: Option<TurnId> }` lives in `ids.rs` too.

### Grant and capability

```rust
#[non_exhaustive]
#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Aspect {
    Tool, Mcp, Skill, Ext, Mode,
    Read, Write, Spawn, Net, Creds, Ui, Render,
    #[serde(rename = "mem.read")]  MemRead,
    #[serde(rename = "mem.write")] MemWrite,
}

pub struct Capability { pub aspect: Aspect, pub scope: Vec<String> }

#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Consent { Always, Once, Never }

pub struct Grant { pub capabilities: Vec<Capability>, pub consent: Consent }

impl Grant {
    /// The ONLY operation ever applied to a Grant (§4.15). A child's grant is never
    /// a superset of its parent's. Consent intersects as Never < Once < Always.
    pub fn intersect(&self, other: &Grant) -> Grant;
}

/// A declaration that may narrow but is allowed to omit fields (was `Partial<Grant>`).
#[derive(Default)]
pub struct GrantSpec { pub capabilities: Option<Vec<Capability>>, pub consent: Option<Consent> }
impl GrantSpec { pub fn apply_to(&self, parent: &Grant) -> Grant; }
```

`intersect` on `scope` is per-aspect string-set intersection here — **not** glob-aware. Glob semantics belong to `orrery-policy`'s matcher (plan 07); this crate must stay dependency-free. A doc comment says so, because the difference matters: `read(./src/**)` ∩ `read(./**)` is `./src/**` under glob semantics and `{}` under set semantics. **Therefore `Grant::intersect` is only correct for the `consent` field and for exact-match aspects.** See "Open questions".

### Budget and usage

```rust
pub struct Budget {
    pub max_turns: u32,
    pub max_tokens: u64,
    pub wall_clock_ms: u64,
    pub max_micro_usd: Option<u64>,      // translation #7
}

#[non_exhaustive]
pub enum BudgetKind { Turns, Tokens, WallClock, Usd }

pub struct Usage {
    pub input_tokens: u64, pub output_tokens: u64,
    pub cache_hits: u64,   pub micro_usd: Option<u64>,
}
impl std::ops::AddAssign for Usage { /* accumulate across passes */ }

pub struct TokenBudget { pub max: u64, pub reserve: u64 }   // §4.3 recall clamp, §4.2 materialise
```

### Scope, role, layer, subject

```rust
#[non_exhaustive] #[serde(rename_all = "kebab-case")]
pub enum Role { Planner, Executor, Verifier, Compactor, Summariser, Router, Grader }

#[non_exhaustive] #[serde(rename_all = "kebab-case")]
pub enum Layer { Managed, Org, User, Workspace, Project }
// Names resolve closest-layer-first; a managed deny is final. Opposite directions,
// on purpose (§4.4 vs §4.8). The doc comment must say this.

/// Translation #3. Serialises to/from the string forms "agent", "ext:<id>", "agent:<name>".
#[non_exhaustive]
pub enum Subject { Agent, Ext(ExtId), SubAgent(String) }

pub struct AgentScope {
    pub agent: String,
    pub branch: BranchId,
    pub tools: Vec<String>,     // the visible set, not a suggestion
    pub grant: Grant,
}
```

`Layer` derives `PartialOrd`/`Ord` with `Managed` **lowest** so `max()` is "closest layer wins"; the ordering is documented as one-directional and a test pins it.

### Messages

The provider-neutral conversation shape. Deliberately small — provider-specific fields are built in the provider crate, not carried here.

```rust
pub struct Message { pub role: MessageRole, pub content: Vec<ContentBlock> }

#[non_exhaustive] #[serde(rename_all = "kebab-case")]
pub enum MessageRole { System, User, Assistant }

#[non_exhaustive] #[serde(tag = "t", rename_all = "kebab-case")]
pub enum ContentBlock {
    Text      { text: String },
    Image     { media_type: String, data: String },     // base64; providers without `images` reject
    ToolUse   { call: CallId, name: String, input: serde_json::Value },
    ToolResult{ call: CallId, outcome: Outcome },
    Thinking  { text: String },
}
```

`ContentBlock::ToolResult` carries `Outcome`, the same type the `tool.settled` frame carries — one shape for "what happened", whether it is being shown to a user or fed back to a model. That is what keeps a denial identical in the transcript and on the wire.

### Surfaces

A direct transcription of §6.2. All ten core variants plus `custom`.

```rust
#[non_exhaustive] #[serde(tag = "t", rename_all = "kebab-case")]
pub enum SurfaceKind {
    Text      { value: String, style: Option<TextStyle> },
    Table     { columns: Vec<String>, rows: Vec<Vec<Cell>> },
    Tree      { nodes: Vec<TreeNode> },
    Diff      { path: String, hunks: Vec<Hunk> },
    Progress  { label: String, done: Option<u64>, total: Option<u64> },
    Stream    { id: String },
    Task      { items: Vec<TaskItem> },
    Question  { prompt: String, choices: Vec<Choice>, multi: bool, free: bool,
                default: Option<String>, deadline_ms: Option<u64> },
    Form      { fields: Vec<Field>, submit: String },
    Stack     { dir: StackDir, title: Option<String>, collapsed: bool, children: Vec<Surface> },
    Markdown  { value: String, complete: bool },
    Custom    { kind: String, payload: serde_json::Value, fallback: Box<Surface> },
}

pub struct Surface { pub id: Option<SurfaceId>, pub status: Option<Status>, pub kind: SurfaceKind }

#[non_exhaustive] #[serde(tag = "op", rename_all = "kebab-case")]
pub enum SurfacePatch {
    Replace { id: SurfaceId, value: Surface },
    Append  { id: SurfaceId, text: String },          // streams, hot path
    Set     { id: SurfaceId, path: Vec<String>, value: serde_json::Value },
    Remove  { id: SurfaceId },
}
```

`Custom.fallback` is `Box<Surface>` and **not** `Option` — §6.2 makes it mandatory, so the type enforces it. `Custom.kind` is validated as namespaced (`<ext>.<name>`) by a `validate()` method this crate provides; plan 09 calls it.

### Frames

```rust
#[non_exhaustive] #[serde(tag = "t")]
pub enum Request {
    #[serde(rename = "session.create")] SessionCreate { id: ReqId, profile: String, workspace: String },
    #[serde(rename = "session.attach")] SessionAttach { id: ReqId, session: SessionId, since: Option<Seq> },
    #[serde(rename = "turn.submit")]    TurnSubmit    { id: ReqId, session: SessionId, input: UserInput },
    #[serde(rename = "turn.cancel")]    TurnCancel    { id: ReqId, session: SessionId, turn: TurnId },
    #[serde(rename = "intent")]         Intent        { id: ReqId, session: SessionId, surface: SurfaceId, value: serde_json::Value },
    #[serde(rename = "consent.answer")] ConsentAnswer { id: ReqId, prompt: PromptId, answer: ConsentAnswerKind },
    #[serde(rename = "command")]        Command       { id: ReqId, session: SessionId, name: String, args: Option<serde_json::Value> },
    #[serde(rename = "query")]          Query         { id: ReqId, of: QueryOf },
}

#[non_exhaustive] #[serde(tag = "t")]
pub enum Event {
    #[serde(rename = "turn.started")]     TurnStarted    { seq: Seq, turn: TurnId },
    #[serde(rename = "delta")]            Delta          { seq: Seq, surface: SurfaceId, patch: SurfacePatch },
    #[serde(rename = "tool.started")]     ToolStarted    { seq: Seq, call: CallId, r#ref: ToolRef },
    #[serde(rename = "tool.settled")]     ToolSettled    { seq: Seq, call: CallId, outcome: Outcome },
    #[serde(rename = "consent.request")]  ConsentRequest { seq: Seq, prompt: ConsentPrompt, deadline_ms: u64 },
    #[serde(rename = "turn.settled")]     TurnSettled    { seq: Seq, turn: TurnId, usage: Usage },
    #[serde(rename = "error")]            Error          { seq: Seq, scope: ErrorScope, detail: ErrorDetail },
}
```

Note `Request` carries an explicit `id: ReqId` on every variant — §5.2 says "every request carries an id" but the TS sketch omits it from the shapes. A struct-level id cannot be expressed on a tagged enum in serde, so it is repeated. A test asserts every variant has one.

```rust
pub struct ToolRef { pub ext: ExtId, pub name: String }   // Display = "ext.name"; FromStr splits on the LAST '.'
```

`FromStr` splitting on the **last** dot is what makes `mcp.jira.create_issue` parse as `ExtId("mcp.jira") + "create_issue"` (§4.4). A test covers both `ripgrep.search` and the MCP three-segment form.

```rust
#[non_exhaustive] #[serde(tag = "t", rename_all = "kebab-case")]
pub enum Outcome {
    Ok        { surface: Option<Surface>, value: Option<serde_json::Value> },
    Denied    { rule: RuleId, reason: String },
    Truncated { surface: Option<Surface>, bytes_emitted: u64, limit: u64 },
    Cancelled { reason: CancelReason },
    Unloaded  { ext: ExtId },
    Failed    { code: String, message: String },
}
```

This is the type that makes "a denial is a value" real across every layer: the tool registry returns it, the transcript stores it, the wire carries it, the renderer draws it.

### Verdict

The type half of translation #2. The `Phase` trait itself is plan 05; the payload-generic verdict lives here so `orrery-ext-api` can name it without depending on the kernel.

```rust
#[non_exhaustive]
pub enum Verdict<P> {
    Continue,
    Rewrite(P),                       // typed: a tool input cannot become a model request
    Deny    { reason: String },
    Handled { result: Outcome },      // from data the interceptor already holds
}
```

### Expr and predicate

```rust
#[non_exhaustive] #[serde(untagged)]
pub enum Expr { Literal(serde_json::Value), Ref { r#ref: String, path: Option<Vec<String>> } }

#[non_exhaustive] #[serde(rename_all = "kebab-case")]
pub enum Predicate {
    Cmp { lhs: Expr, op: CmpOp, rhs: Expr },
    All(Vec<Predicate>), Any(Vec<Predicate>), Not(Box<Predicate>),
}
```

Deliberately not a language: a step input is a literal or a reference to an earlier step's typed return, and a predicate compares declared values. Neither can call out. Evaluation and the load-time typecheck are plan 11.

---

## File structure

**Create**

- `harness/core/crates/orrery-proto/Cargo.toml` — `publish = true`, `#![deny(missing_docs)]`
- `harness/core/crates/orrery-proto/src/{lib,ids,grant,budget,scope,message,surface,frame,expr,load,tool,verdict}.rs`
- `harness/core/crates/orrery-proto/tests/round_trip.rs`
- `harness/core/crates/orrery-proto/tests/fixtures/*.json` — one per frame variant, one per surface variant
- `harness/xtask/src/typegen.rs`
- `harness/protocol/package.json`, `harness/protocol/README.md`

**Generated, committed**

- `harness/protocol/protocol.schema.json`
- `harness/protocol/protocol.d.ts`

---

## Tasks

### Task 1 · Ids

Files: `src/ids.rs`, `tests/round_trip.rs`

- [x] **Failing test first.** `ids::round_trips_as_plain_string` — a `TurnId` serialises to a bare JSON string, parses back equal, and `FromStr` rejects `"not-a-uuid"`. `ids::seq_is_ordered` — `Seq(1) < Seq(2)`.
- [x] Write the `opaque_id!` macro: newtype, `new()` (v7), `Display`, `FromStr`, `Serialize`/`Deserialize` as string, `JsonSchema` as `{"type":"string","format":"uuid"}`, `Copy` where the payload is `Uuid`.
- [x] Add `ExtId` with validation: lowercase alphanumeric and `-`; `.` only as the `mcp.` prefix. Test both accepted and rejected forms.
- [x] `SessionRef`.

### Task 2 · Grant, capability, budget

Files: `src/grant.rs`, `src/budget.rs`, `tests/grant_props.rs`

- [x] **Failing test first.** `grant::intersect_never_widens` (proptest): for arbitrary `a`, `b`, every capability in `a.intersect(&b)` is present in both, and `consent` is the minimum. `grant::intersect_is_commutative` and `..._idempotent`.
- [x] Implement `Aspect` with its serde renames (`mem.read`, `mem.write` are not kebab-case of the variant name — pin them in a test).
- [x] Implement `Grant::intersect`, `GrantSpec::apply_to`.
- [x] Document loudly that scope intersection here is set-based and the glob-aware version lives in `orrery-policy`.
- [x] `Budget`, `BudgetKind`, `Usage` + `AddAssign`, `TokenBudget`. Test that `Usage` accumulation is saturating, not wrapping.

### Task 3 · Scope, role, layer, subject

Files: `src/scope.rs`

- [x] **Failing test first.** `scope::subject_string_forms` — `Subject::Ext(ExtId("buildgraph"))` ⇄ `"ext:buildgraph"`, `Subject::Agent` ⇄ `"agent"`, `Subject::SubAgent("critic")` ⇄ `"agent:critic"`. `scope::layer_ordering` — `Layer::Project > Layer::Managed`.
- [x] Implement, with the one-directional ordering note in the doc comment.

### Task 4 · Messages and outcome

Files: `src/message.rs`, `src/frame.rs` (Outcome only)

- [x] **Failing test first.** `message::tool_result_carries_denial` — a `ContentBlock::ToolResult` holding `Outcome::Denied` round-trips and the rule id survives.
- [x] Implement `Message`, `MessageRole`, `ContentBlock`, `Outcome`, `CancelReason`.

### Task 5 · Surfaces

Files: `src/surface.rs`, `tests/fixtures/surface-*.json`

- [x] **Failing test first.** `surface::every_variant_round_trips` — a table-driven test over one fixture per variant; failing until each is implemented. `surface::custom_requires_fallback` — a `custom` payload with no `fallback` fails to deserialize.
- [x] Implement `Surface`, `SurfaceKind` (all twelve), `SurfacePatch`, and the leaf types (`Status`, `TextStyle`, `Cell`, `TreeNode`, `Hunk`, `TaskItem`, `Choice`, `Field`, `StackDir`).
- [x] `SurfaceKind::validate(&self) -> Result<(), SurfaceError>` — `custom.kind` is namespaced; `question` with `deadline_ms` and no `default` is an error only when unattended, so it is **not** checked here (plan 09 owns that rule; note it).

### Task 6 · Frames

Files: `src/frame.rs`, `tests/fixtures/frame-*.json`

- [x] **Failing test first.** `frame::tag_names_are_dotted` — `Request::TurnSubmit` serialises with `"t":"turn.submit"`, not `"turn-submit"`. One assertion per variant; this is the test that catches a missing explicit `rename`.
- [x] `frame::every_request_has_an_id` — reflect over the fixtures, assert an `id` field.
- [x] Implement `Request`, `Event`, `UserInput`, `ConsentPrompt`, `ConsentAnswerKind`, `QueryOf`, `ErrorScope`, `ErrorDetail`, `ToolRef`.
- [x] `ToolRef::from_str` splits on the **last** dot; test `ripgrep.search` and `mcp.jira.create_issue`.

### Task 7 · CBOR

Files: `tests/round_trip.rs`

- [x] **Failing test first.** `cbor::frames_round_trip` — every fixture, JSON → value → CBOR bytes → value → JSON, equal. This is the test that fails loudly if anyone adds a newtype variant to a tagged enum.
- [x] Add `ciborium` as a dev-dependency only. The crate itself stays format-agnostic.

### Task 8 · Verdict and Expr

Files: `src/verdict.rs`, `src/expr.rs`

- [x] **Failing test first.** `expr::ref_and_literal_are_distinguishable` — `{"ref":"step1"}` parses as `Expr::Ref`, `{"a":1}` as `Expr::Literal`.
- [x] Implement both. `Verdict<P>` needs no serde (it never crosses the wire) — derive only `Debug`.

### Task 9 · typegen

Files: `harness/xtask/src/typegen.rs`, `harness/protocol/*`

- [x] **Failing test first.** `xtask::typegen_is_committed` — run the generator into a temp dir, compare byte-for-byte with the committed files, fail on difference. This is the CI drift gate; it fails until task 9 lands.
- [x] Implement: `schemars::schema_for!` over a root type that references `Request`, `Event`, `Surface`, `SurfacePatch`, `Grant`, `Budget`, `Usage`, `Message`, `LoadOutcome`; write `protocol.schema.json`.
- [x] Shell out to `json-schema-to-typescript` via pnpm; write `protocol.d.ts`. Deterministic ordering — sort definitions, or the drift gate flaps.
- [x] `harness/protocol/package.json`: name `@orrery/protocol`, private, `types: protocol.d.ts`.

### Task 10 · `LoadOutcome`

Files: `src/load.rs`

- [x] **Failing test first.** `load::outcome_variants` — `{"status":"degraded", …}` parses, and `skipped` requires a `reason` from the closed set.
- [x] Implement `LoadOutcome`, `Contribution`, `LoadStage`, `SkipReason`. `Contribution.kind` is a plain enum here; the derive macro that keeps it in step with the manifest is plan 06 (translation #10).

---

### Task 11 · `ToolDescriptor`

Files: `src/tool.rs`, `tests/tool.rs`, `harness/xtask/src/typegen.rs`

Added after the wave-1 audit: plan 03 and plan 04 each declared their own
`ToolDescriptor`, with no conversion between them. It is a wire type — the
registry builds it, the provider serialises it, the model is shown it — so it
belongs here and nowhere else.

- [x] **Failing test first.** `tool::round_trips_as_json`, `tool::round_trips_over_cbor`,
  `tool::has_a_schema` — all four fields, `atomic` included, survive JSON and CBOR and
  appear in the schemars output. Plus `descriptor::is_protos_type` in *both*
  `orrery-provider` and `orrery-tools`: an assignment that only compiles if the
  re-exported name is this type.
- [x] Implement `ToolDescriptor { name, description, input_schema, atomic }` with the
  schemars derive; delete `orrery-provider/src/request.rs`'s copy and
  `orrery-tools/src/descriptor.rs`, and re-export from both.
- [x] Add it to the typegen root so it lands in `protocol.schema.json` / `protocol.d.ts`.

---

## Done when

- `cargo test -p orrery-proto` is green, including the proptests and the CBOR round-trip.
- `cargo xtask typegen` produces no diff.
- No dependency on tokio, reqwest, or any I/O crate: `cargo tree -p orrery-proto` fits on a screen.
- `cargo publish --dry-run -p orrery-proto` packages cleanly.

## State

Tasks 1-10 landed with phase 1 and are green as of 2026-09-18; task 11
(`ToolDescriptor` consolidation) landed 2026-09-18 in the wave-1 audit cleanup.
`cargo test -p orrery-proto` passes, `cargo test -p xtask` holds the typegen
drift gate, and `harness/protocol/protocol.{schema.json,d.ts}` are committed in
step with the derives. Three decisions are recorded under "Open questions":
`Grant::intersect` became `Grant::intersect_exact` with the narrowing operation
moved to `orrery-policy` (plan 07), `ContentBlock::Thinking` stays, and
`ContentBlock::Image` ships base64 with the blob-store variant deferred to
phase 4.

## Open questions

Answer these here when you decide them.

1. **Glob-aware grant intersection.** `Grant::intersect` as specified is set-based and therefore wrong for path aspects. Three options: (a) leave it, document it as exact-match only, and have `orrery-policy` own the real narrowing — simplest, but a footgun with a correct-looking name; (b) move `intersect` out of this crate entirely into `orrery-policy` and have `orrery-proto` carry only the data; (c) pull `globset` in here. **(b) is the recommendation** — the type stays here, the operation moves — but it changes §4.15's shape, so record the decision.

   **Decided: (b), with the footgun disarmed by name.** `orrery-proto` has no
   method called `intersect`. The glob-aware narrowing the kernel enforces is
   `orrery-policy`'s and lands with plan 07. What this crate ships is:

   - `Consent::min` — the consent lattice (`Never < Once < Always`) is
     format-independent and therefore correct here. `Consent` derives its `Ord`
     from an explicit `rank()`, because the declaration order is widest-first
     and the lattice order is the opposite.
   - `Grant::intersect_exact` — string-set intersection, named for what it does.
     Correct for `consent` and for the exact-match aspects; a doc comment spells
     out that `read(./src/**) ∩ read(./**)` is empty here and `./src/**` under
     policy semantics, and `Aspect::is_exact_match()` tells a caller which
     aspects it may be used on.
   - `GrantSpec::apply_to` resolves a declaration against its parent with the
     same operation: an omitted field inherits, a present field is *intersected*
     rather than substituted, so a spec can never widen what it was given.

   One semantic addition the plan did not specify and the property tests forced:
   an **empty `scope` means unqualified** — the whole aspect, not nothing — so
   an unscoped parent is wider than a scoped child. Without that rule,
   intersecting an unscoped `write` with `write(./src/**)` drops the capability
   and narrowing is not a lattice. Two non-empty scopes that do not overlap drop
   the capability rather than being promoted to unqualified.

   §4.15 changes accordingly: the `Grant` *type* is `orrery-proto`'s, the
   narrowing *operation* is `orrery-policy`'s.

2. **`Thinking` content blocks.** Not in the spec. They exist in every current provider and the transcript should keep them. Keep, or drop until a provider needs them?

   **Decided: keep.** Every current provider emits reasoning, the transcript is
   the audit record, and a turn tree that silently discards what the model was
   thinking cannot be replayed faithfully. It costs one variant. A provider that
   does not produce them simply never constructs one.

3. **Image content.** Base64 in the message is simple and matches the providers, but it means a screenshot sits in the turn tree and in every `materialise`. Reference-with-blob-store (the ADE's `history/mod.rs` shape) is the alternative. Phase 1 does not need images; decide before phase 4.

   **Deferred, as the plan allows, but the shape is now pinned:**
   `ContentBlock::Image { media_type, data }` ships with base64 `data`, because
   phase 1 never constructs one and a type nobody builds costs nothing. The
   blob-store variant, when it is needed, is an *additional* variant on a
   `#[non_exhaustive]` enum rather than a change to this one — so taking the
   decision in phase 4 is not a breaking change. Revisit before phase 4.
