# 14 · WASM and WIT — the sandboxed path, and the one source of truth

**Goal.** A `.wit` world that generates bindings for every WASM-target language and doubles as the schema the JSON-RPC paths serialise, plus a wasmtime host where an extension is sandboxed by construction: no preopens, memory capped, wall clock capped, and every resource arriving as an imported broker function. Adding a language means generating bindings, not changing the kernel.

**Covers.** §4.13 in full · the wasm row of §4.7's runtime table.

**Crates.** `core/crates/orrery-wit` · `core/crates/orrery-host-wasm` · `extensions/crates/orrery-guest` (the guest SDK, published) · `extensions/examples/`.

**Depends on.** [`01`](01-proto-shared-types.md), [`06`](06-extension-host.md), [`07`](07-policy-broker-audit.md).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- One `Store` per instance, `ResourceLimiter` for memory, `epoch_interruption` for wall clock, **zero WASI preopens**, fuel only under the eval runner.
- Target **WASI p2**. p3 async is a later migration; the contract must stop moving before the runtime does.
- Wasm cancellation is coarse: trap and discard the Store.
- `Surface` crosses the boundary as a **flat arena** (translation #9).
- JSON crosses as `string` — measure the cost before phase 2 ends.

This plan owns translation **#9** and the two flagged-but-undecided wasm items in the overview.

---

## Architecture

### The WIT world is the single source of truth

One `.wit` definition generates bindings for every WASM-target language **and** is the schema the JSON-RPC paths serialise. That is what keeps this from becoming six half-maintained SDKs.

```wit
package orrery:extension@1.0.0;

interface broker {
  record run-opts { timeout-ms: u64, max-output-bytes: u64 }
  record proc-out { exit-code: s32, stdout: list<u8>, stderr: list<u8>, truncated: bool }
  variant error { denied(string), budget(string), io(string), cancelled }

  run-proc: func(cmd: string, args: list<string>, opts: run-opts) -> result<proc-out, error>;
  read-file: func(path: string, max-bytes: u64) -> result<list<u8>, error>;
  write-file: func(path: string, bytes: list<u8>, atomic: bool) -> result<_, error>;
  fetch: func(url: string, opts: fetch-opts) -> result<fetch-out, error>;
  use-credential: func(name: string, usage: cred-usage) -> result<_, error>;
}

interface surfaces {
  // Translation #9: WIT has no recursive types, so a Surface crosses as a flat arena.
  record surface-node { kind: node-kind, children: list<u32>, /* … */ }
  record surface { nodes: list<surface-node>, root: u32 }
}

world orrery-extension {
  import broker;
  use surfaces.{surface};
  export tools: interface {
    call: func(name: string, input: string) -> result<surface, string>;   // input is JSON
  }
}
```

**The flat arena is the largest single impedance mismatch in the whole design.** `Surface` has `stack.children: Vec<Surface>` and `custom.fallback: Box<Surface>`; neither survives WIT. The arena — `list<surface-node>` plus child indices — is rebuilt host-side into the real `Surface`. Settle this **before phase 2 freezes the WIT**, because every guest language binds against it.

### The host

```rust
pub struct WasmHost { engine: Engine, linker: Linker<HostState> }

struct HostState {
    broker: Arc<dyn Broker>,
    token_source: TokenSource,     // mints a per-call token; the guest never sees one
    budget: ToolBudget,
    cancel: CancellationToken,
    limits: StoreLimits,
}
```

Configuration, each line load-bearing:

| Setting | Why |
|---|---|
| `Config::epoch_interruption(true)` + a ticker thread | Wall-clock ceiling. Traps at loop backedges. |
| `Store::limiter(..)` with `memory_size` from the budget | Memory ceiling, enforced by wasmtime rather than hoped for. |
| **No `preopened_dir`, ever** | The filesystem arrives only through `broker.read-file`. A preopen would be a hole straight past the policy engine. |
| `Config::consume_fuel(true)` **only under the eval runner** | Determinism for reproducible runs; it costs throughput, so it is off normally. |
| `Config::wasm_component_model(true)` | We bind components, not core modules. |
| One `Store` per instance | Isolation, and it makes "discard on trap" a complete cleanup. |

### Cancellation is coarse, and we say so

Epoch interruption traps at loop backedges and **cannot cleanly unwind a guest blocked inside a host import**. So `turn.cancel` against a wasm tool is "trap the guest and discard the `Store`", not a graceful abort. Consequences to document:

- The guest gets no chance to clean up. Anything it needed to finish must have gone through the broker, which is transactional where it matters (atomic writes).
- A guest blocked in `run-proc` is cancelled by the **broker** killing the child, which returns `error::cancelled` to the guest, which then traps at its next backedge. Two mechanisms, and the broker's is the one that stops the work.

### Why the broker is an import, not a capability handle

The guest never holds a `CapabilityToken` — it cannot, since the type is not `Serialize` (plan 07). Instead each imported function looks up the current call's token from `HostState` on the host side. The guest asks; the host decides. That is objective 2 and the structural rule, enforced by the ABI rather than by discipline.

### The guest SDK

`extensions/crates/orrery-guest`, published, thin:

```rust
orrery_guest::export_extension! {
    tools: {
        "impacted" => |input: Impacted, ctx: Ctx| -> Result<Surface, String> {
            let out = ctx.proc.run("java", &["-jar", "bg.jar"], RunOpts::default())?;
            Ok(ctx.ui.table(&["module", "reason"], parse(&out.stdout)))
        }
    }
}
```

It hides the arena: `ctx.ui.*` builds a normal tree and flattens it on the way out.

### Costs, stated honestly

§4.13 already says them; repeat them here so nobody rediscovers them in a sprint:

- The WASM path **constrains what a language can do** — threads, some syscalls and large native dependencies are limited, and not every ecosystem has mature WASM targets.
- The external-process path has no sandbox of its own beyond what the broker withholds, so an enterprise will want those pinned by image digest.
- Three runtime paths is three sets of docs and examples. Ship wasm and node first-class in phase 2; add python and process once the contract has stopped moving.

---

## File structure

**Create**

- `harness/wit/orrery-extension.wit`
- `harness/core/crates/orrery-wit/src/{lib,arena}.rs` (no `bindings.rs`: the host
  bindings are generated in `orrery-host-wasm`, so `orrery-wit` stays free of
  `wasmtime` and the arena is linkable from paths with no wasm in them)
- `harness/core/crates/orrery-host-wasm/src/{lib,engine,store,imports,limits,cancel,broker}.rs`
  (`broker.rs` added: the token-free `HostBroker` face the imports are served from)
- `harness/core/crates/orrery-host-wasm/tests/{sandbox,limits,cancel,roundtrip,imports,examples}.rs`
- `harness/extensions/crates/orrery-guest/{Cargo.toml,README.md,src/*}`
- `harness/extensions/examples/wasm-hello-rs/`, `harness/extensions/examples/wasm-hello-go/`
- `harness/core/crates/orrery-host-wasm/tests/guests/sandbox-probe/` — added: a guest
  written against the raw `.wit` that probes the boundary from the inside. Every
  sandbox, ceiling, import and cancellation test is driven through it, because
  the host can only assert that it *configured* a sandbox.

---

## Tasks

### Task 1 · The WIT world and the arena

Files: `harness/wit/orrery-extension.wit`, `orrery-wit/src/arena.rs`

- [x] **Failing test first.** `arena::round_trips` (proptest) — for an arbitrary `Surface`, `flatten` then `rebuild` yields the original. Including deep stacks and nested `custom` fallbacks.
- [x] `arena::rejects_cycles` — a hand-built arena with a child index loop is rejected, not infinitely recursed. Two tests: the self-loop and the 0→1→0 loop.
- [x] `arena::rejects_dangling_indices`, plus `rejects_a_dangling_root`, `rejects_a_kind_tag_that_disagrees_with_the_payload`, `custom_must_carry_exactly_one_fallback_child`, `a_leaf_carries_no_children`, `nodes_are_written_parent_first_depth_first`.
- [x] Write the `.wit`; implement `flatten`/`rebuild`.
- [x] `cargo xtask wit-check` — the `.wit` parses and the frozen shapes have not drifted from the Rust that rebuilds them (`orrery-wit/tests/wit.rs`, run by the xtask).

**FROZEN, 2026-09-18.** A node is `{ kind, id, status, payload, children }`; `kind`
is a flat enum over all twelve `SurfaceKind` variants and must agree with the
`t` tag inside `payload`; `payload` is the variant's serde JSON minus its
recursive fields; `children` indices are **strictly greater** than the node's
own, so the arena is acyclic by construction and walkable without a visited set;
nodes are written parent-first, depth-first; only `stack` (any number) and
`custom` (exactly one — the mandatory fallback) carry children. `rebuild` is the
trust boundary and enforces every one of those as a value, plus a `MAX_DEPTH`
of 128 so a 100k-node chain cannot blow the host stack.

Two corrections to the sketch in this file, both made in the frozen `.wit`:
the `node-kind` enum in the sketch was missing `tree`, `task`, `question` and
`form` and invented a `details` that `SurfaceKind` does not have; and
`broker.read-file` now returns a `file-out` record rather than a bare
`list<u8>`, because task 3 requires the truncation flag to come back with the
bytes.

### Task 2 · Engine and store

Files: `orrery-host-wasm/src/{engine,store,limits}.rs`

- [x] **Failing test first.** `limits::memory_ceiling_traps` — a guest that allocates past its ceiling traps; the host reports `Outcome::Trapped` with a clear reason, and the session lives (the next call on the same host is asserted to work).
- [x] `limits::epoch_ceiling_traps` — an infinite-loop guest is trapped within the wall-clock budget. Plus `a_missing_memory_budget_is_not_an_absent_ceiling` and `a_zero_wall_clock_budget_is_not_forever`.
- [x] `sandbox::no_preopens` — **done, and from inside a real guest.** `tests/guests/sandbox-probe` tries four paths, a `read_dir` and a write; the host asserts none succeeded and that the broker was never reached, so it is the guest failing to get out rather than the broker refusing it. A second, cheaper test fails at the same time if anybody adds a `preopened_dir` / `inherit_*` / `allow_*` call to `store::no_preopens`.
- [x] Implement.

Note on the outcome type: the plan said `Outcome::Failed`. `Failed` is now the
guest's **own** error string — the message the model sees — and a ceiling is
`Outcome::Trapped`. Two different things deserved two names.

A trap needs a guest that really loops. `sandbox-probe`'s spin uses
`std::hint::black_box`: without it LLVM proves a counting loop terminates,
deletes it, and the "infinite loop" test silently stops testing anything.

### Task 3 · Broker imports

Files: `orrery-host-wasm/src/imports.rs`

- [x] **Failing test first.** `imports::denied_call_returns_error_not_trap` — the guest receives `error::denied`, reports it in a surface, and carries on; returning at all is what proves it was not trapped. `every_import_denies_as_a_value` does the same for the other four.
- [x] `imports::guest_never_sees_a_token` — a type-level check (nothing in `HostBroker`'s signatures, nothing in the `.wit`'s imports) plus `orrery-wit`'s `no_import_carries_a_capability_token`, which asserts it against the world itself.
- [x] `imports::output_ceiling_applies` — and `a_short_file_is_not_flagged_truncated` for the other side of it.
- [x] Implement all five imports over the broker.

**`read-file` now returns a `file-out` record.** The sketch returned a bare
`list<u8>`, which has nowhere to put the truncation flag this task requires.

### Task 4 · Cancellation

Files: `orrery-host-wasm/src/cancel.rs`, `tests/cancel.rs`

- [x] **Failing test first.** `cancel::traps_a_spinning_guest`.
- [x] `cancel::blocked_in_import_is_freed_by_the_broker` — **both halves asserted.** The guest writes a witness file between the import returning and its spin, so the test can tell "the broker freed it with a value" from "the epoch alone stopped it". Plus `a_cancelled_call_starts_no_more_work`: after a cancel, imports refuse without reaching the broker.
- [x] Implement; the coarseness is in `orrery-host-wasm`'s crate docs and in `cancel`'s module docs, and is readable as data via `WasmHost::coarseness()` so a client can say the true thing in a tooltip.

**Mechanism correction.** Epoch interruption alone cannot carry cancellation: a
cancel would have to race the epoch counter past a live deadline, which for a
ten-minute budget is 60 000 increments. The store instead keeps a **one-tick
deadline with a callback** that decides each time whether to extend — it reads
the cancel flag and the elapsed wall clock, and refuses. One mechanism, both
ceilings, and a cancel that lands within one tick whatever the budget. The
callback only runs while the guest is executing wasm, which is exactly why it
cannot free a guest blocked in an import — the documented coarseness, now with a
mechanism behind it rather than an assertion.

### Task 5 · The guest SDK

Files: `orrery-guest/*`

- [x] **Failing test first.** `guest::hides_the_arena` — `orrery-guest/tests/ui.rs` asserts the arena a `ui::section(..)` tree flattens to, and `orrery-host-wasm`'s `the_sdk_hides_the_arena` asserts the example's source contains no index *and* that it round-trips through a real host.
- [x] Implement `export_extension!`, `Ctx` (`proc` / `fs` / `net`), the `ui` builders, error mapping.

`Ctx` is `ctx.proc` / `ctx.fs` / `ctx.net` rather than the sketch's
`ctx.proc` / `ctx.ui`: `ui` is free functions, because a builder hanging off the
context implied it needed one.

### Task 6 · Examples in two languages

Files: `extensions/examples/wasm-hello-{rs,go}/`

- [x] Rust example using `orrery-guest` (`wasm-hello-rs`). 53 KiB with the size recipe.
- [~] **TinyGo example** (`wasm-hello-go`) — written against the raw `.wit` for `wit-bindgen-go`, with a README giving the exact two commands. **NOT BUILT OR RUN HERE: TinyGo is not installed on this machine**, so it is behind the `tinygo-examples` cargo feature and `examples::both_load_and_dispatch` prints "NOT COMPARED" rather than skipping quietly. The Go source is written but unverified; compiling it is the remaining work.
- [x] **Failing test first.** `examples::both_load_and_dispatch` — builds the Rust half, loads it, dispatches, and compares structurally (heading, columns, first cell) so the one legitimate difference between the languages is the only one allowed.

**The world binds without an SDK — proven, in Rust.**
`tests/guests/sandbox-probe` is written against the raw `.wit` with
`wit-bindgen` alone, uses every import and builds arenas by hand, and drives
every test in `orrery-host-wasm`. It compiled first time bar one borrow. That is
most of the evidence this task wanted; the TinyGo half would extend it to a
non-Rust toolchain.

### Task 7 · JSON-over-the-boundary cost

Files: `tests/roundtrip.rs`

- [x] **Measure, then decide.** `orrery-wit/tests/json_cost.rs`, 1000 delta updates of the hot shape (a row stack holding a text line and a two-cell table).
- [x] Decision recorded below; the test is the regression guard.

**MEASURED, 2026-09-18** (`cargo test -p orrery-wit --test json_cost --release -- --nocapture`):

| | bytes over 1000 updates | JSON documents |
|---|---|---|
| JSON-as-string (what the WIT does) | 218 300 | 3 000 |
| typed in WIT (hypothetical) | 165 300 | 0 |

**Overhead: 32.1%.** Flatten 5.9 µs/update, rebuild 8.3 µs/update, 16.6 µs for a
whole round trip of a 3-node update. **Measured at 32%, acceptable — closed.**
Typing twelve more records into the contract to save a third of the bytes on a
path that costs microseconds is not worth freezing them, and the shapes would
then have to move in lockstep with `SurfaceKind` forever. The test fails above
120% so the decision is re-opened if payloads ever grow a shape the estimate
does not model.

---

## Done when

- [x] `cargo test -p orrery-wit -p orrery-host-wasm -p orrery-guest` green, including the arena proptest. 19 + 26 + 4 tests.
- [x] A guest cannot touch the filesystem except through the broker — proven by `sandbox::no_preopens`, from inside a guest.
- [x] Memory and wall-clock ceilings demonstrably trap, and the session survives both.
- [~] Two example extensions in two languages load from the same `.wit`. The Rust one is built and asserted; the **TinyGo one is written but unbuilt — TinyGo is not installed here** — and is gated behind `--features tinygo-examples`.
- [x] The JSON-cost measurement is recorded: 32.1%, acceptable.

## Open questions

1. **CLOSED — the arena is frozen.** See task 1. The shape, its six invariants
   and the `rebuild` that enforces them are settled, and `cargo xtask wit-check`
   fails if the `.wit` and the Rust drift.
2. **CLOSED — fuel needs a second engine.** `consume_fuel` is a `Config`
   setting and `Config` is per-`Engine`, so the eval runner cannot turn it on
   per run against a shared engine. `WasmHost::for_eval()` builds the second
   one. It is cheap: an `Engine` is a compiler and a code cache, and eval wants
   a separate cache anyway.
3. **OPEN, deliberately — WASI p2 → p3.** What a migration touches, now that
   there is code to look at: the imports are already async
   (`imports: { default: async }`), so the host-function bodies do not move.
   What moves is `wasmtime_wasi::p2::add_to_linker_async` → the p3 equivalent,
   the `WasiView` / `WasiCtxView` impl in `store.rs`, and any guest built for
   `wasm32-wasip2`. **The `.wit` does not move at all**, which was the point of
   freezing the contract before the runtime. Track it; do not chase it.
4. **CLOSED — component size measured.** Raw guest, no SDK, `opt-level = "z"` +
   `strip` + `panic = "abort"`: **109 KiB**. With `orrery-guest` and the full
   recipe (`lto`, `codegen-units = 1` as well): **53 KiB**. The recipe and the
   numbers are in `orrery-guest`'s README. `wasm-opt -Oz` takes roughly another
   fifth off and is not a cargo dependency.

## Not done here

- **The real broker is not wired in.** `orrery-host-wasm` defines `HostBroker` —
  the token-free face the guest's imports are served from — and maps
  `orrery_broker::BrokerError` onto its four cases. What is missing is the
  embedder that holds both a `TokenMinter` and a `LocalBroker`:
  `TokenMinter::mint` is `pub(crate)` in `orrery-policy` (plan 07, and
  correctly — only the engine mints), so there is **no public path from a
  `PendingCall` to a `CapabilityToken`**. Plan 07 or the kernel needs to expose
  one before this host can call the real broker. Everything above that seam is
  done and tested against a fake.
- **`orrery-guest` pins `wit-bindgen = "0.51"` directly**, not
  `{ workspace = true }`: the root pins 0.36, whose generated `export!` emits
  edition-2021 attribute syntax and will not compile inside an edition-2024
  guest. Move it back to the workspace pin once the root moves.
