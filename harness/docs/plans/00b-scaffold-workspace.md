# 00b · Scaffold — every crate as an empty, compiling stub

**Goal.** Create the whole harness skeleton in one pass: the `core/` · `extensions/` · `clients/` tree, every crate's `Cargo.toml` and `lib.rs` with a pointer to its plan, the publish flags, the manifests, the pnpm packages, `xtask` with a working `deps-check`, the WIT skeleton and the CI job. Nothing implements anything. When this is done, `cargo test` at the root builds the whole harness, `cargo run -p orrery-cli -- --help` prints the full command tree, and `cargo xtask deps-check` is green.

**Covers.** No architecture section — this is the scaffold [`00-overview.md`](00-overview.md) describes.

**Depends on.** [`00a-restructure-ade.md`](00a-restructure-ade.md) must land first.

**Why scaffold everything at once rather than crate-by-crate.** The dependency direction rules (`core` never depends on `extensions`; extensions depend only on published crates) are only checkable when the whole graph exists. Creating them together and turning on `deps-check` immediately means the boundary is enforced from the first commit rather than discovered later.

---

## Tasks

### Task 1 · The directory tree

- [ ] Create:
  ```
  harness/core/crates/          harness/extensions/crates/
  harness/extensions/node/      harness/extensions/examples/
  harness/clients/{sdk-rs,sdk-ts,ratatui,json,ink,ade,conformance}/
  harness/xtask/  harness/wit/  harness/protocol/
  ```
- [ ] `harness/README.md` — what the harness is, how to build it, and a pointer to `docs/README.md`.
- [ ] `harness/extensions/README.md` — a stub carrying the "how to write one" heading; [`18`](18-writing-an-extension.md) fills it.
- [ ] `harness/clients/README.md` — the one-renderer-per-folder rule, and the AG-UI outward-only state note from [`08`](08-protocol-transport.md).
- [ ] `harness/clients/conformance/README.md` — the fixture format; [`08`](08-protocol-transport.md) task 1 writes the fixtures.

### Task 2 · Root workspace

- [ ] Extend the root `Cargo.toml` created by `00a`:
  ```toml
  [workspace]
  resolver = "2"
  members = [
    "ade/src-tauri", "ade/src-tauri/updater-stub",
    "harness/core/crates/*",
    "harness/extensions/crates/*",
    "harness/clients/sdk-rs", "harness/clients/ratatui", "harness/clients/json",
    "harness/xtask",
  ]
  default-members = [
    "harness/core/crates/*",
    "harness/extensions/crates/*",
    "harness/clients/sdk-rs", "harness/clients/ratatui", "harness/clients/json",
  ]
  ```
  `clients/` members are listed explicitly because that directory also holds TypeScript packages, which a glob would try to include.
- [ ] Add every dependency from [`00-overview.md`](00-overview.md)'s table to `[workspace.dependencies]`, pinned. Crates reference them as `{ workspace = true }`.
- [ ] `[workspace.package]` with shared `edition`, `license`, `repository`, `rust-version`.
- [ ] **Verify:** `cargo build` at the root builds no Tauri code.

### Task 3 · Core crates

One directory per row of `00-overview.md` table 1.

- [ ] For each: `Cargo.toml` (workspace-inherited metadata, no dependencies beyond what compiles empty) and `src/lib.rs` containing only:
  ```rust
  //! <one line: what this crate owns>
  //!
  //! Implementation plan: `harness/docs/plans/<NN>-<name>.md`
  #![deny(missing_docs)]
  #![forbid(unsafe_code)]    // omit where a crate genuinely needs unsafe: host-wasm, broker
  ```
- [ ] Set `publish = true` on exactly: `orrery-proto`, `orrery-ext-api`, `orrery-provider`, `orrery-session`, `orrery-memory`, `orrery-grader`. Everything else in `core/` gets `publish = false`.
- [ ] `orrery-cli` gets `[[bin]] name = "orrery"`.
- [ ] **Verify:** `cargo check` passes for all of them.

### Task 4 · Extension crates

One per row of table 2.

- [ ] For each: `Cargo.toml`, `orrery.toml`, `README.md`, `src/lib.rs` stub.
- [ ] Dependencies on core use **both** halves — `orrery-ext-api = { version = "0.1", path = "../../../core/crates/orrery-ext-api" }`. This is the rule that makes the eventual repo split free.
- [ ] Each `orrery.toml` declares `api = "orrery-ext/1"`, `runtime = "native"`, its `[provides]` and its `[requires]` — even though nothing reads them yet. Writing them now means [`06`](06-extension-host.md) has real fixtures to parse on day one.
- [ ] `orrery-ext-memory-file` is **not** in any default feature set (see [`12`](12-memory.md)).

### Task 5 · Client crates and packages

- [ ] `clients/sdk-rs` (`orrery-client`, `publish = true`), `clients/ratatui` (`orrery-client-ratatui`), `clients/json` (`orrery-client-json`) as empty Rust crates.
- [ ] `clients/sdk-ts` (`@orrery/client`) and `clients/ink` (`@orrery/client-ink`) as `package.json` + `README.md` + `tsconfig.json` stubs, private for now.
- [ ] `clients/ade/README.md` — a pointer saying the Angular renderer lives in `ade/` and builds on `@orrery/client`.

### Task 6 · `orrery-cli` command tree

- [ ] Implement the full clap tree from [`17`](17-cli.md), with every subcommand present.
- [ ] Each unimplemented subcommand exits 2 with `not implemented in this build — see harness/docs/plans/<NN>-<name>.md`. The tree is complete from day one and fills in plan by plan.
- [ ] **Failing test first.** `cli::help_snapshot` — snapshot `--help` and each subcommand's help.
- [ ] **Verify:** `cargo run -p orrery-cli -- --help` prints the tree.

### Task 7 · `xtask`

- [ ] `harness/xtask` with subcommands `deps-check`, `typegen`, `wit-check`, `agui-drift`.
- [ ] **`deps-check` is implemented for real** — it is the only thing here that enforces a rule. Over `cargo metadata`:
  1. No `core/` crate depends on an `extensions/` or `clients/` crate. Allow-list exactly one exception: `orrery-harness`.
  2. Every `extensions/` → `core/` dependency names a `publish = true` crate **and** carries a `version`.
  3. No `extensions/` crate dev-depends on an unpublished core crate.
- [ ] **Failing test first.** Fixture workspaces under `xtask/tests/fixtures/` that violate each rule; assert each is caught and the message names both crates. ([`18`](18-writing-an-extension.md) task 2 owns these tests; write them here and let that plan extend them.)
- [ ] `typegen`, `wit-check`, `agui-drift` are stubs that exit 0 with "not implemented", each naming its plan.

### Task 8 · WIT and protocol

- [ ] `harness/wit/orrery-extension.wit` — the world sketched in [`14`](14-wasm-wit.md), including the flat-arena surface. It does not have to be final; it has to exist so `wit-check` has a target and so the arena question is visible rather than deferred silently.
- [ ] `harness/protocol/package.json` (`@orrery/protocol`, private, `types: protocol.d.ts`) and a `README.md` saying the contents are generated by `cargo xtask typegen` and must not be hand-edited.

### Task 9 · pnpm workspace

- [ ] `pnpm-workspace.yaml` — extend the `packages:` list `00a` created:
  ```yaml
  packages:
    - ade
    - harness/protocol
    - harness/clients/sdk-ts
    - harness/clients/ink
    - harness/extensions/node/*
  ```
- [ ] `pnpm install --lockfile-only` and commit.

### Task 10 · CI

- [ ] `.github/workflows/test.yml` — add a `harness` job, independent of the ADE job:
  ```yaml
  - run: cargo fmt --all -- --check
  - run: cargo clippy --workspace --exclude orrery-ade --exclude orrery-updater -- -D warnings
  - run: cargo test -p orrery-proto
  - run: cargo xtask deps-check
  ```
- [ ] Rust cache keyed on the root `Cargo.lock`, shared with the ADE job.
- [ ] **Do not** add `cargo test --workspace` — the repo convention is per-crate test runs, and plans add their own jobs as they land.

### Task 11 · Verification

```powershell
cargo metadata --format-version 1 --no-deps    # every harness crate is a member
cargo build                                     # default-members only; no Tauri
cargo test -p orrery-proto                      # placeholder passes
cargo run -p orrery-cli -- --help
cargo xtask deps-check                          # green
cargo clippy --workspace --exclude orrery-ade --exclude orrery-updater -- -D warnings
cargo publish --dry-run -p orrery-proto         # the published surface packages
pnpm install --frozen-lockfile
pnpm -C ade build                               # the ADE is untouched
```

- [ ] Every crate in `00-overview.md`'s three tables exists, and every one names its plan file in its `lib.rs`.

---

## Done when

- `cargo test` at the root builds the whole harness and no Tauri code.
- `cargo run -p orrery-cli -- --help` prints the complete command tree.
- `cargo xtask deps-check` passes, and demonstrably fails on each of its three fixture violations.
- `cargo publish --dry-run` succeeds for every `publish = true` crate.
- The ADE still builds and tests exactly as it did after `00a`.

## Open questions

1. **`#![forbid(unsafe_code)]` exceptions.** `orrery-host-wasm` and `orrery-broker` will need `unsafe` (wasmtime host state, platform process APIs). Mark those two `#![deny(unsafe_op_in_unsafe_fn)]` instead, and require a `// SAFETY:` comment on every block. Decide now so it is not negotiated per-PR.
2. **Initial version numbers.** `0.1.0` across the board, or track the ADE's `0.24.x`? They are separate products with separate release cycles — suggest `0.0.0` for unpublished crates and `0.1.0` for the six published ones, so an accidental publish is obvious.
3. **`rust-version` floor.** Pick one now (wasmtime and the component model set the real floor) and put it in `[workspace.package]`.
4. **Does `orrery-cli` belong in `core/crates/`?** It is a binary, not a library, and arguably a client. Keeping it in `core/` is simpler and matches `00-overview.md`'s table. Confirm, or move it to `clients/cli/` before anything depends on the path.
