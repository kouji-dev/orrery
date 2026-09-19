# 00 · Overview — the Rust translation

> Read [`../architecture.md`](../architecture.md) first. This file does not restate it; it says how it becomes Rust, and it is the one place every other plan inherits decisions from.

**Covers.** §1 (objectives) · §2 (current state) · §3 (core architecture) · §7 (fixed and customizable) · §8 (build order). Every other section is owned by a numbered plan; see the index in [`../README.md`](../README.md).

## What we are building

A Rust agent runtime an organisation configures into the coding agent it actually wants, without forking anything and without trusting arbitrary community code with its filesystem. Ten objectives, §1 of the architecture. The structural rule the whole design hangs on:

> **An extension has no capability except through a token the policy engine issued for that specific call.**

## Repository shape

```
orrery/
├─ ade/                    # today's Tauri + Angular app, renamed orrery-ade (plan 00a)
├─ harness/
│  ├─ core/crates/         # kernel-owned: traits, loop, hosts, policy, transport
│  ├─ extensions/
│  │  ├─ crates/           # first-party Rust extensions — publishable, community-shaped
│  │  ├─ node/             # first-party TS extensions + the @orrery/ext SDK
│  │  └─ examples/         # wasm (Rust, TinyGo) + process samples
│  ├─ clients/             # every renderer, one folder each
│  │  ├─ sdk-rs/ sdk-ts/   # AguiSession + SurfaceStore, no drawing
│  │  ├─ ratatui/ json/    # Rust renderers, linked into the binary
│  │  ├─ ink/              # React/Ink renderer, spawned as a Node process
│  │  ├─ ade/              # pointer: the Angular renderer lives in ade/
│  │  └─ conformance/      # fixtures every client runs
│  ├─ xtask/               # typegen · deps-check · wit-check · agui-drift
│  ├─ wit/                 # orrery-extension.wit
│  ├─ protocol/            # generated schema + .d.ts (@orrery/protocol)
│  └─ docs/                # this
├─ landing/  .github/  Cargo.toml  package.json  pnpm-workspace.yaml
```

### Why `core/` and `extensions/` are different directories

§4.7 gives the rule: **"if a subsystem can be replaced, it is a field on `ExtensionDefinition`; if it is not a field there, it is ours."** `core/` is what the kernel owns and calls. `extensions/` is every first-party implementation of an `ExtensionDefinition` field — the tool bundle, providers, the session backend, memory, graders, the default role agents, the default view bindings.

§8 decided *everything is an extension*: the built-in tools ship as a first-party bundle so the extension API carries real work from day one and no privileged in-kernel path exists to rot beside it. That is what makes `extensions/` non-empty in phase 1.

**Direction rule, enforced by `cargo xtask deps-check`:** a crate in `extensions/` may depend on `core/`; **no `core/` crate may depend on an `extensions/` crate** — except `orrery-harness`, the facade, which links the first-party set as shipped defaults behind cargo features (`default = ["builtin-tools", "anthropic", "sqlite"]`).

### Extensions are publishable crates

Every crate under `extensions/crates/` is written as if a third party owned it, so a community extension is literally the same crate in someone else's repo, and `extensions/` can move to its own repository later without edits. Rules:

- Extensions depend on core through **published** crates only: `orrery-ext-api`, `orrery-proto`, and the trait crates (`orrery-provider`, `orrery-session`, `orrery-memory`, `orrery-grader`). Those get `publish = true`, semver, a CHANGELOG, `#![deny(missing_docs)]`. Everything else in `core/` is `publish = false` — kernel internals are not an API.
- Dependencies are declared with **both** halves: `orrery-ext-api = { version = "0.1", path = "../../../core/crates/orrery-ext-api" }`. `version` makes them publishable, `path` makes them build in-tree. At split time, delete the `path`; nothing else changes.
- No extension reaches into `core/` by relative path for anything else — no shared `build.rs`, no `include!`, no dev-dependency on `orrery-kernel`. Tests use the mock broker in `orrery-ext-api::testing`, the same one `orrery ext test` uses, so a community author has the identical harness.
- Each extension carries its own `README.md`, `orrery.toml`, `CHANGELOG.md`.
- The signed registry (plan 15) indexes crates.io / npm names, versions and signatures. It is a pin list, not a second package host.

See [`18-writing-an-extension.md`](18-writing-an-extension.md).

---

## Crate map

### Table 1 — `harness/core/crates/`

| Crate | Owns | Deps | Phase | Plan |
|---|---|---|---|---|
| `orrery-proto` | The wire and shared types, nothing else. §5.2 `Request`/`Event`; §6.2 `Surface`/`SurfacePatch`; §4.15 `Grant`/`Capability`/`Budget`/`Usage`/`AgentScope`/`Expr`/`Predicate`/`Layer`/`Role`; `Message`/`ContentBlock`; ids; `LoadOutcome`; `Aspect`, `MemScope`, `Subject`. serde + schemars. **No async, no I/O.** Source of the generated `.d.ts`. | — | 1 | 01 |
| `orrery-audit` | Append-only structured stream; redaction in schema (inputs hashed, bodies by reference); three `tracing` layers (ledger / audit / telemetry); file sink; OTLP feature. | proto | 1 file / 3 | 07 |
| `orrery-policy` | `Rule`/`RuleSet`, selector grammar and matcher, `check`/`consent`/`explain`, `PermissionHandler` narrowing, **`CapabilityToken` + `TokenMinter` + nonce ledger**. Fully sync. | proto, audit | 3 | 07 |
| `orrery-broker` | read/write/spawn/net/creds behind a token; output ceilings; wall-clock watchdog; process containment; credential store; atomic-write revert. | proto, policy, audit | 3 | 07 |
| `orrery-provider` | `Provider` trait, `ModelRequest`/`ModelEvent`, `ProviderAuth`/`AuthState`, error classification, `TokenCounter`. **No concrete providers.** Published. | proto | 1 | 03 |
| `orrery-session` | Turn-tree types, `SessionStore` trait, `BranchLease`, pure `materialise`/`compact` algebra, **conformance suite**. No backend. Published. | proto | 1 | 02 |
| `orrery-memory` | `MemoryProvider` trait, scope lifetimes, token clamp, visibility rule, `LifecycleHandler`, conformance suite. No provider. Published. | proto | 6 | 12 |
| `orrery-tools` | Namespacing, `resolve`, `visible(scope)`, `dispatch` — the only path to a tool — `ToolHost` trait, `ToolBudget`. | proto, policy, broker, audit | 1 flat / 2 | 04 |
| `orrery-router` | Roles, bindings, declarative rules, `Signals → RouteDecision` (data), fan-out arithmetic. Sync. | proto | 6 | 11 |
| `orrery-kernel` | `Phase` trait with associated payload, sync interceptor chain, `context.build`, **`run_turn`**, retry charged to budget, cancellation tree, usage accounting. | session, provider, tools, memory, router, policy, audit | 1 | 05 |
| `orrery-orchestrator` | Steps, branch runs on leases, workflow machine, `Expr`/`Predicate` eval + load-time typecheck, budget slicing, parent-performs-join. | kernel, session, router | 6 | 11 |
| `orrery-ext-api` | `ExtensionManifest` (TOML), `ExtensionInstance`, `ToolDef`, `CallCtx`, `ToolResult`, `Contribution`, `LoadLedger`, grant-diff, `testing` mock broker. Published. | proto | 1→2 | 06 |
| `orrery-host` | `ExtensionHost` trait, instance table by **generation id**, load/degrade/unload machine, the `native` runtime, supervision. | ext-api, tools, broker | 1 / 2 | 06 |
| `orrery-jsonrpc` | JSON-RPC 2.0: `framing::{ContentLength, LineDelimited}`, async correlation, bidirectional, `$/cancel`, stderr ring. | — | 2 | 06 |
| `orrery-host-rpc` | node · python · process children; Job Object / process group; manifest `[process]`. | host, jsonrpc | 2 node / 10 | 06 |
| `orrery-wit` | `wit/` as data + `wasmtime::component::bindgen!` host bindings; flat-arena surface. | proto | 2 | 14 |
| `orrery-host-wasm` | wasmtime engine, `Store` per instance, `ResourceLimiter`, epoch deadlines, broker imports, zero preopens, WASI p2. | host, wit | 2 | 14 |
| `orrery-surface` | Kernel-side differ, validation, per-turn store, seal at `turn.settled`. | proto | 1 partial / 4 | 09 |
| `orrery-transport` | §5 framing, `seq` authority, replay ring, per-client coalescer. Listeners: in-process, named pipe + UDS, AG-UI HTTP/SSE; TCP+TLS in phase 4. | proto, agui | 1 / 4 | 08 |
| `orrery-agui` | **Encoder only**: frames → AG-UI events, vendored enum, §5.6 mapping, drift check. | proto | 1 | 08 |
| `orrery-config` | Five layers, TOML merge, provenance, profiles, trust gating, `config explain`, `import`. | proto, policy | 5 | 10 |
| `orrery-mcp` | Client + server; `mcp.<server>` as an ext id; connect-on-need; health; `list_changed` re-resolution. | jsonrpc, host, tools | 7 | 13 |
| `orrery-skills` | `SKILL.md` front matter, discovery pass, `scripts/` under a grant. | proto, ext-api | 7 | 13 |
| `orrery-registry` | Signed registry client, pinning, signature check, refuse-unpinned under managed. | ext-api, config | 8 | 15 |
| `orrery-grader` | `Grader` trait + `GradeInput`/`Score` only. Published, so grader extensions never depend on the runner. | proto | 9 | 16 |
| `orrery-eval` | Runner, isolation, `EvalResult`, compare/replay, cross-harness adapters. | kernel, orchestrator, config, audit, grader | 9 | 16 |
| `orrery-harness` | Facade: builds a `Kernel` from resolved config; owns the tokio `Runtime`; links the first-party set behind features. The only crate an embedder names. | all core + extensions (gated) | 1 | 05 |
| `orrery-cli` (bin `orrery`) | The command tree. Links the ratatui and json clients. | harness, client-ratatui, client-json | 1 | 17 |

### Table 2 — `harness/extensions/crates/`

Each has an `orrery.toml` beside `Cargo.toml` — the same manifest a third party ships — with `runtime = "native"`, and loads through `orrery-host` like anything else: the ledger shows it, a deny rule disables it, `ext test` runs it.

| Crate | Manifest field | Owns | Phase | Plan |
|---|---|---|---|---|
| `orrery-ext-tools-builtin` | `tools` | read, write, edit, bash, grep, glob. Ids `builtin.read` … | 1 | 06 |
| `orrery-ext-provider-fixture` | `providers` | Scripted provider replaying a recorded `ModelEvent` stream from `.jsonl`. Deterministic, no API key. What every client test and demo runs against. | 1 | 03 |
| `orrery-ext-provider-anthropic` | `providers` | Messages API builder, SSE parser, tool-use blocks, `cache_control`, usage. | 1 | 03 |
| `orrery-ext-session-sqlite` | `session` (singleton) | WAL, one txn per append, writer actor, compaction watermarks. Default backend. | 1 | 02 |
| `orrery-ext-views-default` | `views` | The §6.7 floor: assistant text, tool started/settled, consent, errors. | 4 | 09 |
| `orrery-ext-agents-default` | `agents` | §4.6's shipped roles: planner, executor, verifier, compactor, summariser. | 6 | 11 |
| `orrery-ext-memory-file` | `memory` (singleton) | Reference provider, file-backed, `global` + `session` only. | 6 | 12 |
| `orrery-ext-graders` | `graders` | `command`, `assertion`, `model`. | 9 | 16 |
| `orrery-ext-provider-openai-compat` | `providers` | OpenAI-compatible chat completions — local models via ollama/vllm. | 5 | 03 |
| `orrery-ext-git`, `orrery-ext-lsp` | `tools` | Named in §4.9's profile examples. `git.*` can reuse the gix knowledge in `ade/src-tauri/src/git/`. | 4+ | — |

### Table 3 — `harness/clients/`

Every renderer is a whole AG-UI client. None is privileged; the one compiled into the binary attaches over the in-process transport exactly as an external one attaches over a pipe.

| Folder | Crate / package | Owns | Phase | Plan |
|---|---|---|---|---|
| `sdk-rs/` | `orrery-client` (published) | Rust `AguiSession` (decode, `seq`, re-attach `since`, control RPC) + `SurfaceStore`. Pure data, no drawing. | 1 | 08 |
| `sdk-ts/` | `@orrery/client` | The same two things in TS, on stock `@ag-ui/client` `HttpAgent` over SSE. | 1 | 08 |
| `ratatui/` | `orrery-client-ratatui` | Default TUI, linked into the `orrery` binary. §6.4 hybrid. | 1 min / 4 | 09b |
| `json/` | `orrery-client-json` | The `kind: "json"` renderer: line-delimited AG-UI events. Backs `run --json`, CI, evals, adapters. | 1 | 08 |
| `ink/` | `@orrery/client-ink` | React/Ink TUI. Spawned by `orrery --ui ink`, or standalone against `orrery serve`. | 1 min / 4 | 09c |
| `ade/` | README | The Angular + kouji-ui renderer is built inside `ade/` on `@orrery/client`. | later | — |
| `conformance/` | fixtures | AG-UI event scripts + expected `SurfaceStore` state per step. Every client runs them. | 1 | 08 |

### Dependency graph

Acyclic. The cut that makes it so: **the router returns data, the orchestrator acts on it** — the kernel runs a turn, the orchestrator runs steps.

```
core:    proto ─┬─> audit ─> policy ─> broker ─┐
                ├─> provider (trait)            ├─> tools ─> host ─┬─> host-rpc  (← jsonrpc)
                ├─> session  (trait)            │                  └─> host-wasm (← wit)
                ├─> memory / router / ext-api ──┘
                ├─> surface / transport / agui / config / skills / grader
                └──> kernel ─> orchestrator ─> mcp / registry / eval ──┐
                                                                       ├─> harness ─> cli
ext:     provider ─> ext-provider-{fixture,anthropic,openai-compat} ───┤
         session  ─> ext-session-sqlite ──────────────────────────────┤
         ext-api  ─> ext-tools-builtin / ext-agents-default / ext-views-default
         memory   ─> ext-memory-file ;  grader ─> ext-graders

clients: proto ─> orrery-client (sdk-rs) ─┬─> client-ratatui ─┐
                                          └─> client-json ────┴─> linked into cli
         @orrery/protocol ─> @orrery/client (sdk-ts) ─> @orrery/client-ink  (Node, spawned)
```

---

## Cross-cutting decisions

Every plan inherits these. Do not re-litigate them in a plan file; cite them.

### Runtime and sync boundaries

Tokio, `rt-multi-thread`, one runtime owned by `orrery-harness`. The ADE already links tokio via Tauri, so there is no second reactor.

Async only where there is real I/O: provider streams, extension hosts, broker operations, lifecycle handlers.

**Everything that must be replayable is a sync `fn`**, so "performs no I/O" is a compile-time property rather than a convention: `PolicyEngine::check`, `Interceptor::run`, selector matching, the `materialise` tree walk, `Router::decide`, `Expr`/`Predicate` evaluation.

### dyn vs enum vs generic

One rule: **config picks it at runtime → `dyn`; the variant set is ours and closed → `enum`; pure, hot, and chosen at compile time → generic.**

- `Arc<dyn …>`: `Provider`, `SessionStore`, `ToolHost`, `MemoryProvider`, `ExtensionHost`. All bound by configuration.
- `#[non_exhaustive] enum`: `ModelEvent`, `Decision`, `ToolResult`, `RouteDecision`, `Step`, `Surface`, `Request`, `Event`, `TurnOutcome`, `Aspect`, `MemScope`. Anything crossing the wire or needing an exhaustive match.
- Generics: pure paths (`TokenCounter`, matchers) and test fakes only. **The kernel is not generic over `Provider` or `SessionStore`** — that would infect every signature and defeat runtime binding.

`Provider` stays object-safe without `async_trait` by returning a boxed stream from a non-async fn:

```rust
fn stream(&self, req: ModelRequest, cancel: CancellationToken)
    -> BoxStream<'static, Result<ModelEvent, ProviderError>>;
```

`ModelRequest` owns its messages as `Arc<[Message]>` so the stream is `'static` and moves into a task without cloning the context each pass.

### Branch concurrency — the lease

§4.2 requires one turn at a time per branch, parallel work as parallel branches, `append` serialised per branch, and a second submit refused with a typed error rather than queued invisibly. All four fall out of one type:

```rust
pub struct BranchLease(OwnedMutexGuard<BranchState>);   // no Clone, no ctor outside orrery-session

async fn append(&self, lease: &BranchLease, turn: NewTurn) -> Result<TurnId, SessionError>;
```

`try_lock_owned()` (not `lock`) gives `SessionError::BranchBusy` for free. Taking the lease as an argument makes "serialised per branch" unforgeable — the same trick as the capability token. Storage adds `UNIQUE(branch_id, seq)` so a bug is a constraint violation, not corruption.

**The trap:** a child branch closing cannot append to the parent — the parent holds its own lease. **The parent performs the merge at the join point**, writing a `TurnKind::BranchResult { child: BranchId }` row. Nothing is copied; the sub-agent's turns stay inspectable in place. This needs an explicit deadlock test (plan 02).

### Errors — denial is a value

Per-crate `thiserror` enums, `#[from]` upward, **no `anyhow` in any library crate**, no `Box<dyn Error>`. Every variant carries a stable `code: &'static str`.

The load-bearing split: **denial, budget stop and cancellation are values, not errors.**

```rust
pub enum ToolResult { Ok{..}, Denied{ rule: RuleId, reason: String }, Truncated{..},
                      Cancelled, Unloaded, Failed{..} }
pub enum TurnOutcome { Completed(Usage), StoppedByBudget(BudgetKind),
                       Cancelled(CancelReason), NeedsLogin{..} }
```

`dispatch` returns `Result<ToolResult, ToolError>` with denial in the `Ok` arm, so it flows into the transcript as content and the model re-plans instead of retrying until the budget is gone (§4.8: "a denial is terminal and says so"). `ToolError` is reserved for "the harness is broken". A compile-time test asserts `ToolError` has no `Denied` variant.

### CapabilityToken — unforgeable in-process

Primary mechanism is **type privacy**, which in-process beats an HMAC and costs nothing:

```rust
pub struct CapabilityToken(Inner);  // private field; no pub ctor, no Clone/Copy/Default/Serialize/From
```

Only `orrery-policy::TokenMinter::mint` constructs one, and the minter is only constructible at boot from the `Arc<TokenLedger>` the broker also holds. Broker methods take `token: CapabilityToken` **by value** — single-use is enforced by the move. It deliberately does not implement `Serialize`, so it physically cannot cross the extension RPC boundary: §4.8's "no extension ever holds a handle" becomes a type error.

Belt on top: each token carries a `nonce: u64` registered in the ledger's `DashSet`, checked-and-removed by the broker on use. That survives `transmute` tricks, gives replay protection, and gives **revocation on cancel** — cancelling a turn drops its nonces, so an in-flight tool's next broker call fails `Revoked`.

### Budgets, enforced at the broker

- **Wall clock.** `tokio::time::timeout` bounds the *await*, not a blocking syscall — so the deadline also lives in the token and is re-checked before every operation, and for `spawn` a watchdog arms SIGTERM→SIGKILL. On Windows, terminate the **Job Object**; `ade/src-tauri/src/runtime/jobobj.rs` already does kill-on-job-close and the code carries over.
- **Output bytes.** "Truncated at the ceiling, never buffered whole" means the broker never returns a `Vec<u8>` from a tool. Every reader is wrapped in `LimitedReader { inner, remaining }`, every stream in a `take_bytes` combinator that emits `Truncated` and closes. Process stdout is pumped by the broker, so the child gets backpressure and eventually EPIPE. `read` never calls `read_to_end`.
- **Memory.** Not enforceable per-call in-process — one heap, and a global allocator cannot attribute allocations to a call. The spec already scopes it to spawned processes; keep it there. Windows `JOB_OBJECT_LIMIT_PROCESS_MEMORY`; Linux cgroup v2 `memory.max` else `RLIMIT_AS` in `pre_exec`; macOS `RLIMIT_AS` is unreliable, so sample RSS via `sysinfo` and kill. **Best-effort on macOS — say so in the docs.**
- **Tokens and money** are not broker business: kernel accounting at `provider.after` from `Usage`. `maxUsd` becomes `max_micro_usd: u64` — no floats for money.

### Cancellation

`tokio_util::sync::CancellationToken`, one per turn, `child_token()` per pass, per tool call, per sub-agent branch. Chosen over a hand-rolled AbortSignal because it already gives the tree, `cancelled()` as a `select!` arm, and `run_until_cancelled`.

Turn token → `Provider::stream` (dropping the reqwest body aborts the request, so "stops costing money at once" is drop-driven as well as explicit) → the registry passes a child into `ToolHost::call`, which sends `$/cancel` over RPC **and** revokes the token nonce → orchestrator branch tasks hold children, so a cancelled parent closes every child branch as `Cancelled`, still in the tree.

Cancellation surfaces as `TurnOutcome::Cancelled` / `ToolResult::Cancelled`, never `Err`, so partial work is recorded. `CancelReason { User, Budget(BudgetKind), Parent, Shutdown }` — `AbortSignal.reason` has no direct equivalent and the audit needs it. Caveat: a blocking sqlite write cannot be cancelled; they are short, so let them finish and drop the result.

§5.4's rule holds: **cancellation stops work, it does not undo it.** A tool declaring `atomic: true` is reverted by the broker (write-temp-then-rename); everything else reports what it completed in the `tool.settled` outcome.

### Session persistence

**SQLite via rusqlite, bundled** — already in the ADE's tree and already shipped, so no new native dependency and one storage engine in the binary.

`journal_mode=WAL`, `synchronous=NORMAL`, **one transaction per turn append**: nothing lost on process crash, at most the last commit on OS crash — exactly the "≤1 turn" bar. `FULL` is a documented knob.

Tables: `sessions`, `branches(id, session, parent_branch, forked_at_turn, label, state)`, `turns(id, branch, seq, kind, payload BLOB, created_at, UNIQUE(branch, seq))`, `compactions(branch, upto_seq, summary_turn)`, `events`.

Replay is structural: turns are **immutable and append-only**, and `compact` writes a *new* summary turn plus a watermark row — it never mutates or deletes. `materialise` walks branch ancestry, applies the highest watermark, replays rows above it. The raw tree always replays; the compacted view is derived. That is what makes a failed eval case openable.

The writer is **one actor task per session** over an `mpsc` with `oneshot` replies — not `spawn_blocking` per call, which would serialise anyway and burn the blocking pool. Reads use a WAL read-only connection under `spawn_blocking`.

Rejected: **sled** (stalled pre-1.0, we would own crash recovery); **append-only files** (we would reimplement the ordered branch-ancestry index and torn-write handling); **redb** (genuinely good, but hand-rolled branch indexes and a second engine in the binary for no gain).

### Wire format

`#[serde(tag = "t")]` with explicit per-variant renames — `#[serde(rename = "session.create")]`; dotted names are not a `rename_all` rule. All variants are struct variants, so internal tagging is legal.

CBOR via `ciborium` when a client asks. **MessagePack is rejected**: it is not self-describing, and serde's internally-tagged enums fail to deserialize from non-self-describing formats. That is not a preference, it is a constraint.

Internal tagging buffers through serde's `Content` on deserialize. The kernel only *deserializes* `Request` (low volume) and *serializes* `Event` (high volume, unbuffered), so the cost lands in the right place.

`seq` is assigned once, per session, at the differ's output — never per connection — so replay and coalescing read the same numbering.

### Rust ↔ TypeScript

`schemars` derives on every `orrery-proto` type → `cargo xtask typegen` → `harness/protocol/protocol.schema.json` → `json-schema-to-typescript` → committed `protocol.d.ts`. CI runs typegen and fails on `git diff --exit-code`.

Chosen over `ts-rs`/`specta` because the JSON Schema is needed **anyway** — for `ToolDef` input validation, `ParamSchema<P>`, and §5.6's promise that a third party builds against a published spec. TypeScript is then a by-product rather than a second generator.

### Surfaces

The kernel diffs, not the client (§6.1). `orrery-surface` holds, per live surface id, the surface plus a blake3 hash per node, so an unchanged subtree is skipped in O(1).

Rules: discriminant change → `replace`. `text`/`stream`/`markdown` where `next.starts_with(prev)` → `append` with the suffix — the hot path, and the reason `append` exists. `stack` children matched by explicit `id` then by index, recursing. Leaf scalar change → `set { path }`. A **cost guard** caps it: if accumulated patch bytes exceed ~60% of a full `replace`, discard and emit `replace`. That is what stops patch storms on a re-sorted table.

Coalescing lives downstream in `orrery-transport`, **one coalescer per connected client** — two clients at different speeds must not share one — on a ~33 ms tick merging consecutive `append` on an id and collapsing `set`+`replace` to the last `replace`.

Surfaces **seal at `turn.settled`**: the store is per turn, and a later emit returns `Err(SurfaceSealed)` to the extension rather than a frame a client cannot apply.

### Clients

**Every client is an AG-UI subscriber, the in-binary one included.**

| Command | What happens |
|---|---|
| `orrery` | Kernel in-process → AG-UI encoder → in-process channel → ratatui. Still through the encoder; there is no shortcut path. |
| `orrery --ui ink` | Kernel listens (pipe + SSE), spawns `node clients/ink` with `ORRERY_ENDPOINT`. |
| `orrery serve` | Kernel only, prints the endpoint. Then `orrery attach <endpoint>` (ratatui) **and** the Ink client can sit on **one session at once**, each with its own coalescer. |
| `orrery run --json` | The json renderer: line-delimited AG-UI events. CI, evals, cross-harness adapters. |
| `orrery replay <session>` | Re-emits a stored session as AG-UI events, so any renderer can draw any past session. |

Sessions outlive clients (§5.3): the kernel keeps running when the terminal closes, and reattaching replays from the last `seq` the client saw.

**Renderer tests are data first, pixels second.** `clients/conformance/*.jsonl` scripts → `SurfaceStore` state assertions in `sdk-rs` and `sdk-ts` from the same fixtures; then drawing snapshots per core surface — ratatui `TestBackend` + `insta`, Ink `ink-testing-library` `lastFrame()` + vitest, json = the event lines diffed against the fixture.

### Isolation and live unload

wasm: one `Store` per instance, memory capped by `ResourceLimiter`, wall clock by `epoch_interruption`, **zero WASI preopens** — fs and proc arrive only as imported broker functions; fuel enabled only under the eval runner, for determinism. node/python/process: one child, Windows Job Object with kill-on-job-close, unix setsid plus process-group kill.

`unload(ext)` marks the instance `Draining`, fires its `CancellationToken`, waits a grace window, then kills. The registry holds `Arc<ExtInstance>` keyed by **generation id**, so a `ToolRef` resolved before the unload cannot resurrect a dead instance — it gets `ToolResult::Unloaded`, which is an outcome, not a session failure. That is the phase-2 acceptance criterion.

---

## Dependencies

**Reuse from the ADE's tree** (same versions, one resolution): `serde`, `serde_json`, `thiserror 2`, `rusqlite 0.40 bundled`, `uuid` (add `v7`), `blake3`, `sysinfo`, `windows-sys` (JobObjects), `libc`, `reqwest 0.13` + `rustls` with `ring`, `toml_edit`, `clap 4`, `regex`, `semver`, `sha2`, `tempfile` (dev).

> The ADE's `Cargo.toml` comment already forbids a second crypto stack. `tokio-rustls` inherits that setup; nothing else brings TLS.

**New, each for a reason:**

| Crate | Why |
|---|---|
| `tokio`, `tokio-util` | One runtime. `CancellationToken` gives the cancellation *tree*, which is the whole requirement. |
| `futures-core`, `futures-util` | `BoxStream` and byte-limit combinators, without all of `futures`. |
| `async-trait` | Object safety for the four `dyn` traits only. |
| `globset` | `*` / `**` / `prefix:*` with gitignore path semantics — §4.8 names them explicitly. |
| `dunce` | §4.8 demands UNC and drive-relative normalisation before matching. |
| `dashmap` | Branch registry and token ledger. Sharded, not one global lock. |
| `arc-swap` | Hot-swap the resolved `RuleSet` snapshot without blocking every `check`. |
| `parking_lot` | Sync mutexes on pure paths where `tokio::sync` would be a mistake. |
| `indexmap` | Deterministic `visible()` ordering — an unstable prompt prefix silently kills provider caching. |
| `bytes` | Zero-copy streamed tool output. |
| `getrandom` | Token nonce seed. |
| `tracing`, `tracing-subscriber` | A span per phase and step; the three §4.12 streams are three layers. OTLP feature-gated. |
| `schemars`, `jsonschema` | Generate the schema; validate `ParamSchema` at `session.start`. `jsonschema` feature-gated — heavy, load-time only. |
| `ciborium` | CBOR, self-describing (see wire format). |
| `interprocess` | One API over named pipes and UDS. The alternative is two `cfg` paths in a Windows-primary repo. |
| `axum` | The AG-UI HTTP/SSE listener. `tower` at the HTTP edge only. |
| `tokio-rustls` | TLS on the existing rustls/ring setup. |
| `wasmtime`, `wasmtime-wasi` ≥ 43 | The only production component-model runtime. Epoch interruption and `ResourceLimiter` *are* the wasm isolation story. |
| `wit-bindgen` | The guest SDK. |
| `toml` | Manifest deserialize (`toml_edit` for writes). |
| `serde_yaml_ng` | SKILL.md front matter. `serde_yaml` is unmaintained. |
| `ratatui`, `crossterm` | The default TUI. `insert_before` gives the §6.4 hybrid without a second screen model. |
| dev: `insta`, `proptest`, `tokio-test` | Snapshot the materialised context and the audit stream; property-test the matcher and budget arithmetic. |

**Rejected, with the reason:**

- `tower` **inside the loop** — our pipeline is policy-gated typed verdicts, not `Service<Req>` middleware, and `poll_ready` backpressure is exactly what §5.4 forbids from reaching the agent loop. It stays at the HTTP edge.
- `rmp-serde` — see wire format.
- `json-patch` — four ops, hand-written.
- `sled` / `redb` — see session persistence.
- `anyhow` in library crates — bins and tests only.
- The community AG-UI Rust crates — `sdks/community/rust` has no code owner and stalled PRs; the three unaffiliated crates are unmaintained. We only ever *produce* AG-UI, so the client half would be dead weight anyway. Vendor the event enum, pin the version, CI-diff against upstream's published schema.

---

## Where the spec does not survive contact with Rust

Each item is owned by a plan, which records the final shape.

| # | Spec says | Rust does | Owner |
|---|---|---|---|
| 1 | `Interceptor.run` is `async` but "performs no I/O" | The trait is **sync**, and `InterceptCtx` has no broker handle and no token. I/O becomes impossible by type rather than by convention. Remote (node/python) interceptors are the one async adapter, at the host boundary. | 05 |
| 2 | `rewrite: { value: unknown }` | `Verdict<P>` generic over the phase payload, so `Phase` is a trait with an associated type rather than a flat string enum — a tool input can no longer be rewritten into a model request. | 05 |
| 3 | ``Subject = "agent" \| `ext:${string}` `` | `enum Subject { Agent, Ext(ExtId), SubAgent(AgentName) }`, serialised back to the string form for config and wire compatibility. | 07 |
| 4 | `systemPrompt: string \| ((p) => string)` | A closure crosses no boundary. `enum SystemPrompt { Static(String), Template(String), Export { name: String } }`, the last resolved by calling back into the extension. | 06, 11 |
| 5 | `Expr` is untyped | Validate the whole workflow dataflow **at load**: step *N* may only `ref` steps `< N`, and refs must typecheck against the target's `returns`. A workflow must not fail mid-run after paying for three model calls. | 11 |
| 6 | `materialise(budget)` assumes token counting is free | A `TokenCounter` per provider; `maxContext` enforcement is approximate, so compaction keeps a safety margin or you get a rejected request *after* the round trip. | 02, 03 |
| 7 | `maxUsd: number` | `max_micro_usd: u64`. | 01 |
| 8 | `compact` is a phase | It is the one phase that does I/O (it calls the `compactor` role). Named exception: async, cancellable, charged to the turn budget. | 05 |
| 9 | `Surface` is recursive | WIT has no recursive types. Across the component boundary it is a **flat arena** — `list<surface-node>` + child indices — rebuilt host-side. **Settle this before phase 2 freezes the WIT.** | 14 |
| 10 | `ExtensionDefinition` | Split into `ExtensionManifest` (deserialize) and `ExtensionInstance` (runtime handles). `Contribution.kind` is TS `keyof` — needs a derive macro to stay in step. | 06 |
| 11 | `Partial<Grant>`, structural intersection | Explicit `Grant::intersect()` with `Option` fields. No structural typing, so every narrowing site is hand-written and must be property-tested. | 01 |
| 12 | Consent `deadlineMs` | Needs a kernel-owned monotonic clock, or `attach(since)` replays an already-expired prompt. | 08 |
| 13 | "fs is stripped from the isolate" (node) | A loader-hook trick in the TS SDK, not something Rust enforces. Rust can only withhold capabilities; a determined Node extension still has `require('fs')` unless the SDK loader is in place. **Say so in the threat model.** | 06 |
| 14 | `runtime: "wasm" \| "node" \| "python" \| "process"` | Add **`native`** for first-party Rust crates compiled in: same manifest, same registry → policy → broker dispatch, feature-gated in the facade. Without it phase 1 needs a privileged in-kernel path — exactly what §8 forbids. | 06 |
| 15 | `GraderDef` on `ExtensionDefinition` | Graders need a published trait crate `orrery-grader`, or a grader extension would depend on `orrery-eval` (kernel-internal, unpublished). | 16 |
| 16 | §6.4 "hybrid, built in React" | Two reference clients of one AG-UI stream: ratatui inside the binary (no Node at runtime, same language as the kernel) and React/Ink beside it as the third-party-client proof. §6.4's sharing argument does not hold here because the web/ADE client is Angular + kouji-ui. What is shared is the surface vocabulary and `protocol.d.ts`, enforced by one conformance suite all three run. | 09b, 09c |

Also flagged, not yet decided:

- **`json` in WIT is a `string`**, so `payload: unknown` and `input: unknown` double-encode across the wasm boundary. Measure on delta-heavy extensions before phase 2 ends (plan 14).
- **Wasm cancellation is coarse.** Epoch interruption traps at loop backedges and cannot cleanly unwind a guest blocked inside a host import. `turn.cancel` against a wasm tool is "trap and discard the Store", not a graceful abort (plan 14).
- **WASI 0.3 is RC-tracking.** Target **p2** for phase 2; treat p3 async as a later migration. The contract must stop moving before the runtime does.

---

## Milestones

§8's phases, mapped onto this repo.

| Phase | Plans | Done when |
|---|---|---|
| **docs** | this directory | Every crate in the tables above is owned by exactly one plan; every architecture section is covered by one. |
| **0** | `00a`, then `00b` | The old app builds and runs from `ade/`; `cargo test` at root builds only harness crates, all empty but compiling; `cargo run -p orrery-cli -- --help` prints; `cargo xtask deps-check` is green. |
| 1 | 01, 02, 03, 05, 06 (builtin only), 08, 09/09b/09c (minimal), 17 | `orrery run -p "list files here"` completes one real turn with a tool call in a terminal. **And** `orrery serve --provider fixture:turn.jsonl` with ratatui and Ink attached to the same session both render the fixture turn; the `SurfaceStore` conformance tests pass in Rust and TS. |
| 2 | 04, 06 (rpc + node), 14, 18 | Two extensions claiming `search` both work; killing one leaves the session alive. |
| 3 | 07 | An extension denied `spawn` degrades instead of failing; every decision is logged. |
| 4 | 08 (pipes/TLS), 09, 09b, 09c | Three ported extensions render in **both** TUIs with no drawing code of their own; both pass the conformance suite. **The porting is done** (plan 09, task 8): `workspace-census`, `patch-review` and `release-train` in `extensions/examples/`, run through `orrery-host` by `clients/ported` and snapshotted in ratatui, Ink and `--json`. The escape hatch it found was ours: nine of the twelve surface builders were in a `publish = false` crate. **Amended 2026-09-19: exercisable through the binary, which is what made it read PARTIAL.** Nothing in `orrery-cli` or `orrery-harness` named the three crates, so `orrery ext list` never listed them and no turn could call one — the criterion was true of `orrery-ported`'s own tests and untestable through the product. They are now compiled in behind `example-extensions` (off by default: a shipped binary carries no demos) and `orrery-cli/tests/ported.rs` drives the built binary — `ext list` names all three, one `run --json` turn renders all three surfaces, and the same turn drawn by `attach --ui ratatui` shows the census table, the diff with its question and the release timeline. Compiled in rather than discovered on disk because `features::host_for` answers `None` for `native`, and that answer is correct: a native extension is linked, not loaded. |
| 5 | 10 | Two profiles produce measurably different agents from one binary. |
| 6 | 11, 12 | A verify loop terminates on its own cap, budget enforced by the kernel. |
| 7 | 13 | An existing `SKILL.md` and an existing MCP server both work unmodified. **Amended 2026-09-19: true of a turn now, not only of an inspection command.** `orrery mcp tools fixture` did a real handshake and listed `mcp.fixture.echo` while a turn calling it answered `no-such-tool`, because `orrery-cli` depended on `orrery-mcp` and `orrery-skills` and `orrery-harness` — which builds what a turn runs — depended on neither. The run path now registers declared servers as ordinary `mcp.<server>` extensions in the ordinary registry behind the ordinary gate, connect-on-need, with `list_changed` growth refused **and recorded**; discovered `SKILL.md`s reach section 4 of the same turn's system prompt. Proof drives the binary: `orrery-cli/tests/mcp_turn.rs`. |
| 8 | 15 | An admin pins a version set and unpinned extensions refuse to load. **Both halves, since round 7: `orrery registry init\|add\|sign\|verify` makes the pin set, and managed `unpinned = "refuse"` enforces it. Publishing an index over a network is out of this build.** |
| 9 | 16 | One suite runs against two profiles and one competing harness, same graders, reproducible cost numbers. |
| 10 | 06 (python/process) | An existing internal service works as an extension with no rewrite. |

**Phase 2 is the real milestone** — where the thesis is proven or falsified, and small enough to reach quickly. **Phase 4 is the honest test of the surface vocabulary**: port two or three existing extensions, and if they need escape hatches the schema is wrong. *Run.* Three ported extensions needed no variant the vocabulary does not have — the one thing that was genuinely missing, a timeline, is what `custom` is for. The escape hatches were on our side of the line: the builders for nine of the twelve surfaces were unpublished, and there was no way for an extension to mint the id it re-emits under. Both are fixed; plan 09's task 8 carries the full list, including two client bugs the exercise found.

## Conventions

- **Never run a full test suite.** `cargo test -p <crate>` for the crate you touched and the crate whose subject you changed. `cargo check` freely.
- `cargo` runs from the repo root; `default-members` excludes the ADE.
- Never two test runners at once.
- Commits wait for review.
- Each plan's open questions get answered **in the plan file** when the executor decides them.

---

## State

**Living document, current as of 2026-09-19.** This file is the map rather than
a wave of work, so it has no task list to tick. What it records is a shape, and
the shape has held: `core/` · `extensions/` · `clients/` · `xtask/` · `wit/` ·
`protocol/`, with the dependency direction enforced mechanically by
`cargo xtask deps-check` rather than by agreement.

Two of its claims have been tested by the rounds since and are worth stating as
outcomes rather than intentions:

- **The crate table is real.** 37 of the 44 crates it names are reachable from
  the shipped `orrery` binary, which `orrery-cli/tests/reachable.rs` asserts by
  driving `CARGO_BIN_EXE_orrery` rather than the libraries.
- **The dependency rule survived contact.** The two exceptions this file grants
  — `orrery-harness` as the facade and `orrery-cli` as a binary — are still the
  only two, and `deps-check` fails rather than warns when a third appears.

The overview is amended in place as decisions land; the per-plan `## State`
sections carry the wave-by-wave record.
