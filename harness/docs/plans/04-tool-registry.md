# 04 · Tool registry — one namespace, one resolver, one dispatch path

**Goal.** Every tool from every source — builtin, extension, MCP, skill — held under a namespaced id, resolved by a documented precedence, shown to the model as a deliberate subset, and dispatched through exactly one function that cannot skip the policy check. When this is done, two extensions claiming `search` coexist and the model sees a tool list that is a real subset rather than a promise in a prompt.

**Covers.** §4.4 in full · the registration half of §4.11's MCP decision 1.

**Crates.** `core/crates/orrery-tools`.

**Depends on.** [`01`](01-proto-shared-types.md), [`06`](06-extension-host.md) (the `ToolHost` it dispatches to), [`07`](07-policy-broker-audit.md) (the check it cannot skip — stub with allow-all until then).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **Namespacing, precedence and the single dispatch path are ours.** Nothing reaches a tool around the registry — that is what makes the policy check unavoidable.
- `visible()` ordering is `indexmap`-stable. An unstable prompt prefix silently kills provider caching.
- Denial is a value: `dispatch` returns `Result<Outcome, ToolError>` with `Outcome::Denied` in the `Ok` arm.
- Ambiguity resolves **closest layer first** and is **logged, never fatal** — Pi's fatal duplicate-name exit is objective 2's named failure.
- Note the direction: for a **name** the closest layer wins; for a **permission** a managed deny is final (§4.8). Naming is a convenience, permission is a boundary. The doc comment must say this.

---

## Architecture

```rust
pub struct Registry { /* IndexMap<ToolRef, Entry>, alias table, layer index */ }

impl Registry {
    pub fn resolve(&self, call_name: &str, scope: &AgentScope) -> Resolution;
    pub async fn dispatch(&self, r: &ToolRef, input: serde_json::Value, ctx: CallCtx)
        -> Result<Outcome, ToolError>;
    pub fn visible(&self, scope: &AgentScope) -> Vec<ToolDescriptor>;
}

#[non_exhaustive]
pub enum Resolution {
    Ok        { r#ref: ToolRef },
    Ambiguous { candidates: Vec<ToolRef>, chose: ToolRef },   // resolved AND reported
    Unknown   { name: String, did_you_mean: Vec<String> },
}
```

`Ambiguous` carries what it chose, not just the candidates: the call proceeds, and the event stream records that a choice was made. Objective 2 is "a duplicate tool name must not exit 1".

### Names

- Full form is always `ext.name` — `ripgrep.search`, `mcp.jira.create_issue`, `builtin.read`.
- Short names are shown where unambiguous. Where two extensions provide `search`, the model sees `ripgrep.search` and `semantic.search`, never a bare `search`.
- `ToolRef::from_str` splits on the **last** dot (plan 01), which is what makes the three-segment MCP form work with no second scheme.
- An MCP server needs no special case: it registers as extension id `mcp.<server>` and inherits namespacing, precedence, policy and audit unchanged (§4.11 decision 1).

### Precedence

Closest layer first: project → workspace → user → organisation → managed. The winner takes the short name; every loser is recorded in the ledger with its layer.

### `visible(scope)`

This is what makes a sub-agent's tool list real. It intersects:

1. the tools that exist,
2. the scope's `tools` glob list,
3. what the policy engine would not categorically deny for this subject,
4. extensions currently `Live` (not `Draining`/`Dead`).

and returns them in a **stable order** — registration order within a layer, layers in precedence order. Never a `HashMap` iteration.

```rust
pub struct ToolDescriptor {
    pub name: String,              // the form the model should emit
    pub description: String,
    pub input_schema: serde_json::Value,   // JSON Schema, validated at the boundary
    pub atomic: bool,
}
```

### Budgets ride on every dispatch

```rust
pub struct ToolBudget {
    pub wall_clock_ms: u64,
    pub output_bytes: u64,     // applied WHILE reading, never after
    pub memory_bytes: Option<u64>,   // spawned processes only
}
```

Objective 5, concretely. The registry carries it; the broker enforces it (plan 07). The registry's job is to make it impossible to dispatch without one.

### Dispatch, in order

1. `tool.resolve` interceptors (plan 05).
2. Input validated against `input_schema` — a malformed call is `Outcome::Failed`, never a panic and never passed to the extension.
3. `tool.before` interceptors. A `Deny` here narrows.
4. **Policy check.** No path around this; `dispatch` is the only public entry.
5. Mint token, call the host with `CallCtx`.
6. `tool.after` interceptors.
7. Audit, return.

---

## File structure

**Create**

- `harness/core/crates/orrery-tools/src/{lib,registry,resolve,visible,dispatch,budget,descriptor,error}.rs`
- `harness/core/crates/orrery-tools/tests/{resolve,visible,dispatch}.rs`

---

## Tasks

### Task 1 · Registration and the name table

Files: `src/registry.rs`, `tests/resolve.rs`

- [x] **Failing test first.** `resolve::two_extensions_claiming_search` — register `ripgrep.search` and `semantic.search`; both resolve by full name; a bare `search` returns `Ambiguous` with both candidates and a chosen ref; **nothing errors**.
- [x] `resolve::closest_layer_wins` — the same tool at project and user layers; the project one is chosen and the user one is in the ledger.
- [x] `resolve::unknown_suggests` — `serch` returns `Unknown` with `search` in `did_you_mean`.
- [x] `resolve::mcp_three_segments` — `mcp.jira.create_issue` resolves to `ExtId("mcp.jira") + "create_issue"`.
- [x] Implement `Registry`, `Entry`, registration from `Contribution`s, the alias table.

### Task 2 · `visible`

Files: `src/visible.rs`, `tests/visible.rs`

- [x] **Failing test first.** `visible::order_is_stable` — build twice from the same registry, assert byte-identical descriptor lists. Run it 20 times to catch hash ordering.
- [x] `visible::scope_is_a_real_subset` — a scope with `tools = ["git.*"]` sees only git tools, and a dispatch of a non-visible tool is refused.
- [x] `visible::draining_extension_is_hidden`.
- [x] Implement, backed by `IndexMap`.

### Task 3 · Dispatch

Files: `src/dispatch.rs`, `tests/dispatch.rs`

- [x] **Failing test first.** `dispatch::is_the_only_path` — a compile-level check: the `ToolHost` field is private and no public method returns it. Pair with a doc test showing the intended call.
- [x] `dispatch::validates_input` — a call whose input violates `input_schema` returns `Outcome::Failed` and the host is never invoked.
- [x] `dispatch::denial_is_ok_arm` — a denied call returns `Ok(Outcome::Denied)`, and a test asserts `ToolError` has no `Denied` variant.
- [x] `dispatch::budget_is_always_present` — constructing a dispatch without a `ToolBudget` does not compile.
- [x] Implement the seven-step order above.

### Task 4 · Budgets

Files: `src/budget.rs`

- [x] **Failing test first.** `budget::derives_from_scope_and_manifest` — the effective budget is the minimum of the profile's, the agent's and the tool's declared ceiling.
- [x] Implement.

### Task 5 · Ledger integration

Files: `src/registry.rs`

- [x] **Failing test first.** `resolve::ambiguity_is_recorded` — after an ambiguous resolution, the ledger holds an entry naming both candidates and the winner.
- [x] Wire to `orrery-audit`.

---

## Stubs standing in for plans not yet written

The seams are traits in `src/dispatch.rs`, with the dispatch path already
routed through every one of them, so each plan substitutes an implementation
rather than reshaping this crate:

- `ToolHost` — TODO(plan-06). Stub `UnavailableHost` answers `Outcome::Unloaded`.
- `PolicyCheck` — TODO(plan-07). Stub `AllowAll` allows everything; step 4 of
  dispatch calls it unconditionally.
- `ToolInterceptor` — TODO(plan-05). No interceptors registered by default;
  steps 1, 3 and 6 run the (empty) chain.
- `ExtState` — TODO(plan-06). The registry keeps its own copy so `visible` can
  hide a draining extension without a call across crates.
- The ledger is in-crate (`Registry::ledger`), not `orrery-audit`: that crate is
  an empty stub and is not in the workspace's dependency table, which this plan
  may not edit. Step 7 emits a `tracing` event in the meantime.

## Done when

- `cargo test -p orrery-tools` green.
- Two extensions claiming `search` both dispatch; the ledger explains the short-name winner.
- `visible()` is provably stable across runs.
- There is no public way to reach a `ToolHost` except `dispatch`.

## Open questions

1. **`did_you_mean` cost.** Levenshtein over every registered name on every unknown call is fine at 50 tools and silly at 5000. Cap it, or precompute a trigram index? Cap for now.

   **Decided: cap, no index.** `resolve.rs` scans at most `MAX_SUGGESTION_SCAN`
   = 512 entries, keeps distances <= 2 and offers at most 3 names. A trigram
   index is a second structure to keep in step with the name table, for a
   suggestion nobody is entitled to; past the cap the list is merely shorter,
   never wrong.
2. **Schema validation placement.** Validating in the registry means `jsonschema` is a dependency of a hot crate. Alternative: validate in the host, once per runtime. Registry is better for uniformity — confirm the feature-gate keeps it out of lean builds.

   **Decided: registry, behind the default-on `schema-validation` feature.**
   `jsonschema` is `optional = true`, and `cargo check -p orrery-tools
   --no-default-features` builds without it — step 2 of dispatch becomes a
   no-op and every other step is unchanged. Uniformity where it is wanted, and
   a lean build that genuinely drops the dependency.

   Still open for plan 06: the validator is compiled per call. A per-entry
   cached `jsonschema::Validator` is the obvious next move, once the host
   exists and there is a real call rate to measure.
3. **Short-name aliases in config.** §7 says "short-name aliases" are the user's. That is a config feature (plan 10) that writes into this table. Confirm the table has a place for user-declared aliases now, so plan 10 does not need to change this crate.

   **Confirmed: `Registry::alias(name, ref)` exists and is checked first** —
   before the fully-qualified parse and before the generated short-name table —
   so a user can settle an ambiguity permanently. Plan 10 calls it and changes
   nothing here. Covered by `resolve::user_alias_resolves`.
