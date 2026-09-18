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
- `harness/core/crates/orrery-wit/src/{lib,bindings,arena}.rs`
- `harness/core/crates/orrery-host-wasm/src/{lib,engine,store,imports,limits,cancel}.rs`
- `harness/core/crates/orrery-host-wasm/tests/{sandbox,limits,cancel,roundtrip}.rs`
- `harness/extensions/crates/orrery-guest/{Cargo.toml,README.md,src/*}`
- `harness/extensions/examples/wasm-hello-rs/`, `harness/extensions/examples/wasm-hello-go/`

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

- [ ] **Failing test first.** `limits::memory_ceiling_traps` — a guest that allocates past its ceiling traps; the host reports `Outcome::Failed` with a clear reason, and the session lives.
- [ ] `limits::epoch_ceiling_traps` — an infinite-loop guest is trapped within the wall-clock budget.
- [ ] `sandbox::no_preopens` — a guest calling a WASI filesystem function fails; assert the linker exposes no preopened directory. **This is the test that proves the boundary.**
- [ ] Implement.

### Task 3 · Broker imports

Files: `orrery-host-wasm/src/imports.rs`

- [ ] **Failing test first.** `imports::denied_call_returns_error_not_trap` — a guest calling `run-proc` without a `spawn` grant gets `error::denied`, and the guest can handle it. A denial must be a value here too.
- [ ] `imports::guest_never_sees_a_token` — a type-level check plus an ABI review note: no import signature carries one.
- [ ] `imports::output_ceiling_applies` — `read-file` with `max_bytes` returns truncated data and the flag.
- [ ] Implement all five imports over the broker.

### Task 4 · Cancellation

Files: `orrery-host-wasm/src/cancel.rs`, `tests/cancel.rs`

- [ ] **Failing test first.** `cancel::traps_a_spinning_guest` — cancel; the guest traps; the call settles `Cancelled`.
- [ ] `cancel::blocked_in_import_is_freed_by_the_broker` — a guest blocked in `run-proc`; cancelling kills the child, the import returns `cancelled`, the guest traps at its next backedge. Assert both halves happened.
- [ ] Implement; **document the coarseness in the crate docs**, not only here.

### Task 5 · The guest SDK

Files: `orrery-guest/*`

- [ ] **Failing test first.** `guest::hides_the_arena` — an extension written with `ctx.ui.table(..)` produces a valid arena without the author touching indices.
- [ ] Implement `export_extension!`, `Ctx`, the `ui` builders, error mapping.

### Task 6 · Examples in two languages

Files: `extensions/examples/wasm-hello-{rs,go}/`

- [ ] Rust example using `orrery-guest`.
- [ ] **TinyGo example using `wit-bindgen` directly** — the point is to prove the WIT generates usable bindings for a language with no SDK of ours. If this is painful, the WIT is wrong.
- [ ] **Failing test first.** `examples::both_load_and_dispatch` — build both (behind a feature flag so CI without TinyGo skips), load each, dispatch, assert identical output.

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

- `cargo test -p orrery-wit -p orrery-host-wasm` green, including the arena proptest.
- A guest cannot touch the filesystem except through the broker — proven by the no-preopens test.
- Memory and wall-clock ceilings demonstrably trap.
- Two example extensions in two languages load from the same `.wit` and behave identically.
- The JSON-cost measurement is recorded.

## Open questions

1. **Freeze the arena shape before phase 2 ends.** Every guest language binds against it; changing it later is the one genuinely expensive mistake available here.
2. **Fuel under eval only** — confirm the eval runner can turn it on per-run without a second engine. `Config` is per-`Engine`, so this may mean two engines. Check early.
3. **WASI p2 → p3.** Track it, do not chase it. Write down what a migration would touch (async imports, mainly) so the cost is known.
4. **Component size.** A Rust wasm component with the SDK is not small. Measure and, if it matters, document the `opt-level = "z"` + `wasm-opt` recipe in `orrery-guest`'s README.
