# 03 · Provider — model I/O and nothing else

**Goal.** A `Provider` trait thin enough that a community provider is a manifest plus three functions, plus three implementations: a **fixture** provider that replays a recorded stream (deterministic, no API key — the thing every other plan's tests run against), the **Anthropic** Messages API, and later an **OpenAI-compatible** client for local models. When this is done the kernel can stream a completion, count its tokens, classify its failures, and never see a credential.

**Covers.** §4.5 in full.

**Crates.** `core/crates/orrery-provider` (published, trait only) · `extensions/crates/orrery-ext-provider-fixture` · `extensions/crates/orrery-ext-provider-anthropic` · `extensions/crates/orrery-ext-provider-openai-compat` (phase 5).

**Depends on.** [`01-proto-shared-types.md`](01-proto-shared-types.md). The `creds` grant it uses is plan 07; until then a dev-only env-var resolver marked `TODO(plan-07)`.

---

## Constraints

From [`00-overview.md`](00-overview.md):

- Object-safe without `async_trait`: `stream` is a non-async fn returning `BoxStream<'static, …>`.
- `ModelRequest` owns `Arc<[Message]>` so the stream is `'static`.
- **Provider code never sleeps and never retries.** It classifies; the kernel spends. Otherwise budgets stop meaning anything.
- **No vendored SDKs.** Hand-written request builders, as §4.5 argues.
- Credentials stay in the broker under a named grant. A provider asks for one; it never reads a key off disk or out of config.
- Dropping the stream must abort the HTTP request — that is what makes cancellation stop costing money.

This crate owns translation **#6** jointly with plan 02 (the `TokenCounter` implementations).

---

## Architecture

### The trait

```rust
pub trait Provider: Send + Sync + 'static {
    fn id(&self) -> &str;
    fn capabilities(&self) -> &Capabilities;

    fn stream(&self, req: ModelRequest, cancel: CancellationToken)
        -> BoxStream<'static, Result<ModelEvent, ProviderError>>;

    fn counter(&self) -> Arc<dyn TokenCounter>;
    fn auth(&self) -> Arc<dyn ProviderAuth>;
}

pub struct Capabilities {
    pub tools: bool, pub images: bool, pub cache: bool,
    pub max_context: u64, pub max_output: u64,
}
```

`capabilities` is not decoration (§4.5). A provider without `tools` is never sent tool descriptors; a `max_context` below the assembled context triggers compaction instead of a rejected request. Plan 05 reads it before it builds anything.

### The request

```rust
pub struct ModelRequest {
    pub model: String,
    pub system: Option<Arc<str>>,
    pub messages: Arc<[Message]>,
    pub tools: Arc<[ToolDescriptor]>,     // empty when !capabilities.tools
    pub max_output_tokens: u64,
    pub temperature: Option<f32>,
    pub stop: Vec<String>,
    /// Where the stable prefix ends. Providers with `cache` key on this.
    pub cache_breakpoint: Option<usize>,
}
```

`cache_breakpoint` is an index into `messages`, computed by plan 05's assembly order. Putting it in the request rather than letting each provider guess is what makes §4.3's "stable prefix first" actually save money.

### The event stream

```rust
#[non_exhaustive]
pub enum ModelEvent {
    Started      { id: String },
    TextDelta    { text: String },
    ThinkingDelta{ text: String },
    ToolUseStart { call: CallId, name: String },
    ToolUseDelta { call: CallId, json_fragment: String },
    ToolUseEnd   { call: CallId },
    Usage        (Usage),
    Done         { stop: StopReason },
}

#[non_exhaustive]
pub enum StopReason { EndTurn, ToolUse, MaxTokens, StopSequence, Refusal }
```

Tool-call arguments arrive as JSON **fragments**, because every streaming API emits them that way. Accumulation and parsing happen once, in this crate, behind a helper the kernel uses — not three times in three providers and not in the kernel:

```rust
/// Accumulates ToolUse{Start,Delta,End} into complete calls. One implementation for all providers.
pub struct ToolCallAccumulator { /* .. */ }
impl ToolCallAccumulator {
    pub fn feed(&mut self, ev: &ModelEvent) -> Option<CompletedToolCall>;
}
```

### Errors — the classification is the contract

```rust
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum ProviderError {
    // Retryable: the kernel may back off and retry, charged to the turn budget.
    #[error("rate limited{}", .retry_after_ms.map(|m| format!(", retry after {m}ms")).unwrap_or_default())]
    RateLimited { retry_after_ms: Option<u64> },
    #[error("server error {status}")]      ServerError { status: u16 },
    #[error("timeout after {elapsed_ms}ms")] Timeout { elapsed_ms: u64 },
    #[error("connection: {0}")]            Connection(String),

    // Terminal: retrying is pointless, and pretending otherwise burns the budget.
    #[error("authentication: {0}")]        Auth(String),
    #[error("bad request: {0}")]           BadRequest(String),
    #[error("refused: {0}")]               Refusal(String),
    #[error("context too long: {tokens} > {max}")] ContextTooLong { tokens: u64, max: u64 },
    #[error("model {0} not available")]    NoSuchModel(String),
}

impl ProviderError {
    pub fn is_retryable(&self) -> bool;
    pub fn code(&self) -> &'static str;
}
```

A `429` with no `Retry-After` is still retryable; a `400` never is. `ContextTooLong` is separated from `BadRequest` because the kernel's response is different — compact and retry rather than fail the turn.

### Auth

```rust
#[async_trait]
pub trait ProviderAuth: Send + Sync {
    fn methods(&self) -> &[AuthMethod];
    async fn state(&self) -> Result<AuthState, ProviderError>;
    async fn login(&self, ctx: &dyn AuthCtx) -> Result<AuthState, ProviderError>;
    async fn refresh(&self) -> Result<AuthState, ProviderError>;   // idempotent under concurrent passes
    async fn logout(&self) -> Result<(), ProviderError>;
}

#[non_exhaustive]
pub enum AuthState {
    Anonymous,
    Ready      { account: Option<String>, expires_at: Option<u64> },
    NeedsLogin { reason: String },
    Expired,
}
```

`login` declares its steps as **UI surfaces** through `AuthCtx`, so one flow serves ratatui, Ink, the ADE and `--json`, and a profile with `consent = "never"` fails rather than prompting (§4.5). `AuthCtx` is a trait here with one method — `async fn ask(&self, surface: Surface) -> Result<serde_json::Value, ProviderError>` — implemented by plan 08's client bridge.

A turn that starts `NeedsLogin` or `Expired` is refused at `provider.before` with a typed error the client renders as a login prompt — never a stall mid-pass waiting on a browser. That check is plan 05's; this crate only reports the state.

### Token counting

```rust
pub trait TokenCounter: Send + Sync {
    fn count_messages(&self, messages: &[Message]) -> u64;
    fn count_text(&self, text: &str) -> u64;
    /// True when this is a real tokenizer rather than an estimate. Compaction keeps
    /// a bigger safety margin when false (translation #6).
    fn is_exact(&self) -> bool { false }
}
```

Phase 1 ships `HeuristicCounter` (chars/4 with a per-block overhead) for every provider. An exact tokenizer is a phase-5 question — it is a large dependency and the margin covers us.

---

## The three providers

### fixture — build this first

`extensions/crates/orrery-ext-provider-fixture`. Reads a `.jsonl` file where each line is a `ModelEvent`, emits them with optional delays, ignores the request. Everything else in the harness tests against it.

```
orrery serve --provider fixture:harness/clients/conformance/turn-with-tool-call.jsonl
```

It must support: a plain text turn; a turn with one tool call; a turn with a tool call that will trigger consent; a turn that stops on `MaxTokens`; a stream that errors partway with a retryable error; a stream that never ends (for cancellation tests). Those six fixtures are the reusable corpus.

### anthropic

Hand-written Messages API client. `reqwest` with the existing rustls setup, `POST /v1/messages`, `stream: true`.

- SSE parsing: hand-rolled over `bytes`. `eventsource-stream` is a thin wrapper and the format is six lines of parsing; the dependency is not worth it. **Decide in task 5 and record it.**
- Map `content_block_start` / `content_block_delta` / `message_delta` / `message_stop` onto `ModelEvent`.
- `cache_control: {"type":"ephemeral"}` on the block at `cache_breakpoint`.
- Read `usage.input_tokens`, `output_tokens`, `cache_read_input_tokens` into `Usage`.
- Auth: `x-api-key` from a `creds` grant named `anthropic`. OAuth is a later task.

### openai-compat

Phase 5. `POST /v1/chat/completions`, `stream: true`, `tool_calls` deltas. Exists so §4.6's `local/qwen-coder` examples are real. Same trait, different builder.

---

## File structure

**Create**

- `harness/core/crates/orrery-provider/src/{lib,provider,request,event,error,auth,counter,accumulate}.rs`
- `harness/core/crates/orrery-provider/tests/accumulate.rs`
- `harness/extensions/crates/orrery-ext-provider-fixture/{Cargo.toml,orrery.toml,README.md,src/lib.rs}`
- `harness/clients/conformance/streams/*.jsonl` — the six fixtures (shared with plan 08/09)
- `harness/extensions/crates/orrery-ext-provider-anthropic/{Cargo.toml,orrery.toml,README.md}`
- `harness/extensions/crates/orrery-ext-provider-anthropic/src/{lib,request,sse,map,auth}.rs`
- `harness/extensions/crates/orrery-ext-provider-anthropic/tests/fixtures/*.sse` — recorded responses

---

## Tasks

### Task 1 · The trait and the types

Files: `orrery-provider/src/*`

- [x] **Failing test first.** `provider::is_object_safe` — a `fn takes(_: Arc<dyn Provider>) {}` compiles. Trivial, and it catches the day someone adds a generic method.
- [x] Implement `Provider`, `Capabilities`, `ModelRequest`, `ModelEvent`, `StopReason`, `ProviderError`, `AuthState`, `ProviderAuth`, `AuthMethod`, `AuthCtx`.
- [x] `ProviderError::is_retryable` + `code`, with a test enumerating every variant so a new one cannot be added without classifying it.

### Task 2 · The tool-call accumulator

Files: `src/accumulate.rs`, `tests/accumulate.rs`

- [x] **Failing test first.** `accumulate::reassembles_split_json` — feed `{"pa`, `th":"/tmp`, `"}` across three deltas, get one `CompletedToolCall` with parsed input.
- [x] `accumulate::two_interleaved_calls` — two `CallId`s streaming at once come out separately.
- [x] `accumulate::malformed_json_is_an_error_not_a_panic`.
- [x] Implement.

### Task 3 · The heuristic counter

Files: `src/counter.rs`

- [x] **Failing test first.** `counter::is_monotonic` (proptest) — adding a message never lowers the count.
- [x] Implement `HeuristicCounter`, `is_exact() == false`.

### Task 4 · The fixture provider

Files: `orrery-ext-provider-fixture/*`, `harness/clients/conformance/streams/*.jsonl`

- [x] **Failing test first.** `fixture::replays_in_order` — a three-event file yields three events in order.
- [x] `fixture::cancellation_ends_the_stream` — with a fixture that has a long delay, cancel and assert the stream ends promptly.
- [x] Implement the provider, the `fixture:<path>` spec parsing, and optional `delay_ms` per line.
- [x] Write the six fixtures. These are consumed by plans 05, 08, 09b and 09c — name them clearly and document the format in `harness/clients/conformance/README.md`.

### Task 5 · Anthropic — SSE

Files: `orrery-ext-provider-anthropic/src/sse.rs`, `tests/`

- [x] **Decision to record:** hand-rolled SSE or `eventsource-stream`.

> **Decided: hand-rolled, in `src/sse.rs`.** (2026-09-18)
>
> The parser is a `push(&[u8]) -> Vec<SseEvent>` state machine over a `BytesMut`
> — about eighty lines, of which half is the field-name match. Four reasons it
> beat the dependency:
>
> 1. **It is a byte splitter, not a protocol.** Anthropic uses none of what
>    `eventsource-stream` exists to provide: no `id:`, no `Last-Event-ID`
>    resumption, no `retry:`, no reconnection. That is the whole value of the
>    crate and we would use none of it.
> 2. **The test we actually need is synchronous.** `sse::handles_split_frames`
>    feeds the same fixture in 7-byte chunks and asserts identical output. A
>    `push`/`drain` parser tests that with a `for` loop; a `Stream` adapter needs
>    a mock stream, a runtime and a collect, to prove less.
> 3. **Dependency shape.** `eventsource-stream` pins its own `futures`, `bytes`
>    and `nom` majors. The workspace deliberately holds one copy of each of
>    those, and a transitive bump on somebody else's release schedule is a worse
>    trade than eighty lines we own.
> 4. **Failure attribution.** A malformed frame has to become a typed
>    `ProviderError`, not a `Box<dyn Error>` from a crate that has never heard of
>    one.
>
> Revisit if a second provider needs `Last-Event-ID` resumption — that is real
> protocol and worth a real dependency. Byte splitting is not.
- [x] **Failing test first.** `sse::parses_recorded_stream` — feed a recorded `.sse` fixture through the parser, assert the event sequence. Record fixtures once from a real call and commit them; **no test makes a network request.**
- [x] `sse::handles_split_frames` — the same fixture fed in 7-byte chunks produces identical output.
- [x] `sse::ignores_ping_and_comment_lines`.
- [x] Implement.

### Task 6 · Anthropic — request building and mapping

Files: `src/request.rs`, `src/map.rs`

- [x] **Failing test first.** `request::tool_descriptors_omitted_when_unsupported` — with `capabilities.tools == false` the body has no `tools` key.
- [x] `request::cache_control_lands_on_the_breakpoint` — given `cache_breakpoint: Some(2)`, the third block carries `cache_control`.
- [x] `map::usage_includes_cache_hits`.
- [x] `map::stop_reason_maps` — every Anthropic `stop_reason` string maps to a `StopReason`; an unknown one is an error, not a silent default.
- [x] Implement.

### Task 7 · Anthropic — errors and auth

Files: `src/lib.rs`, `src/auth.rs`

- [x] **Failing test first.** `error::classification` — a table of (status, body) → expected `ProviderError` variant and `is_retryable`, including 429 with and without `Retry-After`, 401, 400, 500, 529.
- [x] `auth::missing_key_is_needs_login` — no credential ⇒ `AuthState::NeedsLogin`, not a panic and not a 401 at request time.
- [x] Implement API-key auth over a named `creds` grant. OAuth: leave a `TODO(phase-5)` with the `AuthMethod::OAuth` variant present but `login` returning `NeedsLogin { reason: "oauth not implemented" }`.

### Task 8 · Cancellation actually aborts

Files: `tests/cancel.rs`

- [x] **Failing test first.** `cancel::drops_the_body` — start a stream against a local test server that never finishes, cancel the token, assert the server observes the connection close within a timeout. This is the test that proves "stops costing money at once".
- [x] Wire `CancellationToken` into the stream via `select!` and ensure the `reqwest::Response` is dropped.

### Task 9 · Manifests

Files: `*/orrery.toml`

- [x] Fixture: `runtime = "native"`, `[provides] providers = ["fixture"]`, `[requires] read = ["$WORKSPACE/**"]`.
- [x] Anthropic: `[provides] providers = ["anthropic"]`, `[requires] net = ["api.anthropic.com"]`, `creds = ["anthropic"]`.
- [x] Note in both READMEs that a community provider is exactly this shape.

---

## State

Phase 1 is implemented and green as of 2026-09-18: tasks 1-9 except the
`openai-compat` crate, which the plan itself places in phase 5. `cargo test -p
orrery-provider -p orrery-ext-provider-fixture -p orrery-ext-provider-anthropic`
passes 53 tests, clippy is clean, and no test opens a socket to anything but
loopback.

Three shapes departed from the sketch above, each for a reason worth keeping:

- **`ModelEvent::Usage { usage }`**, a struct variant, not `Usage(Usage)`. Plan 01's
  "struct variants only in every tagged enum" rule is what keeps the type
  round-tripping over CBOR, and the fixture corpus serialises these events.
- **`ToolCallAccumulator::feed` returns `Option<Result<CompletedToolCall, _>>`.** The
  plan asks for `Option<CompletedToolCall>` and, two lines later, for malformed JSON to
  be an error rather than a panic. There is nowhere else for that error to go.
- **`ToolDescriptor` lives in `orrery-provider`**, not in plan 04's `orrery-tools`. A
  provider crate has no business depending on dispatch, policy and budgets to name the
  three fields a model is shown. The registry converts into it.

## Done when

- `cargo test -p orrery-provider`, `-p orrery-ext-provider-fixture`, `-p orrery-ext-provider-anthropic` green.
- No test makes a network request.
- The six conformance stream fixtures exist and are documented.
- The cancellation test demonstrates the connection closing.

## Open questions

1. **Exact tokenizers.** Heuristic counting means compaction runs with a margin and `maxContext` enforcement is approximate. Adding `tiktoken`-style tokenizers is a large dependency per provider family. Revisit when a real workload gets bitten; until then the margin is the answer.
2. **OAuth.** Anthropic and OpenAI both have device-code flows worth supporting, and `login` already declares UI surfaces for it. Phase 5, unless a user needs it sooner.
3. **Retry policy lives in the kernel** — but *where* do the defaults come from? Profile config (plan 10) is the natural home. Until then a hardcoded bounded backoff in plan 05, marked `TODO(plan-10)`.
4. **Prompt caching across providers.** `cache_breakpoint` is an Anthropic-shaped idea. OpenAI-compatible endpoints cache implicitly by prefix. Confirm the field degrades to a no-op rather than forcing a bad request.

> **Confirmed, and pinned by a test.** (2026-09-18) `cache_breakpoint` is advice, not an
> instruction. The Anthropic builder consults `capabilities.cache` before it writes a
> marker, so a provider that declares `cache: false` emits no `cache_control` anywhere —
> `request::cache_breakpoint_degrades_to_a_no_op_without_the_capability`. An index past the
> end of `messages` is likewise ignored rather than rejected
> (`an_out_of_range_breakpoint_is_ignored_not_an_error`), because the assembler computing
> it and the provider consuming it can disagree by a message after a compaction and the
> right answer to that is a slightly worse cache hit rate, not a failed turn. An
> OpenAI-compatible provider will read the field and do nothing with it.
