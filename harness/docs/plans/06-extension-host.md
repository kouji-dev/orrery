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

- [x] **Failing test first.** `manifest::unknown_major_is_refused` — `api = "orrery-ext/2"` fails at load with `LoadStage::Manifest`.
- [x] `manifest::provides_round_trips` — every field of `Provides` parses from TOML.
- [x] `manifest::requires_becomes_capabilities` — `read = ["$WORKSPACE/**"]` becomes `Capability { aspect: Read, scope: [...] }`.
- [x] `manifest::process_required_for_process_runtime` — `runtime = "process"` without `[process]` is an error naming the file.
- [x] Implement `ExtensionManifest`, `Provides`, `RuntimeKind`, `ProcessSpec`, `ApiVersion`.
- [x] Derive macro for `Contribution::kind` from `Provides` fields.

### Task 2 · Instance table and state machine

Files: `orrery-host/src/{table,state}.rs`, `tests/lifecycle.rs`

- [x] **Failing test first.** `lifecycle::degraded_keeps_the_rest` — an extension whose `spawn` grant is denied loads `Degraded`, the tool needing spawn is disabled, its other tools still dispatch.
- [x] `lifecycle::singleton_conflict_resolves_by_layer` — two extensions claiming `router`; the closer layer wins, the loser is in the ledger.
- [x] `lifecycle::collection_member_failure_disables_only_itself`.
- [x] Implement `ExtensionInstance`, `Generation`, `InstanceState`, the table.

### Task 3 · The `native` runtime

Files: `orrery-host/src/native.rs`

- [x] **Failing test first.** `native::goes_through_dispatch` — a native extension's tool call is observable in the audit as a normal dispatch, with a policy check, not a direct function call.
- [x] `native::manifest_is_the_same_shape` — the builtin bundle's `orrery.toml` parses with the same parser a third-party one does.
- [x] Implement registration (a `NativeRegistry` the facade fills behind cargo features).

### Task 4 · The builtin tool bundle

Files: `orrery-ext-tools-builtin/src/*`, `orrery-harness/tests/builtin.rs`

> **Where the tests live, and why not here.** `cargo xtask deps-check` rule 3
> forbids an extension crate from dev-depending on an unpublished core crate, so
> that a community author runs the identical suite. `orrery-broker` is exactly
> that, and these tests need the real one. They live in
> `orrery-harness/tests/builtin.rs`, which may name both halves - and are better
> for it: the facade under test is `PolicyBroker`, the one the harness wires in
> production, rather than a rig written for the tests.
> `orrery-ext-tools-builtin` has no dev-dependencies at all.

> Deferred out of wave 2 and **landed in wave 3**, for the reason it was
> deferred: every test in this task is a test *of the broker* — an output
> ceiling that bounds peak memory, a write that reverts on cancel, a child
> whose grandchild dies with it — and against a stubbed broker those four
> assertions would have been assertions about the stub. `orrery-broker` landed
> first, so the rig in `tests/common/mod.rs` wires the real policy engine to
> the real `LocalBroker` and the assertions mean what they say.

- [x] **Failing test first.** `builtin::read_respects_the_output_ceiling` — a 10 MB file with a 4 KB ceiling returns `Outcome::Truncated`, and (the important half) **peak memory stays bounded** — assert via a counting reader that no more than the ceiling plus a small buffer was ever read.
- [x] `builtin::write_is_atomic` — cancel mid-write; the original file is intact and no temp file remains.
- [x] `builtin::bash_is_contained` — a child that spawns a grandchild; kill the call; assert both are gone (Job Object on Windows, process group on unix).
- [x] `builtin::grep_streams` — a large tree, a ceiling, bounded memory.
- [x] Implement all six tools, each declaring its `ToolDef` schema and `atomic` flag.

### Task 5 · JSON-RPC

Files: `orrery-jsonrpc/src/*`

- [x] Copy `transport.rs` in; keep its tests; add `LineDelimited` and a test that a newline-delimited stream parses.
- [x] **Failing test first.** `client::concurrent_calls_demux` — three in-flight requests reply out of order; each caller gets its own reply.
- [x] `client::cancel_reaches_one_call` — cancel call 2; calls 1 and 3 complete; a `$/cancel` notification was sent for 2 only.
- [x] `client::server_initiated_request` — the guest calls back into the broker and gets a reply.
- [x] `client::stderr_is_ringed` — a chatty child does not grow memory without bound.
- [x] Implement the async client and server halves.

### Task 6 · The RPC host (node)

Files: `orrery-host-rpc/src/*`

- [x] **Failing test first.** `rpc::child_dies_with_us` — spawn a node extension, kill the harness process, assert the child is gone.
- [x] `rpc::crash_degrades_not_kills` — the child exits mid-call; the call settles `Failed`, the extension is `Degraded`, the session lives.
- [x] Implement spawn, containment (reuse `ade/src-tauri/src/runtime/jobobj.rs`), the manifest `[process]` path.

### Task 7 · Live unload

Files: `orrery-host/src/unload.rs`, `tests/unload.rs`

- [x] **Failing test first, and it is the phase-2 criterion.** `unload::killing_one_leaves_the_session_alive` — load two extensions both providing `search`; unload one mid-session; assert the other still dispatches, a stale ref to the dead one returns `Outcome::Unloaded`, and no turn failed.
- [x] `unload::in_flight_calls_settle_cancelled`.
- [x] `unload::grace_then_kill` — a child that ignores the cancel is killed after the grace window.
- [x] Implement the `Draining` state machine.

### Task 8 · The mock broker and `ext test`

Files: `orrery-ext-api/src/testing.rs`

- [x] **Failing test first.** `testing::denial_is_observable` — a test declaring no `spawn` grant; the extension's call returns `Denied` and the mock records it.
- [x] Implement `MockBroker`, `load_for_test`, recorded calls, canned responses, surface assertions as data.
- [x] Wire `orrery ext test` to it (the command lands in plan 17). *(Landed in plan 17: `orrery ext test [path]` reads a manifest, loads it through `load_for_test` against the grants the manifest itself asks for, prints what it contributes and how many broker calls it made, and exits 0 / 1 / 2. `orrery ext list` reports the compiled-in set through `testing::missing`, the same function the host calls. Neither builds a kernel, opens a database or asks a provider for anything.)* **Amended, round 5:** two halves of that were wrong in a way that made the command misleading. `ext list` reported the compiled-in set **and nothing else**, so installing an extension and then running the one command named after listing extensions did not show it; it now also reports plan 10's discovered set, each line naming the layer that contributed it. And `ext test` took only a *path*, so `orrery ext test builtin` failed with `could not read builtin\orrery.toml` — a compiled-in bundle has no manifest on disk to point at, which made the one runtime that ships in every build the one runtime the test harness could not reach. A name is now a way in (compiled-in first, then installed), via `testing::load_parsed_for_test`. Asserted through the binary in `orrery-cli/tests/ext.rs`.

### Task 9 · The node SDK

Files: `extensions/node/ext-sdk/*`

> The wire format is pinned by a hand-written guest
> (`orrery-host-rpc/tests/fixtures/echo-ext/`) that deliberately shares no code
> with the SDK — so the protocol is provably implementable from the spec alone,
> which is the property an SDK cannot establish about itself. The SDK is checked
> against the same host from the other side: `orrery-host-rpc/tests/sdk.rs`
> loads `extensions/examples/node-hello`, which is built on the SDK, through the
> real `RpcHost`.
>
> **It ships as JavaScript with hand-written `.d.ts`, not as compiled
> TypeScript.** There is no build step and no dependency, so what a reader sees
> is what runs, and `pnpm install` is not on the path between a clone and a
> passing test. The plan's `z.object(...)` is a plain JSON Schema for the same
> reason: zod would be the package's first dependency.

- [x] `defineExtension`, the `ctx` shape (`ctx.proc`, `ctx.fs`, `ctx.net`, `ctx.creds`, `ctx.ui`), the JSON-RPC client half, the loader hook that strips `fs`/`child_process`.
- [x] **The README states the threat-model caveat** (translation #13) in its own section. Do not bury it.
- [x] One worked example extension under `extensions/examples/node-hello/`.
- [x] `python` and `process` (section 8's phase 10): the same RPC path, a different argv. `orrery-host-rpc/tests/runtimes.rs` starts a hand-written python guest by the `python main.py` convention, and an "existing internal service" through its manifest's `[process]` table with no rewrite and no Orrery-shaped code.

---

## Done when

- `cargo test -p orrery-ext-api -p orrery-host -p orrery-jsonrpc -p orrery-host-rpc` green, and the builtin bundle's own suite with `-p orrery-harness --test builtin` (see Task 4). `builtin::bash_is_contained` needs its example built first: `cargo build -p orrery-harness --example contain_probe`.
- The node SDK's own suite is `node --test "test/*.test.mjs"` in `extensions/node/ext-sdk`, and it is checked against the host by `-p orrery-host-rpc --test sdk`.
- Two extensions claiming `search` coexist as `a.search` and `b.search`; unloading one leaves the session alive. **Proved from the binary, round 6.** The
  library had always had this; what it did not have was evidence that two
  extensions *on disk* reach a turn, and the acceptance run's evidence turned
  out to be two extensions that appeared in `ext list` and never loaded.
  `orrery-cli/tests/node_extensions.rs::two_extensions_claim_search_and_killing_one_leaves_the_session_alive`
  installs two node extensions that both provide `search`, calls `alpha.search`
  — which answers and then exits its own process — and then calls
  `beta.search` in the same session, which answers.
- `builtin.read` on a huge file demonstrably does not buffer it —
  `builtin::read_respects_the_output_ceiling` reads 10 MB under a 4 KB ceiling
  and asserts, through `LimitedReader`'s own pull counter, that no more than the
  ceiling plus one 8 KB buffer was ever pulled from the file.
- `orrery ext test` runs an extension with no model and no network. **True as
  of plan 17.** The command is wired to `orrery_ext_api::testing::load_for_test`
  — the mock broker, the recorded calls, the real ledger — and
  `ext::test_runs_without_a_model` in `orrery-cli` runs it on a fixture
  extension with no `--provider`, no key and nothing to reach.
  `ext::a_broken_manifest_is_usage` pins the other half: a manifest that will
  not parse is exit 2 naming the file, never a panic.
- **…and what it reports is what the run path would do.** Amended round 6: both
  `ext list` and `ext test` answered from the manifest alone, so a
  `runtime = "native"` extension discovered on disk — which `features::host_for`
  skips, on purpose, because a native bundle is compiled in — was printed `ok`
  while the session logged it as skipped. Both now ask
  `orrery_harness::plan::skip_for`, the same call the builder makes, and a skip
  is a ledger entry rather than a log line. `ext test` additionally starts the
  guest through the real host for any runtime this build has one for, so an
  extension that parses and then cannot activate is not a pass.
- **The node worked example is installable.** Amended round 6: it imported the
  SDK by a relative path that resolves only in this tree, and nothing put the
  SDK beside an installed copy, so `orrery install ./node-hello` produced an
  extension whose first turn died with `ERR_MODULE_NOT_FOUND`. `@orrery/ext` now
  travels inside the binary (`orrery_host_rpc::sdk`) and `RpcHost::install`
  vendors it into `node_modules/@orrery/ext` beside whatever directory it is
  told to run. The example imports `@orrery/ext` like a published extension.

## Open questions

1. **Does `renderers` belong on the manifest in phase 2?** §8 decided custom renderers are allowed for `tui` and `web`. The field can exist and be unhandled until plan 09c. Prefer declaring it now so the manifest does not gain a field later — but confirm.

   **Decided: yes, declare it now.** `renderers` is a field of `Provides` and maps to `ContributionKind::Renderer`, so a manifest that declares one parses, appears in the ledger and is reported to `query extensions` today. Nothing consumes it until plan 09c. The reason is the one the question suggests: `api = "orrery-ext/1"` is a promise about the manifest's shape, and adding a field to it later is a change every extension author has to read about. Adding a *handler* for a field that was always there is not.

2. **Grant diff UI at install.** §4.7 shows a terminal prompt. It should be a `Surface` so every client renders it. Which plan owns it — here, or plan 15 (`ext install`)? Suggest here, since `ext test` needs the same shape.

   **Decided: here.** `orrery_host::consent::grant_diff(manifest, granted)` returns `Option<Surface>` — a `Stack` of a `Table` (aspect, scope, already-granted-or-NEW) and a `Question` with allow / allow-once / deny. `None` when the grant already covers the manifest, because a prompt with nothing in it is worse than no prompt. The question carries **no `default` and no `deadline_ms`**: a capability grant is not something to time out into. Plan 15 wires it to `ext install` and plan 07 turns the answer into a stored grant; neither invents a second shape for the same question, which is exactly what would have happened had the shape lived in the installer.

3. **`api = "orrery-ext/1"` compatibility policy.** What is a breaking change to the manifest, concretely? Write the rule down before the first third-party extension exists, or it will be decided by accident.

   **Decided.** The major in `api = "orrery-ext/<major>"` is bumped if and only if a manifest that was valid stops being valid, or keeps parsing and means something different. Concretely:

   **Breaking — needs a new major:**
   - removing or renaming a field of `[extension]`, `[provides]`, `[requires]` or `[process]`;
   - narrowing what a field accepts (a new validation rule that refuses input that used to load);
   - changing what a field *means* — e.g. `requires.read` ceasing to be glob-matched;
   - removing a `RuntimeKind`, or changing which fields a runtime requires;
   - removing a variant of `Outcome`, `LoadOutcome` or `ContributionKind` that a guest may send.

   **Not breaking — same major:**
   - adding a field to `[provides]` or `[requires]` (an old manifest simply does not set it);
   - adding a `RuntimeKind`, an `Aspect`, a `ContributionKind`, a `SurfaceKind` or an `Outcome` variant — every one of these is `#[non_exhaustive]`, and a guest that does not know a variant never sends it;
   - adding a method to the guest protocol (an unknown method is `method not found`, which a host must already handle);
   - relaxing a validation rule.

   Two consequences worth stating. `Provides` is `deny_unknown_fields`, so a *typo* is an error rather than a silent no-op — that is deliberate, and it is why adding a field is safe: an old manifest cannot have accidentally used the new name. And the version carries **no minor**: a minor would be a way for an extension to say which additions it needs, and additions are exactly what the non-exhaustive types make safe without one.

4. **Node SDK `fs` stripping** — is it worth shipping at all, given it is not a guarantee? Argument for: it catches honest mistakes and makes the brokered path the path of least resistance. Argument against: it implies a boundary that is not there. Recommend shipping it *with* the README caveat.

   **Decided: ship it, with the caveat, and say the caveat in three places.** The loader hook goes in when the SDK does (deferred with Task 9), and the caveat is already written into `orrery-host-rpc`'s module docs — the host's own documentation says plainly that a child process bounds file descriptors and the host's memory and **not** the OS's opinion of who the child is. It must also appear in `extensions/node/ext-sdk/README.md` and in the security docs. The deciding argument is that the alternative is worse in the same direction: without the hook, `require('fs')` is not merely possible, it is the *convenient* path, and an ecosystem grows around it that the brokered path then has to compete with.

---

## State

**Landed (2026-09-19, wave 6): the two tool bundles this plan named and never
built.**

`orrery-ext-git` and `orrery-ext-lsp` were five lines of doc comment each,
which meant this plan had two of its named first-party bundles ticked as
"scaffolded" and reachable from nothing. Both are implemented, registered in
`orrery-harness::features::register_native`, in the default feature set, and
named by the shipped binary — `orrery-cli/tests/reachable.rs` drives
`CARGO_BIN_EXE_orrery` and asserts `orrery ext list` shows them, undegraded,
with the verbs they actually implement.

- **`orrery-ext-git`** — `status`, `log`, `show`, `diff`, `blame` over gitoxide,
  reusing `ade/src-tauri/src/git/gix_backend.rs` rather than rediscovering it.
  Read-only. Fourteen tests: the refusal path against the mock broker, the verbs
  against a real repository built in a temp directory with the system `git`
  binary — deliberately *not* with gitoxide, because a test that built its
  commits with the library under test cannot catch a library that writes and
  reads its own mistake consistently.
- **`orrery-ext-lsp`** — `hover`, `definition`, `references`, `diagnostics` over
  a managed client. Twenty-one tests and **no process started**: `LspTransport`
  is a trait, so a fake language server lives in the test process. Framing is
  tested separately over `&[u8]`, because it is the one part a fake cannot stand
  in for.

Three things worth recording, because each is a deviation from the sketch:

- **gitoxide uses `std::fs`, which the broker cannot mediate.** So every git
  verb asks the broker to read the repository path *first* — a real,
  policy-checked `Aspect::Read` call — and stops on a denial. Without it the
  bundle would go around the one door an extension has.
  `denied_before_gitoxide_is_opened` pins it.
- **`orrery-ext-lsp` does not use `orrery-jsonrpc`,** though this plan says it
  should. `orrery-jsonrpc` is `publish = false` and `deps-check` rule 2 forbids
  an extension from depending on an unpublished core crate — a community author
  has to build against crates.io. Sixty lines of framing live in the extension
  instead, moved from the ADE, with a comment saying to delete them when
  `orrery-jsonrpc` publishes. **Not a workaround to leave undecided: either
  publish `orrery-jsonrpc` or the duplication is permanent.**
- **Both manifests shrank.** The git scaffold declared `branch` and `commit` and
  asked for `write` and `spawn`; the lsp scaffold declared `symbols`. None
  existed. A manifest that lists a tool the extension does not have makes the
  ledger a lie, and a grant nothing uses is a capability handed over for
  nothing — so the declarations went, each with a note saying what brings it
  back.

**Landed (2026-09-18, wave 2).** Tasks 1, 2, 3, 5, 6, 7 and 8 are implemented and green; tasks 4 and 9 are deferred with the reasons written above.

- `orrery-ext-api` — the manifest (one parser for both the plan's `[extension]` spelling and the one the eleven scaffolded first-party bundles use, with a test that parses every `orrery.toml` in the tree), `Provides` and its contribution mapping from one macro invocation, the broker facade, `CallCtx`/`SurfaceSink`/`ToolBudget`, `Generation`/`InstanceState`, the `Ledger`, and `testing::load_for_test` with a recording `MockBroker`.
- `orrery-host` — the instance table keyed by generation, the load/degrade machine (a tool whose aspect is ungranted is *disabled*, a promised-but-absent tool costs only itself, a singleton goes to the closer layer and the loser is demoted in the ledger), the `native` runtime, `consent::grant_diff`, and live unload: `Live → Draining → Dead` with a grace window. It implements `orrery_tools::ToolHost`, so every extension call goes through `Registry::dispatch` and its policy check.
- `orrery-jsonrpc` — `framing/content_length.rs` moved verbatim from `ade/src-tauri/src/lsp/transport.rs` with its five tests, plus the `LineDelimited` variant and an async pair asserted to agree with the moved-in sync one; a bidirectional `Peer` with a pending map, a `Handler` for the other direction, per-call `$/cancel`, and a bounded stderr ring.
- `orrery-host-rpc` — `Containment` (a per-child Job Object with `KILL_ON_JOB_CLOSE` on Windows, a process group on unix; the only `unsafe` in the harness), `Guest`, the guest protocol, the `BrokerBridge` for guest→broker calls, and `RpcHost` for node/python/process.

Two rules ended up in the table rather than in a runtime, because they must be the same for all four: `disabled_by_grant` (one answer to "is this granted", so a `native` tool is never offered where a `node` one is hidden) and the transport rule — **a runtime that dies mid-call degrades its extension and settles the call `Failed`; the session lives.**

`orrery ext test` is wired, in plan 17.

**Landed (2026-09-18, wave 3).** Task 4. `orrery-ext-tools-builtin` implements
all six tools against `BrokerFacade` and nothing else — no `File`, no `Command`,
no `read_to_end` — and its six tests run them over the real `LocalBroker` behind
a real `PolicyEngine`. `bash_is_contained` starts a child that starts a
grandchild and asserts the beacon the grandchild was appending to stops growing
once the call's wall clock runs out, which is the job object doing what a
`Child::kill` could not.

Two things the task revealed and did not fix — **both closed in wave 4, below.**

- **`BrokerFacade` has no directory listing.** `grep` and `glob` therefore
  discover *names* with `std::fs::read_dir` and read every *byte* through the
  broker. A `list` method would close it and is a non-breaking addition under
  `orrery-ext/1` (open question 3 above).
- **The facade is table-wide, not per-call.** `ExtensionTable` holds one
  `Arc<dyn BrokerFacade>` for every call it serves, so a broker cannot see the
  `CallId` or the cancel token of the call reaching it — which is why
  `write_is_atomic` builds a facade per call. Production wiring wants the same
  thing: a `for_call(call, cancel)` factory, or a `CallCtx` that carries its own
  broker.

**Landed (2026-09-18, wave 4).** Task 9, and both disclosures above.

- **`@orrery/ext`** (`extensions/node/ext-sdk`): `defineExtension` — which both
  declares and serves, because the host starts the entry point and expects it to
  answer — the framing, the bidirectional peer with its pending map, the whole
  brokered `ctx` (`fs.list/read/write`, `proc.run`, `net.fetch`, `creds.get`,
  `ui.table/text/markdown`, and a `signal` that fires on `$/cancel` for *one*
  call), the outcome builders, and the loader hook. Twelve `node --test` cases,
  no dependencies, no build step, no network. The threat-model caveat
  (translation #13) is its README's second section, before the API: the hook is
  **opt-in through the manifest's `[process]` table**, because a hook the host
  imposed would look like a boundary the host enforces.
- **`extensions/examples/node-hello`** — two tools, one of which discovers
  through `ctx.fs.list` and declares `requires = ["read"]`, so ungranted it is
  *disabled with a reason* rather than broken. `tests/sdk.rs` runs all three
  properties through the real host.
- **`BrokerFacade::list`** — added under `orrery-ext/1` (open question 3's
  "adding a method" rule). `PolicyBroker` answers it from the engine's own
  rules and **omits** what the call may not read rather than naming it and
  refusing later; the mock broker answers it for `ext test`; `broker/list`
  carries it to a guest. `grep` and `glob` no longer touch `std::fs`.
- **`BrokerSource`** — the table asks for a facade *per dispatch*, and
  `PolicyBroker::for_call` answers, so a token minted by a running tool is tied
  to that call. Revocation also had to be made durable: a token is minted per
  broker call and redeemed at once, so `TokenLedger::revoke_call` emptying
  `live` was undone by the tool's very next `read`. A revoked call is now
  remembered and mints nothing redeemable —
  `ext_broker::revoke_call_reaches_an_in_flight_tool_call` fails without either
  half.
