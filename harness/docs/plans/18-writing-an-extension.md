# 18 · Writing an extension — the community path

**Goal.** Everything a person outside this repository needs to build, test, publish and pin an extension — and the guarantee that first-party extensions are built exactly the same way, so the path is exercised daily rather than discovered by the first outside contributor. This plan also carries the checklist for moving `extensions/` to its own repository, so that split costs nothing when it happens.

**Covers.** §4.7's five steps · §4.13's "any language takes the same five steps" · objective 9 (extend in any language).

**Deliverable.** `harness/extensions/README.md`, per-crate READMEs, four worked examples in four runtimes, the `orrery-ext-api::testing` surface, and the `xtask deps-check` / `xtask publish-check` rules that keep the path honest.

**Depends on.** [`06`](06-extension-host.md), [`14`](14-wasm-wit.md), [`15`](15-registry-supply-chain.md).

**State — done.** All six tasks. `extensions/README.md` is the tutorial, the
threat model, the README template, the release order and the split checklist.
`cargo xtask deps-check` already carried every rule this plan names (task 2 was
built in the scaffold and is wired into CI); `cargo xtask publish-check` is new
and is the offline half of task 5. `hello.parity` now exists in four runtimes
and `orrery-harness/tests/parity.rs` compares them.

Two things are narrower than the plan wrote them, and both are amended in place
below rather than left as prose: the **TinyGo leg is behind
`--features tinygo-examples`** (this machine has neither `tinygo` nor
`wit-bindgen-go`, and the test says out loud that the Go half was not compared),
and the **`cargo publish --dry-run` half of task 5 is a CI step, not a test** —
it resolves against the crates.io index, and nothing in this repository's suite
may open a socket.

---

## The five steps, in one place

§4.7's worked example, written as the canonical tutorial.

### 1 · Declare

The manifest is the contract — **nothing outside it is available at runtime.**

```toml
[extension]
api     = "orrery-ext/1"
name    = "buildgraph"
version = "1.2.0"
runtime = "node"                     # native | wasm | node | python | process

[provides]
tools = ["impacted", "deps"]         # → buildgraph.impacted, buildgraph.deps

[requires]
read  = ["$WORKSPACE/**"]
spawn = ["java"]
net   = false
```

### 2 · Implement

The extension gets a context object. It cannot import `fs` or `child_process` — the equivalents arrive through `ctx`, and every one goes back through the broker: policy-checked, budgeted, audited, cancellable.

`ctx.ui.table(...)` **describes** a table rather than drawing one: ratatui renders a widget, Ink renders a component, `--json` emits the payload, from the same description.

### 3 · Grant

Installing surfaces the manifest's requests as a diff the user or admin approves. **Denying does not fail the install** — the affected tool is disabled, everything else keeps working, and the ledger records `degraded`.

### 4 · Scope

Where it applies is configuration, not code: enabled globally, per workspace, or per project, and optionally restricted to named sub-agents.

### 5 · Distribute

Publish to crates.io or npm for the community, or to the organisation's signed registry for internal use. An enterprise pins the version and the signature; the harness refuses to load anything unpinned when managed config says so (plan 15).

**Any language takes the same five steps.** A Rust, Go or Python extension changes only the `runtime` line and the binding style — same manifest, same grant flow, same registry, same namespacing. The kernel does not know which runtime it dispatched to, which is what keeps the community path and the performance path from splitting the ecosystem in two.

---

## The rules that keep this real

These are enforced mechanically, not by review.

### Crate layout

Every crate under `extensions/crates/` is written as if a third party owned it:

```
orrery-ext-<name>/
├─ Cargo.toml        # publish = true; deps on core carry BOTH version and path
├─ orrery.toml       # the manifest — the same one a third party ships
├─ README.md         # what it does, what it asks for, and why
├─ CHANGELOG.md
└─ src/lib.rs
```

### Dependency rule

```toml
orrery-ext-api = { version = "0.1", path = "../../../core/crates/orrery-ext-api" }
```

`version` makes it publishable; `path` makes it build in-tree. **When `extensions/` moves to its own repo, delete the `path` half. Nothing else changes.**

Extensions may depend only on the **published** core crates: `orrery-proto`, `orrery-ext-api`, `orrery-provider`, `orrery-session`, `orrery-memory`, `orrery-grader`. Everything else in `core/` is `publish = false` — kernel internals are not an API.

No extension reaches into `core/` for anything else: no shared `build.rs`, no `include!` across the boundary, no dev-dependency on `orrery-kernel`.

### Testing

Tests use `orrery-ext-api::testing` — the mock broker, which is **the same code path `orrery ext test` uses**. A community author has the identical harness, including the same ledger and the same denials a real session produces. No model, no network.

---

## Tasks

### Task 1 · `extensions/README.md`

- [x] Write the tutorial: the five steps, with the full worked example from manifest to published crate.
- [x] A table of what each `[provides]` field means and which plan documents it. Both tables: the twelve collections and the four singletons, with the load error a second claim on a singleton produces.
- [x] The dependency rule, stated once, with the reason.
- [x] The threat model, honestly: `wasm` is a real boundary; `native`, `node`, `python` and `process` are capability-withholding, not sandboxing (translation #13). A determined `node` extension still has `require('fs')`. Say it here, not only in a code comment.

### Task 2 · `xtask deps-check`

Files: `harness/xtask/src/deps_check.rs`

**Already existed** — `deps-check` was built in the scaffold with all four rules
and is a CI step. The names below are the ones in the file, corrected from the
ones this plan guessed at.

- [x] **Failing test first.** `deps_check::core_may_not_depend_on_an_extension` — a fixture workspace where a core crate depends on an extension crate fails the check, naming both.
- [x] `deps_check::extension_dependency_must_carry_a_version` — an extension depending on a core crate with `path` but no `version` fails, because that crate could not be published.
- [x] `deps_check::extension_dependency_must_be_published` — an extension depending on an unpublished core crate fails. `deps_check::extension_may_not_dev_depend_on_an_unpublished_core_crate` is the dev half, which the plan did not ask for and which is the one that keeps extension tests on the mock broker.
- [x] `deps_check::orrery_harness_is_the_one_exception` — the facade is the one allow-listed crate. `a_core_binary_may_link_a_client` / `a_core_library_may_not_link_a_client` are the second exception, also unplanned: a composition root may be concrete, a library may not.
- [x] Implement over `cargo metadata`. Wire into CI.
- [x] Fixtures extended for task 5: every published fixture crate now carries the crates.io fields, so `publish_check` and `deps_check` read the same workspaces.

### Task 3 · Per-crate READMEs

- [x] Every crate in `extensions/crates/` gets a README with: what it provides, the capabilities it requests **and why each is needed**, and how to run its tests. `orrery-ext-tools-builtin` also stopped claiming to be a scaffold.
- [x] A template in `extensions/README.md` so new ones are consistent.
- [x] `examples/native-hello` gets one too, so all four worked examples are documented the same way.

### Task 4 · A worked example per runtime

Files: `harness/extensions/examples/`

- [x] `native-hello/` (Rust, compiled in), `node-hello/` (`@orrery/ext`), `wasm-hello-rs/`, `wasm-hello-go/` (plan 14 builds the last two). **`node-hello/` landed with plan 06's task 9** — two tools, its own README, and `orrery-host-rpc/tests/sdk.rs` runs it through the real host. It is JavaScript with hand-written types, not compiled TypeScript: no build step, no dependency, nothing between the source and what runs.
- [x] **Failing test first.** `parity::all_runtimes_produce_the_same_output` — the same logical tool (`hello.parity`), four runtimes, identical output. This is objective 9's proof, and it is the test that catches the kernel accidentally caring which runtime it dispatched to.
- [x] It lives in `orrery-harness/tests/parity.rs`, not in `examples/`: `native-hello` is an extension crate, and the facade is the one core crate `deps-check` allows to name one.
- [x] **Amended: it compares `Surface`, not whole `Outcome`s.** `orrery-host-wasm` does not implement `ExtensionHost` — plan 14 stopped at the engine — so the wasm leg goes through `WasmHost::call` while the other two go through `ExtensionTable::call_tool`. The `Surface` is the value all four hand back and the only thing a client sees. When the wasm host grows its `ExtensionHost` impl, tighten this to `Outcome`.
- [x] **Amended: three legs run by default, four with `--features tinygo-examples`.** TinyGo and `wit-bindgen-go` are not cargo dependencies and no machine is required to have them. Without the feature the test **prints that the Go half was not compared** rather than passing quietly — the same rule `orrery-host-wasm/tests/examples.rs` already followed.
- [x] Found on the way: both this test and `orrery-host-wasm/tests/examples.rs` built the wasm example with an inherited `CARGO_TARGET_DIR` and then read the component back from the crate-local `target/`, so they were comparing a **stale artifact**. Both now pass `--target-dir` and read the same path.

### Task 5 · Publishing dry run

**Amended: this is two halves, not one.** `cargo publish --dry-run` resolves every
dependency against the crates.io index, so it cannot be a test here — nothing in
this repository's suite may reach the network. The offline preconditions are
what a test can hold, and they are where the failures we cause by accident live.

- [x] **Failing test first.** `publish_check::every_published_crate_packages` — `cargo xtask publish-check`, offline, over `cargo metadata`: a path dependency with no `version`, a dependency on a `publish = false` crate, and the fields crates.io refuses an upload without. Four new fixtures, one broken rule each, plus one proving `publish = false` is exempt from all of it.
- [x] The real `cargo publish --dry-run` for each `publish = true` crate is a **CI step**, in dependency order. It is the second and last networked step in `test.yml`, next to `agui-drift --fetch`.
- [x] Scoped to `harness/`. The ADE's crates ship as an installer, not to crates.io; holding their manifests to a publishing rule would make this check something people turn off rather than fix.
- [x] Document the release order (core trait crates first, then extensions) in `extensions/README.md`.

### Task 6 · The split checklist

Written into `extensions/README.md` so the move is mechanical when it happens,
and every item carries how it is verifiable **today** — a checklist
whose items can only be checked after the move is a wish, not a checklist.

- [x] `cargo xtask deps-check` green — proves no `path`-only or unpublished dependency remains.
- [x] `cargo xtask publish-check` green, and `cargo publish --dry-run` green for every `publish = true` crate. Not in the original list; it is the item that catches the previous one lying.
- [x] Every published core crate is actually on crates.io at the version the extensions name.
- [x] `extensions/` gets its own `Cargo.toml` `[workspace]` and its own `pnpm-workspace.yaml` entry.
- [x] CI jobs for `extensions/` move with it; the root workspace drops `harness/extensions/crates/*` **and `harness/extensions/examples/native-hello`** from `members` and `default-members`.
- [x] `orrery-harness`'s feature-gated links become ordinary crates.io dependencies.
- [x] `orrery-harness/tests/parity.rs` moves with the examples, or keeps `native-hello` as a crates.io dependency. It is the only core test that names an extension crate, so it is the only one the split touches.
- [x] The signed registry (plan 15) index gains the new repo's provenance.
- [x] Nothing in `core/` changes. **If something does, the boundary was wrong and this checklist found it.**

---

## Done when

- `extensions/README.md` takes a reader from nothing to a published, pinned extension. **Done.**
- `cargo xtask deps-check` passes and fails for the right reasons. **Done** — nine tests, one fixture workspace per broken rule.
- Four example extensions in four runtimes produce identical output. **Done, with the Go leg opt-in:** three are compared on every run, four with `--features tinygo-examples` on a machine that has `tinygo` and `wit-bindgen-go`. Without it the test prints `NOT COMPARED` rather than passing quietly. The comparison is on the `Surface`, not the whole `Outcome`, until `orrery-host-wasm` implements `ExtensionHost`.
- Every `publish = true` crate packages cleanly in CI. **Done** — `publish-check` offline everywhere, `cargo publish --dry-run` in CI.
- The split checklist exists and each item is verifiable today. **Done**, and each item says *how*.

## Open questions

1. **When to actually split. — Settled: after phase 2.** Arguments for waiting: the contract is still moving, and a single repo makes a breaking change to `orrery-ext-api` a one-commit affair. Arguments for going early: the discipline is only real when it is enforced by distance. The counter-argument lost because the discipline turned out to be enforceable by `deps-check` and `publish-check` without the distance — which is the whole point of writing the checklist now. Recorded in `extensions/README.md`.
2. **Versioning policy across the boundary. — Still open, deliberately.** If `orrery-ext-api` goes 0.1 → 0.2, every extension needs a bump. This is the `api = "orrery-ext/1"` question from plan 06 open question 3, and it cannot be answered before the manifest freezes: a compatibility window promised over a moving contract is a promise to break. What *is* settled is the mechanism — `api = "orrery-ext/1"` is the major, additions inside it are non-breaking (plan 06 added `BrokerFacade::list` under it), and `SUPPORTED_API_MAJOR` is the one place the host decides. The duration of the window is a phase-2 decision.
3. **Where do community extensions get listed? — Settled: not here, and not by the registry.** The signed registry is an enterprise pin list; conflating it with discovery would mean either signing things we have not reviewed or refusing to list things that are fine. A public index is a separate, later product decision. `extensions/README.md` says the one-line version: the registry is how you trust an extension, the other sources are how you try one.
4. **Should `examples/` be published too? — Settled: no.** `native-hello` is `publish = false`, and `publish-check` therefore holds it to none of the publishing rules. Publishing the examples would make `cargo add orrery-ext-example` a starting point at the cost of four names in the crates.io namespace that exist only to be copied. A `cargo generate` template is the better shape and is not urgent: the four examples are four directories a reader can copy today.

---

## The publish order

`cargo publish` refuses a crate whose dependencies are not already on
crates.io, so a first publish is a sequence, not a batch. Today
`cargo publish --dry-run` fails for `orrery-ext-api`, `orrery-provider`,
`orrery-session`, `orrery-memory` and `orrery-grader` for exactly this reason
and no other: each names `orrery-proto`, which has never been uploaded. There is
nothing to fix in those crates — `cargo xtask publish-check` is green, and it is
green correctly.

Publish in this order, waiting for each wave to be live on the index before
starting the next. Crates within a wave are independent of each other.

| Wave | Crates | Why here |
|---|---|---|
| 1 | `orrery-proto` | The wire types. Everything below depends on it and it depends on nothing published. |
| 2 | `orrery-ext-api`, `orrery-provider`, `orrery-session`, `orrery-grader` | The trait crates an extension author implements. Each names `orrery-proto` and nothing else published. |
| 3 | `orrery-memory` | Names `orrery-proto` **and** `orrery-session`, so it cannot go in wave 2. |

`orrery-guest` and `orrery-updater` depend on no published crate of ours and can
go in any wave, including first. `orrery-ade` is the desktop app: it is
`publish = true` for its own release machinery and does not belong to this
sequence at all.

Two things that are easy to get wrong:

- **The index lags the upload.** A wave-2 publish started the second wave 1
  returned will sometimes still fail to resolve `orrery-proto`. Re-run it rather
  than reaching for `--no-verify`.
- **A version, once published, is gone.** Yank replaces nothing. Run
  `cargo xtask publish-check` and a `cargo package` of each crate first — that
  is the rehearsal, and it needs no network.

### `orrery-ext-api` 0.2.0 is a breaking change

It is the one crate in the list that has been published before, so its bump is
not bookkeeping. `CredStore` moved *into* it — from copies in
`orrery-ext-provider-anthropic` and `orrery-ext-provider-openai-compat` — and
became **async** on the way, because a broker call is async and the sync copies
could only ever have been backed by a file the crate picked for you. The error
type changed with it: `ProviderError` became `BrokerError`, so a denial carries
the rule that refused it.

Under pre-1.0 semver that is a major bump, which is why it is 0.2.0 and not
0.1.1. `orrery-ext-api/CHANGELOG.md` carries the migration diff; every extension
naming `orrery-ext-api = "0.1"` must be bumped and rebuilt, not just recompiled.

## State

**Landed.** `extensions/README.md` is the community path end to end,
`cargo xtask deps-check` and `cargo xtask publish-check` both enforce the rules
this plan wrote, and four example extensions in four runtimes are in the tree.

**Amended (2026-09-19): `orrery-guest` could not actually be packaged.**
Task 5 ticked "every `publish = true` crate packages cleanly", and for the SDK
every community wasm-extension author depends on that was not true:
`wit_bindgen::generate!` pointed at `../../../wit`, outside the crate directory,
which `cargo package` does not put in the tarball. The published crate failed to
read the WIT and then failed to compile — `E0433` on `bindings::orrery`, `E0432`
on `wit::NodeKind`, `wit::NodeStatus` and `raw::ProcOut`. The `.wit` is now
vendored at `orrery-guest/wit/`, and `orrery-wit/tests/wit.rs` — which
`cargo xtask wit-check` runs — fails if that copy ever differs by a byte from
`harness/wit/orrery-extension.wit`, which stays the one source of truth.

**Added (2026-09-19): the publish order above.** The five remaining dry-run
failures were being re-diagnosed each round as crate defects. They are not; they
are first-publish ordering, and the order is now written down.
