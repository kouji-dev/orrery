# 18 · Writing an extension — the community path

**Goal.** Everything a person outside this repository needs to build, test, publish and pin an extension — and the guarantee that first-party extensions are built exactly the same way, so the path is exercised daily rather than discovered by the first outside contributor. This plan also carries the checklist for moving `extensions/` to its own repository, so that split costs nothing when it happens.

**Covers.** §4.7's five steps · §4.13's "any language takes the same five steps" · objective 9 (extend in any language).

**Deliverable.** `harness/extensions/README.md`, per-crate READMEs, the `orrery-ext-api::testing` surface, and the `xtask deps-check` rules that keep the path honest.

**Depends on.** [`06`](06-extension-host.md), [`14`](14-wasm-wit.md), [`15`](15-registry-supply-chain.md).

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

- [ ] Write the tutorial: the five steps, with the full worked example from manifest to published crate.
- [ ] A table of what each `[provides]` field means and which plan documents it.
- [ ] The dependency rule, stated once, with the reason.
- [ ] The threat model, honestly: `wasm` is a real boundary; `native`, `node`, `python` and `process` are capability-withholding, not sandboxing (translation #13). A determined `node` extension still has `require('fs')`. Say it here, not only in a code comment.

### Task 2 · `xtask deps-check`

Files: `harness/xtask/src/deps_check.rs`

- [ ] **Failing test first.** `deps_check::rejects_core_depending_on_extensions` — a fixture workspace where a core crate depends on an extension crate fails the check, naming both.
- [ ] `deps_check::rejects_a_path_only_dependency` — an extension depending on a core crate with `path` but no `version` fails, because that crate could not be published.
- [ ] `deps_check::rejects_an_unpublished_core_dep` — an extension depending on `orrery-kernel` fails.
- [ ] `deps_check::allows_the_facade` — `orrery-harness` is the one allow-listed exception.
- [ ] Implement over `cargo metadata`. Wire into CI.

### Task 3 · Per-crate READMEs

- [ ] Every crate in `extensions/crates/` gets a README with: what it provides, the capabilities it requests **and why each is needed**, and how to run its tests.
- [ ] A template in `extensions/README.md` so new ones are consistent.

### Task 4 · A worked example per runtime

Files: `harness/extensions/examples/`

- [ ] `native-hello/` (Rust, compiled in), `node-hello/` (TS, `@orrery/ext`), `wasm-hello-rs/`, `wasm-hello-go/` (plan 14 builds the last two).
- [ ] **Failing test first.** `examples::all_runtimes_produce_the_same_output` — the same logical tool, four runtimes, identical `Outcome`. This is objective 9's proof, and it is the test that catches the kernel accidentally caring which runtime it dispatched to.

### Task 5 · Publishing dry run

- [ ] **Failing test first.** `publish::every_published_crate_packages` — `cargo publish --dry-run` for each `publish = true` crate, in CI. Catches a missing `version` on a path dependency before it reaches crates.io.
- [ ] Document the release order (core trait crates first, then extensions) in `extensions/README.md`.

### Task 6 · The split checklist

Write this into `extensions/README.md` so the move is mechanical when it happens:

- [ ] `cargo xtask deps-check` green — proves no `path`-only or unpublished dependency remains.
- [ ] Every published core crate is actually on crates.io at the version the extensions name.
- [ ] `extensions/` gets its own `Cargo.toml` `[workspace]` and its own `pnpm-workspace.yaml` entry.
- [ ] CI jobs for `extensions/` move with it; the root workspace drops `harness/extensions/crates/*` from `members`.
- [ ] `orrery-harness`'s feature-gated links become ordinary crates.io dependencies.
- [ ] The signed registry (plan 15) index gains the new repo's provenance.
- [ ] Nothing in `core/` changes. **If something does, the boundary was wrong and this checklist found it.**

---

## Done when

- `extensions/README.md` takes a reader from nothing to a published, pinned extension.
- `cargo xtask deps-check` passes and fails for the right reasons.
- Four example extensions in four runtimes produce identical output.
- Every `publish = true` crate packages cleanly in CI.
- The split checklist exists and each item is verifiable today.

## Open questions

1. **When to actually split.** Arguments for waiting: the contract is still moving, and a single repo makes a breaking change to `orrery-ext-api` a one-commit affair. Arguments for going early: the discipline is only real when it is enforced by distance. Suggest: **after phase 2 freezes the manifest and the WIT**, not before.
2. **Versioning policy across the boundary.** If `orrery-ext-api` goes 0.1 → 0.2, every extension needs a bump. Do we promise a compatibility window, and how long? This is the `api = "orrery-ext/1"` question from plan 06 open question 3 — answer both in one place.
3. **Where do community extensions get listed?** The signed registry is an enterprise pin list, not a discovery surface. A public index is a separate, later thing — do not conflate them.
4. **Should `examples/` be published too?** Publishing them makes `cargo add orrery-ext-example` a real starting point; not publishing keeps the crates.io namespace clean. Suggest a `cargo generate` template instead.
