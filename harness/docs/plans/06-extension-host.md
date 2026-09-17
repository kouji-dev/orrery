# 06 · Extension host — the boundary, the manifest, the ledger

**Goal.** The contract that cannot be changed later without breaking every extension written against it. A manifest, four runtimes (one of which is "compiled in"), a load ledger that reports what loaded, degraded, failed or was skipped, live unload that does not kill the session, and a first-party tool bundle that proves the API carries real work. When this is done, two extensions claiming `search` both work and killing one leaves the session alive.

**Covers.** §4.7 in full · §4.13's manifest half (wasm is plan 14) · the `native` runtime (translation #14).

**Crates.** `core/crates/orrery-ext-api` (published) · `core/crates/orrery-host` · `core/crates/orrery-jsonrpc` · `core/crates/orrery-host-rpc` · `extensions/crates/orrery-ext-tools-builtin` · `extensions/node/ext-sdk` (`@orrery/ext`).

**Depends on.** [`01`](01-proto-shared-types.md). Uses [`04`](04-tool-registry.md) for dispatch and [`07`](07-policy-broker-audit.md) for the broker; both can be stubbed with typed no-ops while this lands.

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **The field list is the design rule.** If a subsystem can be replaced, it is a field on the manifest; if it is not a field, it is ours. §7's fixed-vs-customizable table and this interface are two views of one decision.
- **The loop is not replaceable.** A genuinely different architecture embeds the kernel as a library; it does not replace the loop.
- Collections merge across extensions; singletons resolve by layer precedence and the loser is named in the ledger.
- Failure after load is as defined as failure during it: a collection member degrades, a singleton falls back — **except** a `PermissionHandler` that panics fails **closed**, and a `SessionStore` failure **ends the session**.
- Generation ids: a stale `ToolRef` gets `Outcome::Unloaded`, never a session failure.
- Extensions are publishable crates; the mock broker they test against is the one `ext test` uses.

This plan owns translations **#4** (`SystemPrompt`), **#10** (manifest/instance split), **#13** (the node threat-model caveat) and **#14** (the `native` runtime).

---

## Architecture

### The manifest

TOML on disk, `ExtensionManifest` in memory. This is the contract — nothing outside it is available at runtime.

```toml
[extension]
api     = "orrery-ext/1"            # a major it does not know is refused at load
name    = "buildgraph"
version = "1.2.0"
runtime = "native"                  # native | wasm | node | python | process

[provides]
tools   = ["impacted", "deps"]      # → buildgraph.impacted, buildgraph.deps
# providers | agents | workflows | interceptors | lifecycle | graders
# commands | views | renderers | skills | mcp
# singletons: memory | session | permissions | router

[requires]
read  = ["$WORKSPACE/**"]
spawn = ["java"]
net   = false

[process]                           # runtime = "process" only
command  = "./bin/buildgraph-server"
protocol = "orrery-ext/1"
```

```rust
// Translation #10: deserialize target vs runtime handles are different types.
pub struct ExtensionManifest {
    pub api: ApiVersion, pub name: ExtId, pub version: semver::Version,
    pub runtime: RuntimeKind,
    pub provides: Provides,
    pub requires: Vec<Capability>,
    pub process: Option<ProcessSpec>,
}

pub struct ExtensionInstance {
    pub manifest: Arc<ExtensionManifest>,
    pub generation: Generation,          // monotonic; makes a stale ref unresolvable
    pub state: InstanceState,            // Loading | Live | Degraded | Draining | Dead
    host: Arc<dyn ExtensionHost>,
    cancel: CancellationToken,
}
```

`Provides` mirrors the `ExtensionDefinition` field list exactly. A derive macro generates `Contribution::kind` from it so the two cannot drift — the TS original used `keyof`, which Rust has no equivalent for.

### The host trait

```rust
#[async_trait]
pub trait ExtensionHost: Send + Sync {
    fn runtime(&self) -> RuntimeKind;
    async fn load(&self, m: Arc<ExtensionManifest>, grant: Grant) -> LoadOutcome;
    async fn call(&self, ext: &ExtId, tool: &str, input: serde_json::Value, ctx: CallCtx)
        -> Result<Outcome, HostError>;
    async fn unload(&self, ext: &ExtId) -> Result<(), HostError>;   // live, no session restart
}

#[non_exhaustive]
pub enum LoadOutcome {
    Loaded   { contributes: Vec<Contribution>, ms: u64 },
    Degraded { contributes: Vec<Contribution>, disabled: Vec<String>, reason: String },
    Failed   { reason: String, stage: LoadStage },     // resolve | manifest | init
    Skipped  { reason: SkipReason },                   // policy | disabled | unsigned
}
```

`LoadOutcome` is queryable over RPC (`query { of: "ledger" }`), rendered by any client, and written to the audit stream at session start.

### The call context

What an extension gets. Everything routes back through the broker; nothing here is a raw handle.

```rust
pub struct CallCtx {
    pub call: CallId, pub ext: ExtId,
    pub budget: ToolBudget,
    pub cancel: CancellationToken,
    pub broker: Arc<dyn BrokerFacade>,   // fs / proc / net / creds, each policy-checked
    pub ui: SurfaceSink,                 // ctx.ui.table(..) — describes, never draws
}
```

`ctx.ui.table(...)` describes a table rather than drawing one: ratatui renders a widget, Ink renders a component, `--json` emits the payload — from the same description.

### The four runtimes

| Runtime | How it loads | Isolation | Phase |
|---|---|---|---|
| `native` | A first-party crate registered at build time behind a cargo feature. Same manifest, same dispatch, same ledger. | Process-shared. **Only the process boundary is missing** — no fd, no secret, still policy-checked and budgeted. | 1 |
| `node` | Spawn `node`, JSON-RPC over stdio, `@orrery/ext` SDK on the other side. | Child process, Job Object / process group. | 2 |
| `wasm` | Plan 14. | Store, ResourceLimiter, epoch, zero preopens. | 2 |
| `python`, `process` | Same RPC path as node, different argv. | Child process. | 10 |

**Why `native` exists** (translation #14): §8 decided the built-in tools ship as a first-party bundle so no privileged in-kernel path exists. Without a `native` runtime, phase 1 would need exactly that path. With it, `builtin.read` goes registry → policy → broker like everything else, and the only difference from a community extension is where the code was compiled.

### JSON-RPC

`orrery-jsonrpc` is a new crate, but the framing is not new code:

- **Move `ade/src-tauri/src/lsp/transport.rs` verbatim** into `framing::content_length`. It is 65 lines of correct, well-tested `Content-Length` framing with split-read, EOF and garbage-header coverage. Add a `LineDelimited` variant — MCP stdio is newline-delimited JSON, not LSP framing, and one enum serves both.
- **Rewrite the client.** `lsp/client.rs` is synchronous `recv_timeout` request/response on two OS threads per server. Three reasons it cannot carry this: it cannot stream a `delta`; it cannot hold a long-lived guest→broker callback; it has no per-call cancellation, and `turn.cancel` must reach one in-flight tool call, not the connection. The pending-map demux, the stderr ring and the error shape carry over as design.

### Live unload

```
Live ──unload()──> Draining ──grace elapsed──> Dead
                      │
                      └─ cancel token fired; in-flight calls settle as Cancelled
```

The registry holds `Arc<ExtensionInstance>` keyed by **generation**. A `ToolRef` resolved before the unload cannot resurrect a dead instance — dispatch returns `Outcome::Unloaded { ext }`. That is a `tool.settled` outcome the model can re-plan around, not a session failure. This is the phase-2 acceptance criterion.

### Testing an extension needs no model

```rust
// orrery-ext-api::testing — the same code path `orrery ext test` uses.
pub struct MockBroker { grants: Vec<Capability>, recorded: Vec<BrokerCall>, responses: .. }
pub fn load_for_test(manifest: &str, grants: &[&str]) -> TestHarness;
```

The author sees the same ledger and the same denials a real session produces. A community author has this because it ships in a published crate.

### The first-party tool bundle

`extensions/crates/orrery-ext-tools-builtin`, ids `builtin.read`, `builtin.write`, `builtin.edit`, `builtin.bash`, `builtin.grep`, `builtin.glob`.

Every one goes through the broker, which means §4.5's objective-5 complaint is answered concretely: **`builtin.read` never calls `read_to_end`.** It asks the broker for a `LimitedReader` and streams to the ceiling. The same for `grep` (ripgrep's `grep-searcher`, already in the ADE's tree) and `bash` (pumped stdout, EPIPE on the child).

`builtin.write` and `builtin.edit` declare `atomic: true` so the broker reverts them on cancel (write-temp-then-rename).

### The node SDK

`extensions/node/ext-sdk`, published as `@orrery/ext`:

```ts
export default defineExtension({
  tools: {
    impacted: {
      description: "Modules impacted by the current diff",
      input: z.object({ since: z.string().default("HEAD~1") }),
      async run({ since }, ctx) {
        const out = await ctx.proc.run("java", ["-jar","bg.jar","--since",since], { timeoutMs: 30_000 });
        return ctx.ui.table({ columns: ["module","reason"], rows: parse(out.stdout) });
      },
    },
  },
});
```

**Threat-model note (translation #13).** The TS SDK strips `fs` and `child_process` from the isolate with a loader hook. That is an SDK convenience, **not** a Rust guarantee: the host can only withhold capabilities. A determined Node extension still has `require('fs')` unless the loader is in place, and it runs with the harness process's OS privileges. Say this in `extensions/node/ext-sdk/README.md` and in the security docs. The wasm path is the one with a real boundary.

---

## File structure

**Create**

- `harness/core/crates/orrery-ext-api/src/{lib,manifest,instance,tool,ctx,ledger,contribution,testing}.rs`
- `harness/core/crates/orrery-ext-api/tests/manifest.rs`
- `harness/core/crates/orrery-host/src/{lib,host,table,native,state,unload}.rs`
- `harness/core/crates/orrery-host/tests/{lifecycle,unload}.rs`
- `harness/core/crates/orrery-jsonrpc/src/{lib,framing,client,server,cancel}.rs`
- `harness/core/crates/orrery-host-rpc/src/{lib,spawn,node,contain}.rs`
- `harness/extensions/crates/orrery-ext-tools-builtin/{Cargo.toml,orrery.toml,README.md,src/*}`
- `harness/extensions/node/ext-sdk/{package.json,README.md,src/*}`

**Move**

- `ade/src-tauri/src/lsp/transport.rs` → `orrery-jsonrpc/src/framing/content_length.rs` (copy; the ADE keeps its own until the ADE is a client)

---

## Tasks

### Task 1 · The manifest

Files: `orrery-ext-api/src/manifest.rs`, `tests/manifest.rs`

- [ ] **Failing test first.** `manifest::unknown_major_is_refused` — `api = "orrery-ext/2"` fails at load with `LoadStage::Manifest`.
- [ ] `manifest::provides_round_trips` — every field of `Provides` parses from TOML.
- [ ] `manifest::requires_becomes_capabilities` — `read = ["$WORKSPACE/**"]` becomes `Capability { aspect: Read, scope: [...] }`.
- [ ] `manifest::process_required_for_process_runtime` — `runtime = "process"` without `[process]` is an error naming the file.
- [ ] Implement `ExtensionManifest`, `Provides`, `RuntimeKind`, `ProcessSpec`, `ApiVersion`.
- [ ] Derive macro for `Contribution::kind` from `Provides` fields.

### Task 2 · Instance table and state machine

Files: `orrery-host/src/{table,state}.rs`, `tests/lifecycle.rs`

- [ ] **Failing test first.** `lifecycle::degraded_keeps_the_rest` — an extension whose `spawn` grant is denied loads `Degraded`, the tool needing spawn is disabled, its other tools still dispatch.
- [ ] `lifecycle::singleton_conflict_resolves_by_layer` — two extensions claiming `router`; the closer layer wins, the loser is in the ledger.
- [ ] `lifecycle::collection_member_failure_disables_only_itself`.
- [ ] Implement `ExtensionInstance`, `Generation`, `InstanceState`, the table.

### Task 3 · The `native` runtime

Files: `orrery-host/src/native.rs`

- [ ] **Failing test first.** `native::goes_through_dispatch` — a native extension's tool call is observable in the audit as a normal dispatch, with a policy check, not a direct function call.
- [ ] `native::manifest_is_the_same_shape` — the builtin bundle's `orrery.toml` parses with the same parser a third-party one does.
- [ ] Implement registration (a `NativeRegistry` the facade fills behind cargo features).

### Task 4 · The builtin tool bundle

Files: `orrery-ext-tools-builtin/src/*`

- [ ] **Failing test first.** `builtin::read_respects_the_output_ceiling` — a 10 MB file with a 4 KB ceiling returns `Outcome::Truncated`, and (the important half) **peak memory stays bounded** — assert via a counting reader that no more than the ceiling plus a small buffer was ever read.
- [ ] `builtin::write_is_atomic` — cancel mid-write; the original file is intact and no temp file remains.
- [ ] `builtin::bash_is_contained` — a child that spawns a grandchild; kill the call; assert both are gone (Job Object on Windows, process group on unix).
- [ ] `builtin::grep_streams` — a large tree, a ceiling, bounded memory.
- [ ] Implement all six tools, each declaring its `ToolDef` schema and `atomic` flag.

### Task 5 · JSON-RPC

Files: `orrery-jsonrpc/src/*`

- [ ] Copy `transport.rs` in; keep its tests; add `LineDelimited` and a test that a newline-delimited stream parses.
- [ ] **Failing test first.** `client::concurrent_calls_demux` — three in-flight requests reply out of order; each caller gets its own reply.
- [ ] `client::cancel_reaches_one_call` — cancel call 2; calls 1 and 3 complete; a `$/cancel` notification was sent for 2 only.
- [ ] `client::server_initiated_request` — the guest calls back into the broker and gets a reply.
- [ ] `client::stderr_is_ringed` — a chatty child does not grow memory without bound.
- [ ] Implement the async client and server halves.

### Task 6 · The RPC host (node)

Files: `orrery-host-rpc/src/*`

- [ ] **Failing test first.** `rpc::child_dies_with_us` — spawn a node extension, kill the harness process, assert the child is gone.
- [ ] `rpc::crash_degrades_not_kills` — the child exits mid-call; the call settles `Failed`, the extension is `Degraded`, the session lives.
- [ ] Implement spawn, containment (reuse `ade/src-tauri/src/runtime/jobobj.rs`), the manifest `[process]` path.

### Task 7 · Live unload

Files: `orrery-host/src/unload.rs`, `tests/unload.rs`

- [ ] **Failing test first, and it is the phase-2 criterion.** `unload::killing_one_leaves_the_session_alive` — load two extensions both providing `search`; unload one mid-session; assert the other still dispatches, a stale ref to the dead one returns `Outcome::Unloaded`, and no turn failed.
- [ ] `unload::in_flight_calls_settle_cancelled`.
- [ ] `unload::grace_then_kill` — a child that ignores the cancel is killed after the grace window.
- [ ] Implement the `Draining` state machine.

### Task 8 · The mock broker and `ext test`

Files: `orrery-ext-api/src/testing.rs`

- [ ] **Failing test first.** `testing::denial_is_observable` — a test declaring no `spawn` grant; the extension's call returns `Denied` and the mock records it.
- [ ] Implement `MockBroker`, `load_for_test`, recorded calls, canned responses, surface assertions as data.
- [ ] Wire `orrery ext test` to it (the command lands in plan 17).

### Task 9 · The node SDK

Files: `extensions/node/ext-sdk/*`

- [ ] `defineExtension`, the `ctx` shape (`ctx.proc`, `ctx.fs`, `ctx.net`, `ctx.creds`, `ctx.ui`), the JSON-RPC client half, the loader hook that strips `fs`/`child_process`.
- [ ] **The README states the threat-model caveat** (translation #13) in its own section. Do not bury it.
- [ ] One worked example extension under `extensions/examples/node-hello/`.

---

## Done when

- `cargo test -p orrery-ext-api -p orrery-host -p orrery-jsonrpc -p orrery-host-rpc -p orrery-ext-tools-builtin` green.
- Two extensions claiming `search` coexist as `a.search` and `b.search`; unloading one leaves the session alive.
- `builtin.read` on a huge file demonstrably does not buffer it.
- `orrery ext test` runs an extension with no model and no network.

## Open questions

1. **Does `renderers` belong on the manifest in phase 2?** §8 decided custom renderers are allowed for `tui` and `web`. The field can exist and be unhandled until plan 09c. Prefer declaring it now so the manifest does not gain a field later — but confirm.
2. **Grant diff UI at install.** §4.7 shows a terminal prompt. It should be a `Surface` so every client renders it. Which plan owns it — here, or plan 15 (`ext install`)? Suggest here, since `ext test` needs the same shape.
3. **`api = "orrery-ext/1"` compatibility policy.** What is a breaking change to the manifest, concretely? Write the rule down before the first third-party extension exists, or it will be decided by accident.
4. **Node SDK `fs` stripping** — is it worth shipping at all, given it is not a guarantee? Argument for: it catches honest mistakes and makes the brokered path the path of least resistance. Argument against: it implies a boundary that is not there. Recommend shipping it *with* the README caveat.
