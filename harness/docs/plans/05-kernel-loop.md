# 05 · Kernel — the turn loop, the phases, the facade

**Goal.** One loop: build context, call the provider, receive tool calls, dispatch them, append results, repeat until stop. Ten phases, typed interceptors that cannot do I/O, budgets the kernel enforces rather than trusts, one cancellation tree, and a facade that assembles the whole thing from a config. When this is done, `orrery run -p "…"` completes a real turn with a real tool call.

**Covers.** §4.1 (kernel, phases, interceptors) · §4.6 (the loop shape; routing and roles are plan 11) · §4.3's `context.build` assembly order.

**Crates.** `core/crates/orrery-kernel` · `core/crates/orrery-harness` (the facade).

**Depends on.** [`01`](01-proto-shared-types.md), [`02`](02-session-store.md), [`03`](03-provider-layer.md), [`04`](04-tool-registry.md), [`06`](06-extension-host.md). Plans 07 (policy), 11 (router), 12 (memory) plug into holes this plan leaves with typed no-ops.

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **Interceptors are sync and hold no broker handle** (translation #1). I/O is impossible by type.
- **`Verdict<P>` is generic over the phase payload** (translation #2). `Phase` is a trait with an associated type, not a string enum.
- `compact` is the one phase allowed I/O (translation #8) — named exception, async, cancellable, charged to the turn.
- Budget stop and cancellation are `TurnOutcome` values, never `Err`.
- One `CancellationToken` per turn, `child_token()` per pass and per call.
- The kernel is **not generic** over `Provider` or `SessionStore`.
- Retry is the kernel's: bounded backoff, charged to the turn budget, audited. Providers never sleep.

This plan owns translations **#1**, **#2** and **#8**.

---

## Architecture

### Phases

§4.1's naming rule: a **gate** (`.before`, `.after`, `.resolve`) may return a verdict that changes the outcome; a **span** (`.start`, `.end`) only brackets. A phase exists only where a verdict is possible — everything else is an event on the stream.

```rust
pub trait Phase: 'static {
    /// What an interceptor at this phase may rewrite.
    type Payload;
    const NAME: &'static str;
    const SCOPE: PhaseScope;      // Session | Turn | Pass
}

pub struct SessionStart;  impl Phase for SessionStart  { type Payload = ResolvedManifest; .. }
pub struct TurnStart;     impl Phase for TurnStart     { type Payload = TurnInput;        .. }
pub struct TurnEnd;       impl Phase for TurnEnd       { type Payload = TurnSummary;      .. }
pub struct ContextBuild;  impl Phase for ContextBuild  { type Payload = ContextDraft;     .. }
pub struct ContextCompact;impl Phase for ContextCompact{ type Payload = CompactPlan;      .. }
pub struct ProviderBefore;impl Phase for ProviderBefore{ type Payload = ModelRequest;     .. }
pub struct ProviderAfter; impl Phase for ProviderAfter { type Payload = PassResult;       .. }
pub struct ToolResolve;   impl Phase for ToolResolve   { type Payload = PendingCall;      .. }
pub struct ToolBefore;    impl Phase for ToolBefore    { type Payload = ToolInput;        .. }
pub struct ToolAfter;     impl Phase for ToolAfter     { type Payload = Outcome;          .. }
```

Ten phases, not thirty. The pass itself is deliberately unnamed — it is a unit of accounting, so every event carries a `pass_id`, but no decision sits at a pass boundary that `context.build` and `provider.after` do not already cover.

### Interceptors

```rust
/// SYNC. No broker, no token, no I/O — translation #1.
pub trait Interceptor<P: Phase>: Send + Sync {
    fn matches(&self, m: &MatchCtx) -> bool { true }        // tool / agent selector
    fn run(&self, ctx: &InterceptCtx<'_>, payload: &P::Payload) -> Verdict<P::Payload>;
}

/// Deliberately anaemic. If it is not here, an interceptor cannot reach it.
pub struct InterceptCtx<'a> {
    pub session: SessionId, pub turn: TurnId, pub pass: PassId,
    pub agent: &'a AgentScope,
    pub budget_spent: &'a Usage,
}
```

Rules the chain enforces, each with a test:

- A `Deny` verdict **narrows only**: it can refuse a call policy would have allowed, never allow one policy refused. So the chain runs `tool.before` interceptors *before* the policy check and re-checks nothing after.
- A `Handled` verdict must come from data the interceptor already holds. There is no way to violate this because there is no I/O handle — that is the whole reason the trait is sync.
- Every `Deny` and `Handled` lands in the audit stream.
- Interceptor order is deterministic: registration order within a layer, layers closest-first.

Registration is type-erased behind a per-phase vector so the kernel holds one `InterceptorSet`:

```rust
pub struct InterceptorSet { /* HashMap<&'static str, Vec<Box<dyn AnyInterceptor>>> */ }
impl InterceptorSet {
    pub fn run<P: Phase>(&self, ctx: &InterceptCtx, payload: P::Payload)
        -> (P::Payload, ChainOutcome);
}
```

### Lifecycle handlers

The other half of §4.1, and where memory writes (§4.3):

```rust
#[async_trait]
pub trait LifecycleHandler: Send + Sync {
    fn at(&self) -> LifecyclePoint;
    /// MAY do I/O through the broker. Returns nothing, so it cannot alter the turn it fires on.
    async fn run(&self, ctx: &LifecycleCtx) -> Result<(), LifecycleError>;
}

pub enum LifecyclePoint { SessionStart, SessionEnd, TurnEnd, BranchClose, WorkflowEnd }
```

`session.start` appears in both lists on purpose: an interceptor there returns a verdict on the resolved manifest, a lifecycle handler there does the I/O of opening a store.

A failing lifecycle handler is logged and disabled for the session; it never fails the turn.

### The turn

```rust
pub struct Kernel { /* store, provider, registry, interceptors, lifecycle, router, memory, audit, config */ }

impl Kernel {
    pub async fn run_turn(&self, lease: BranchLease, input: TurnInput, cancel: CancellationToken)
        -> Result<TurnOutcome, KernelError>;
}
```

One pass, in order:

1. `turn.start` — verdict on `TurnInput`.
2. **`context.build`** — assemble; see below. If it does not fit `capabilities.max_context`, run `context.compact` and rebuild. A bounded number of compaction attempts (2), then `StoppedByBudget(Tokens)`.
3. `provider.before` — verdict on `ModelRequest`. **Auth is checked here**: `AuthState::NeedsLogin | Expired` ⇒ return `TurnOutcome::NeedsLogin`, never a stall mid-pass.
4. Stream. Accumulate text into a markdown surface and tool calls via `ToolCallAccumulator`. Retry on `is_retryable()` with bounded backoff, each attempt charged and audited.
5. `provider.after` — verdict on `PassResult`.
6. Text only ⇒ `turn.end`. Tool calls ⇒ for each: `tool.resolve` → `tool.before` → **policy check** → dispatch → `tool.after` → append.
7. Append the pass to the tree **before** the next begins, so an interrupted loop resumes instead of restarting.
8. Loop, until the provider asks for no more tools or a ceiling stops it.

Two exits, and the second belongs to the kernel: text-only, or a ceiling (turns, tokens, wall clock, money) reached.

### Context assembly — the order is fixed and the reason is cost

```rust
pub struct ContextDraft {
    pub system: Vec<Section>,     // stable prefix, in this order:
    //   1. base system prompt
    //   2. agent prompt (role binding)
    //   3. tool descriptors (registry.visible(scope), indexmap-stable order)
    //   4. skills
    pub cache_breakpoint: usize,  // ← everything above this is the cached prefix
    pub recalled: Vec<Message>,   // volatile suffix begins here
    pub history: Vec<Message>,
    pub input: Message,
}
```

Stable prefix first, then the volatile suffix: recalled memory, history, current input. A memory provider injecting near the top would invalidate the cache every pass. The `cache_breakpoint` handed to the provider is computed here and nowhere else.

Nothing reaches the model without passing `context.build` — the visible tool set, the active skills and anything recalled are settled here rather than at the provider.

Interceptors at this phase may **rewrite only**. A `Deny` at `context.build` is meaningless and is rejected at registration.

### Budgets and accounting

```rust
pub struct TurnBudget { spent: Usage, turns: u32, started: Instant, limit: Budget }
impl TurnBudget {
    fn check(&self) -> Option<BudgetKind>;     // called before each pass and before each tool dispatch
    fn charge(&mut self, u: &Usage);
}
```

Money is `micro_usd`, computed from `Usage` and a per-model price table in config (plan 10). Until then a `PriceTable::empty()` that leaves `micro_usd` as `None` and makes `maxUsd` inert — with a `TODO(plan-10)`.

### The facade

```rust
// orrery-harness
pub struct Harness { runtime: tokio::runtime::Runtime, kernel: Arc<Kernel> }

impl Harness {
    pub fn build(config: ResolvedConfig) -> Result<Self, BuildError>;
    pub fn handle(&self) -> tokio::runtime::Handle;
    pub fn block_on<F: Future>(&self, f: F) -> F::Output;   // the bridge for sync embedders (the ADE)
    pub fn kernel(&self) -> Arc<Kernel>;
}
```

Cargo features select the first-party extension set: `default = ["builtin-tools", "anthropic", "sqlite", "fixture-provider"]`. This is the **only** core crate that names an `extensions/` crate; `cargo xtask deps-check` allow-lists it explicitly.

---

## File structure

**Create**

- `harness/core/crates/orrery-kernel/src/{lib,phase,intercept,lifecycle,turn,context,budget,retry,error}.rs`
- `harness/core/crates/orrery-kernel/tests/{turn,intercept,budget,cancel}.rs`
- `harness/core/crates/orrery-harness/src/{lib,build,features}.rs`
- `harness/core/crates/orrery-harness/tests/end_to_end.rs`

---

## Tasks

### Task 1 · Phases and the verdict chain

Files: `src/phase.rs`, `src/intercept.rs`, `tests/intercept.rs`

- [ ] **Failing test first.** `intercept::rewrite_is_typed` — a compile-fail test (`trybuild`) proving an interceptor registered for `ToolBefore` cannot return a `ModelRequest`. This is translation #2's whole value.
- [ ] `intercept::deny_narrows_only` — an interceptor returning `Deny` on a call policy allowed ⇒ denied; an interceptor returning `Continue` on a call policy denied ⇒ still denied.
- [ ] `intercept::order_is_deterministic` — three interceptors, registered in a known order, observed in that order, twice.
- [ ] `intercept::handled_short_circuits` — a `Handled` verdict means dispatch never runs.
- [ ] Implement `Phase`, the ten marker types, `Interceptor<P>`, `InterceptCtx`, `InterceptorSet`, `ChainOutcome`.
- [ ] Reject a `Deny` registration at `context.build` with a clear error.

### Task 2 · Lifecycle handlers

Files: `src/lifecycle.rs`

- [ ] **Failing test first.** `lifecycle::failure_does_not_fail_the_turn` — a handler that returns `Err` at `turn.end`; the turn still reports `Completed`, and the failure is in the audit.
- [ ] `lifecycle::returns_nothing` — a type-level assertion that `run` returns `()`; it cannot alter the turn.
- [ ] Implement.

### Task 3 · Context assembly

Files: `src/context.rs`, `tests/turn.rs`

- [ ] **Failing test first.** `context::stable_prefix_is_first` — with a memory provider contributing, the recalled block appears *after* the tool descriptors, and `cache_breakpoint` points past the descriptors.
- [ ] `context::tool_order_is_stable` — building twice with the same scope produces byte-identical prefixes. (This is the test that catches a `HashMap` sneaking in.)
- [ ] `context::no_tools_when_provider_lacks_them`.
- [ ] `context::overflow_triggers_compact_then_rebuild` — a tiny `max_context`; assert `compact` ran once and the second build fits.
- [ ] `context::compaction_gives_up` — after 2 attempts, `StoppedByBudget(Tokens)`, not an infinite loop.
- [ ] Implement `ContextDraft`, the assembly, the fit check, the compact trigger.

### Task 4 · The turn loop

Files: `src/turn.rs`, `tests/turn.rs`

- [ ] **Failing test first.** `turn::text_only_completes` — fixture provider with a text-only stream ⇒ `TurnOutcome::Completed` with usage, one assistant turn appended.
- [ ] `turn::tool_call_round_trips` — fixture stream with one tool call ⇒ dispatch happens, a `ToolResult` turn is appended, a second pass runs, the turn completes.
- [ ] `turn::denial_is_appended_not_raised` — a denied call produces a `ToolResult` turn holding `Outcome::Denied` and the loop continues.
- [ ] `turn::appends_before_next_pass` — kill the loop between passes and assert the tree holds the completed pass.
- [ ] Implement `run_turn`.

### Task 5 · Budgets

Files: `src/budget.rs`, `tests/budget.rs`

- [ ] **Failing test first.** `budget::max_turns_stops` — a fixture that always asks for a tool, `max_turns = 3` ⇒ `StoppedByBudget(Turns)` after exactly 3.
- [ ] `budget::wall_clock_stops`, `budget::tokens_stop`.
- [ ] `budget::stop_is_a_value` — a compile-time assertion that `StoppedByBudget` is a `TurnOutcome` variant, not a `KernelError` one.
- [ ] Implement `TurnBudget`, checks before each pass and each dispatch.

### Task 6 · Retry

Files: `src/retry.rs`

- [ ] **Failing test first.** `retry::retryable_is_retried_and_charged` — a fixture stream that fails twice with `RateLimited` then succeeds; assert 3 attempts, all audited, and the backoff time counted against wall clock.
- [ ] `retry::terminal_is_not_retried` — `Auth` fails immediately, one attempt.
- [ ] `retry::exhaustion_is_an_outcome` — after the cap, the turn ends with a typed outcome rather than an error.
- [ ] Implement bounded exponential backoff with jitter. Defaults hardcoded with `TODO(plan-10)`.

### Task 7 · Cancellation

Files: `tests/cancel.rs`

- [ ] **Failing test first.** `cancel::mid_stream` — cancel during the provider stream; assert `TurnOutcome::Cancelled(User)`, the partial assistant text is appended, and the stream is dropped.
- [ ] `cancel::mid_tool` — cancel during dispatch; assert `Outcome::Cancelled` on that call and that the tool's child token fired.
- [ ] `cancel::reason_is_recorded` — budget-triggered cancellation carries `CancelReason::Budget`.
- [ ] Implement the token tree.

### Task 8 · The facade

Files: `orrery-harness/src/*`, `tests/end_to_end.rs`

- [ ] **Failing test first.** `end_to_end::one_turn_with_a_tool_call` — build a `Harness` with the fixture provider, the sqlite store and the builtin tool bundle; submit a turn; assert the transcript holds user → assistant(tool_use) → tool_result → assistant(text), and that the tool actually touched the filesystem through the broker.
- [ ] Implement `Harness::build`, feature wiring, the runtime, `block_on`.
- [ ] Add the `deps-check` allow-list entry for this crate.

### Task 9 · Auth gating

Files: `src/turn.rs`

- [ ] **Failing test first.** `turn::needs_login_is_refused_early` — a provider reporting `NeedsLogin`; assert the turn returns `NeedsLogin` at `provider.before` **before** any context is built, and that nothing was appended.
- [ ] Implement.

---

## Done when

- `cargo test -p orrery-kernel` and `-p orrery-harness` green.
- `orrery run -p "list the files here"` (plan 17) completes a real turn with a real tool call against a real provider.
- Every budget kind demonstrably stops the loop.
- Cancellation leaves partial work in the tree.

## Open questions

1. **Where does the retry policy live?** Hardcoded now, profile config later (plan 10). Confirm the shape: per-provider, per-profile, or both?
2. **Compaction attempts = 2.** Arbitrary. Is one enough? Is three ever useful? Decide with a real over-long context.
3. **`PassId`** — is it worth its own opaque id, or is `(TurnId, u32)` enough? The audit wants to group by pass; a plain index is simpler and sorts naturally.
4. **Interceptor registration at `session.start`** must validate that every registered phase name exists. Do it with a `const NAME` lookup table, or make registration type-driven so an unknown phase cannot be named at all? The latter is better and probably free.
