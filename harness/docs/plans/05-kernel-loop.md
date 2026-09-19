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

Money is `micro_usd`, computed from `Usage` and a per-model price table in
config. **Landed (round 5).** `orrery_harness::price_table` reads
`[prices.<model>] inputPerMillion / outputPerMillion` off the layers in force
and hands it over as `KernelConfig::prices`; `orrery-cli/tests/budget.rs`
asserts a ceiling stopping a real turn **through the binary**, exit 3.

A model nobody priced keeps `PriceTable::empty()`'s behaviour: `micro_usd`
stays `None` and `maxUsd` is inert rather than free. That is the documented
state, not a leftover — the same test file asserts it.

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
- `harness/core/crates/orrery-harness/src/{lib,build,features,broker,fixture}.rs`
- `harness/core/crates/orrery-harness/tests/{end_to_end,intercept,lifecycle,turn,budget,retry,cancel}.rs`

**Amended while implementing.** The kernel's integration tests were planned for
`orrery-kernel/tests/` and live in `orrery-harness/tests/` instead. `cargo xtask
deps-check` rule 1 - no `core/` crate depends on an `extensions/` one - counts
**dev-dependencies**, and a kernel test needs a provider and a store, both of
which are extensions. The choice was between doubles written to keep the graph
tidy and the real store with the committed streams; the real ones found two bugs
this plan would otherwise have shipped (see the State note), so the tests moved
rather than the assertions weakening. `orrery-harness` is the one crate the rule
exempts.

Two files the plan did not name, both in `orrery-harness`:

- `src/broker.rs` - `PolicyBroker`, the `BrokerFacade` an extension holds,
  behind the same `PolicyEngine` the registry's gate uses. Nothing existed that
  joined `orrery-ext-api`'s facade to `orrery-broker`'s implementation; without
  it a builtin tool reaches `DeniesEverything`.
- `src/fixture.rs` - a fixture provider that answers a *turn*. The extension's
  own replays one file from the start every time, which cannot express "ask for
  a tool, then answer".

---

## Tasks

### Task 1 · Phases and the verdict chain

Files: `src/phase.rs`, `src/intercept.rs`, `tests/intercept.rs`

- [x] **Failing test first.** `intercept::rewrite_is_typed` — a compile-fail test proving an interceptor registered for `ToolBefore` cannot return a `ModelRequest`. This is translation #2's whole value.
  - **Amended: a `compile_fail` doctest, not `trybuild`.** `trybuild` is not one
    of the workspace's pinned dependencies and adding it would mean a network
    fetch, which this phase forbids. A `compile_fail` doctest is what this repo
    already uses for exactly this — `BranchLease`, `CapabilityToken`, `CallCtx`
    — and it proves the same thing from outside the crate. It sits on
    `phase.rs`, paired with a compiling version of the same shape so that the
    failure is a type error and not a typo; `tests/intercept.rs` holds the
    runtime half.
- [x] `intercept::deny_narrows_only` — an interceptor returning `Deny` on a call policy allowed ⇒ denied; an interceptor returning `Continue` on a call policy denied ⇒ still denied.
- [x] `intercept::order_is_deterministic` — three interceptors, registered in a known order, observed in that order, twice.
- [x] `intercept::handled_short_circuits` — a `Handled` verdict means dispatch never runs.
- [x] Implement `Phase`, the ten marker types, `Interceptor<P>`, `InterceptCtx`, `InterceptorSet`, `ChainOutcome`.
- [x] Reject a `Deny` registration at `context.build` with a clear error.

### Task 2 · Lifecycle handlers

Files: `src/lifecycle.rs`

- [x] **Failing test first.** `lifecycle::failure_does_not_fail_the_turn` — a handler that returns `Err` at `turn.end`; the turn still reports `Completed`, and the failure is in the audit.
- [x] `lifecycle::returns_nothing` — a type-level assertion that `run` returns `()`; it cannot alter the turn.
- [x] Implement.

### Task 3 · Context assembly

Files: `src/context.rs`, `tests/turn.rs`

- [x] **Failing test first.** `context::stable_prefix_is_first` — with a memory provider contributing, the recalled block appears *after* the tool descriptors, and `cache_breakpoint` points past the descriptors.
- [x] `context::tool_order_is_stable` — building twice with the same scope produces byte-identical prefixes. (This is the test that catches a `HashMap` sneaking in.)
- [x] `context::no_tools_when_provider_lacks_them`.
- [x] `context::overflow_triggers_compact_then_rebuild` — a tiny `max_context`; assert `compact` ran once and the second build fits.
- [x] `context::compaction_gives_up` — after 2 attempts, `StoppedByBudget(Tokens)`, not an infinite loop.
- [x] Implement `ContextDraft`, the assembly, the fit check, the compact trigger.

### Task 4 · The turn loop

Files: `src/turn.rs`, `tests/turn.rs`

- [x] **Failing test first.** `turn::text_only_completes` — fixture provider with a text-only stream ⇒ `TurnOutcome::Completed` with usage, one assistant turn appended.
- [x] `turn::tool_call_round_trips` — fixture stream with one tool call ⇒ dispatch happens, a `ToolResult` turn is appended, a second pass runs, the turn completes.
- [x] `turn::denial_is_appended_not_raised` — a denied call produces a `ToolResult` turn holding `Outcome::Denied` and the loop continues.
- [x] `turn::appends_before_next_pass` — assert the tree holds the completed pass before the next one begins. Read from *inside* the tool call rather than by killing the process: the tool host materialises the branch mid-dispatch and sees `user, assistant` already written, which is the same property without a second process.
- [x] Implement `run_turn`.

### Task 5 · Budgets

Files: `src/budget.rs`, `tests/budget.rs`

- [x] **Failing test first.** `budget::max_turns_stops` — a fixture that always asks for a tool, `max_turns = 3` ⇒ `StoppedByBudget(Turns)` after exactly 3.
- [x] `budget::wall_clock_stops`, `budget::tokens_stop`.
- [x] `budget::stop_is_a_value` — a compile-time assertion that `StoppedByBudget` is a `TurnOutcome` variant, not a `KernelError` one.
- [x] Implement `TurnBudget`, checks before each pass and each dispatch.

### Task 6 · Retry

Files: `src/retry.rs`

- [x] **Failing test first.** `retry::retryable_is_retried_and_charged` — a fixture stream that fails twice with `RateLimited` then succeeds; assert 3 attempts, all audited, and the backoff time counted against wall clock.
- [x] `retry::terminal_is_not_retried` — `Auth` fails immediately, one attempt.
- [x] `retry::exhaustion_is_an_outcome` — after the cap, the turn ends with a typed outcome rather than an error.
- [x] Implement bounded exponential backoff with jitter. ~~Defaults hardcoded with `TODO(plan-10)`.~~ **Not hardcoded any more (round 5):** plan 10 landed, and `orrery_harness::kernel_config` reads `[retry] maxAttempts / baseMs / maxMs / jitter` off the profile overlay into `KernelConfig::retry`.

### Task 7 · Cancellation

Files: `tests/cancel.rs`

- [x] **Failing test first.** `cancel::mid_stream` — cancel during the provider stream; assert `TurnOutcome::Cancelled(User)`, the partial assistant text is appended, and the stream is dropped.
- [x] `cancel::mid_tool` — cancel during dispatch; assert `Outcome::Cancelled` on that call and that the call's capabilities were taken back.
  - **Amended: the revoker, not the tool's token.** The kernel cannot observe
    the tool's cancel token: `ExtensionTable` builds a `CallCtx` with a child of
    the *extension's* token, not of the turn's, so "the tool's child token
    fired" is not a property the kernel can assert or even arrange. What it can
    do is take the capabilities back, which is the effect that matters - an
    in-flight tool's next broker call fails `Revoked` instead of succeeding on a
    grant nobody wants. The test asserts the settled `Outcome::Cancelled` and
    that the `CallRevoker` was invoked for that call id.
- [x] `cancel::reason_is_recorded` — budget-triggered cancellation carries `CancelReason::Budget`.
- [x] Implement the token tree.

### Task 8 · The facade

Files: `orrery-harness/src/*`, `tests/end_to_end.rs`

- [x] **Failing test first.** `end_to_end::one_turn_with_a_tool_call` — build a `Harness` with the fixture provider, the sqlite store and the builtin tool bundle; submit a turn; assert the transcript holds user → assistant(tool_use) → tool_result → assistant(text), and that the tool actually touched the filesystem through the broker.
- [x] Implement `Harness::build`, feature wiring, the runtime, `block_on`.
- [x] Add the `deps-check` allow-list entry for this crate.

### Task 9 · Auth gating

Files: `src/turn.rs`

- [x] **Failing test first.** `turn::needs_login_is_refused_early` — a provider reporting `NeedsLogin`; assert the turn returns `NeedsLogin` at `provider.before` **before** any context is built, and that nothing was appended.
- [x] Implement.

---

## Done when

- `cargo test -p orrery-kernel` and `-p orrery-harness` green. **54 tests**
  (~~46~~ — amended: 46 was the `orrery-harness` suites alone and left out the
  kernel's own 8; measured, it is 46 + 8). The
  kernel's own binary runs its unit tests and the two `phase.rs` doctests, one
  of which is the compile-fail that is translation #2's proof; the six
  behavioural suites run under `-p orrery-harness`, for the dependency reason
  under **File structure**.
- ~~`orrery run -p "list the files here"` (plan 17) completes a real turn with a
  real tool call against a real provider.~~ **Amended twice. (1) Not reachable from
  here; (2) the half that said `orrery run` does not exist is now stale.**
  Plan 17 owns the `orrery run` command and it has **landed**:
  `orrery run -p "…"` completes a real turn with a real tool call today, and
  `orrery-cli`'s `json::completes_a_turn_with_a_tool_call` asserts it. What
  stays true is the other half — "a real provider" means an API key and a
  network request, which this phase is not allowed to make, so the turn runs
  against the fixture provider. What is true, and is what
  `end_to_end::one_turn_with_a_tool_call` asserts: a `Harness` built from a
  config - fixture provider, sqlite store, builtin tool bundle - completes one
  turn whose transcript reads user -> assistant(tool_use) -> tool_result ->
  assistant(text), where the tool read a real file from disk **through the
  broker**, under a capability token the policy engine minted, and both
  decisions are in the audit. Plan 17 inherits `Harness::submit` and a working
  loop; what it adds is a command line.
- Every budget kind demonstrably stops the loop - turns, tokens, wall clock and
  money, in `budget.rs`. Money needs a `PriceTable`; without one it is inert
  rather than free, and `budget::money_stops_only_when_priced` asserts both
  halves. **Amended, round 5:** that was a library assertion, and the binary
  could not reach it — `orrery-harness` had no dependency on `orrery-config`, so
  no price table ever arrived and `maxUsd` was inert in every *build*. It is now
  asserted where it counts, in `orrery-cli/tests/budget.rs`, driving the binary.
- Cancellation leaves partial work in the tree.

## Open questions

1. **Where does the retry policy live?** **Answered, round 5: per profile, and
   not per provider.** `[retry] maxAttempts / baseMs / maxMs / jitter`, read
   through the profile overlay by `orrery_harness::kernel_config` and handed
   over as `KernelConfig::retry`. Per-provider stays unbuilt because nothing
   asks for it: a `RetryPolicy` is one value on the kernel, and a per-provider
   one would need the kernel to hold a map keyed by provider id — which would
   also put the "how hard to try" decision back in the providers, the thing
   keeping it here was meant to prevent.

   **Decided: both, and the profile narrows.** A provider knows things a profile
   cannot - an API that always answers `Retry-After`, a local model that never
   rate-limits, a gateway that 503s under load - so a per-provider default
   belongs with the provider. A profile knows what the *user* is willing to
   spend waiting, which is a different question in the same units. So plan 10
   reads `[retry]` at both levels and takes the **tighter** of the two, field by
   field, exactly as `ToolBudget::effective` does for ceilings: `max_attempts`
   the lower, `max_ms` the lower. One direction, so a profile can make a
   provider less patient and never more.

   `RetryPolicy` is one struct on `KernelConfig`, and plan 10 landed: it is
   filled from the profile overlay rather than hardcoded. The per-provider
   half stays unbuilt because nothing asks for it; `backoff` already obeys a provider's `Retry-After` over its own doubling
   - which is the per-provider half in the one place it currently matters.
2. **Compaction attempts = 2.** Arbitrary. Is one enough? Is three ever useful? Decide with a real over-long context.

   **Decided: 2**, with the reasoning written into
   `KernelConfig::compaction_attempts`. One is not enough: the first attempt
   summarises the oldest span, and a turn whose *recent* history is what
   overflows still will not fit. Three has never helped - if two summaries have
   not made room, what does not fit is the newest turn plus the prompt, and a
   third pass over the same material spends another model call to discover that
   again. `turn::compaction_gives_up` pins it: two attempts, then
   `StoppedByBudget(Tokens)`, which is a value a client can act on.
3. **`PassId`** — is it worth its own opaque id, or is `(TurnId, u32)` enough? The audit wants to group by pass; a plain index is simpler and sorts naturally.

   **Decided: `(TurnId, u32)`.** `PassId { turn, index }`, `Copy`, `Ord`, and
   it prints as `<turn>#2`. The only thing an opaque id would add is uniqueness
   across turns, which the `TurnId` half already provides, and it would need a
   generator and a serde impl to do it. The index sorts naturally, which is what
   the audit wanted in the first place.
4. **Interceptor registration at `session.start`** must validate that every registered phase name exists. Do it with a `const NAME` lookup table, or make registration type-driven so an unknown phase cannot be named at all? The latter is better and probably free.

   **Decided: type-driven, and it was free.** `InterceptorSet::register<P: Phase>`
   takes the phase as a type parameter and files the interceptor under
   `P::NAME`, so there is no string to validate and no way to name a phase that
   does not exist. `ALL_PHASES` still exists, for the ledger and for `orrery
   explain`, but nothing looks a phase up by name.

   Registration does validate one thing the question did not anticipate: an
   interceptor that **can deny**, registered at a phase where a denial is
   meaningless, is refused with an error saying what to do instead.
   `Phase::ALLOWS_DENY` is false for `context.build`, and `Interceptor::can_deny`
   is what an interceptor declares about itself, because the alternative -
   discovering it mid-turn - means either ignoring a verdict or failing a turn
   over a configuration mistake. `intercept::deny_at_context_build_is_refused`
   covers it.

---

## State

**Landed (2026-09-18, wave 3).** All nine tasks. `orrery-kernel` is the loop and
`orrery-harness` assembles it: 46 tests, `deps-check` clean, clippy clean.

- **`orrery-kernel`** - `Phase` and the ten marker types, the sync interceptor
  chain behind a type-erased per-phase table, lifecycle handlers, context
  assembly with compaction, `TurnBudget`, `RetryPolicy`, the cancellation tree
  and `run_turn`. `KernelError` has three variants and all of them are the
  harness breaking: a ceiling, a cancellation, a refusal and a login prompt are
  `TurnOutcome` values.
- **`orrery-harness`** - `Harness::build`, the runtime and `block_on`,
  `PolicyBroker` (the missing join between `orrery-ext-api`'s facade and
  `orrery-broker`'s implementation), `LedgerRevoker`, the feature-gated
  first-party set, and a sequencing fixture provider.

**Two bugs the tests found**, both of which a mocked store would have hidden:

- **Compaction was unreachable.** `SessionStore::materialise` already fits a
  branch to its budget by eliding turns out of the *middle* of the view, so the
  kernel never saw an overflow, never compacted, and quietly sent a conversation
  with its middle missing. Elision is now the trigger - which is what
  `Materialised::elided` documented itself as - and the store is handed the
  window *minus* the system prompt, so the prompt and the conversation cannot
  overflow the context between them.
- **A cancelled pass threw away what the model had already said.** The partial
  `PassResult` now travels with the cancellation and is appended, so the tree
  holds what the person was looking at when they stopped it.

Compaction also no longer summarises the turn that triggered it: it covers
everything strictly *before* this turn's input, or it does not run. Without that
floor, a turn long enough to overflow would summarise away the question it was
answering.

**Two holes left open on purpose**, both with typed no-ops so that plan 11 and
plan 12 are substitutions rather than rewrites: `MemoryRecall` (plan 12 also
owes the `TurnKind::Recalled` row, noted at the call site) and `Compactor`,
whose default refuses - a context that will not fit stops the turn with
`Tokens` rather than being silently truncated into a model that would then
answer about half a conversation.

**A gap that is written down rather than hidden.** `ExtensionTable` holds one
`Arc<dyn BrokerFacade>` for every call it serves, while the `CallId` and the
cancel token live on the per-call `CallCtx`. So the broker a tool reaches in
production cannot tell which call it is serving: tokens are minted against a
fresh call id, and `TokenLedger::revoke_call` cannot reach them.
`PolicyBroker::for_call` is the shape of the fix and the builtin suite already
uses it; what is missing is for the extension host to call it. Until then the
kernel's own `CallRevoker` is what makes a cancelled call's capabilities go
away.
