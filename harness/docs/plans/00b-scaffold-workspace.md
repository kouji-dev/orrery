# 00b · Scaffold — every crate as an empty, compiling stub

**Goal.** Create the whole harness skeleton in one pass: the `core/` · `extensions/` · `clients/` tree, every crate's `Cargo.toml` and `lib.rs` with a pointer to its plan, the publish flags, the manifests, the pnpm packages, `xtask` with a working `deps-check`, the WIT skeleton and the CI job. Nothing implements anything. When this is done, `cargo test` at the root builds the whole harness, `cargo run -p orrery-cli -- --help` prints the full command tree, and `cargo xtask deps-check` is green.

**Covers.** No architecture section — this is the scaffold [`00-overview.md`](00-overview.md) describes.

**Depends on.** [`00a-restructure-ade.md`](00a-restructure-ade.md) must land first.

**Why scaffold everything at once rather than crate-by-crate.** The dependency direction rules (`core` never depends on `extensions`; extensions depend only on published crates) are only checkable when the whole graph exists. Creating them together and turning on `deps-check` immediately means the boundary is enforced from the first commit rather than discovered later.

---

## Tasks

### Task 1 · The directory tree

- [x] Create:
  ```
  harness/core/crates/          harness/extensions/crates/
  harness/extensions/node/      harness/extensions/examples/
  harness/clients/{sdk-rs,sdk-ts,ratatui,json,ink,ade,conformance}/
  harness/xtask/  harness/wit/  harness/protocol/
  ```
- [x] `harness/README.md` — what the harness is, how to build it, and a pointer to `docs/README.md`.
- [x] `harness/extensions/README.md` — a stub carrying the "how to write one" heading; [`18`](18-writing-an-extension.md) fills it.
- [x] `harness/clients/README.md` — the one-renderer-per-folder rule, and the AG-UI outward-only state note from [`08`](08-protocol-transport.md).
- [x] `harness/clients/conformance/README.md` — the fixture format; [`08`](08-protocol-transport.md) task 1 writes the fixtures.

### Task 2 · Root workspace

- [x] Extend the root `Cargo.toml` created by `00a`:
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
- [x] Add every dependency from [`00-overview.md`](00-overview.md)'s table to `[workspace.dependencies]`, pinned. Crates reference them as `{ workspace = true }`.
- [x] `[workspace.package]` with shared `edition`, `license`, `repository`, `rust-version`.
- [x] **Verify:** `cargo build` at the root builds no Tauri code.

### Task 3 · Core crates

One directory per row of `00-overview.md` table 1.

- [x] For each: `Cargo.toml` (workspace-inherited metadata, no dependencies beyond what compiles empty) and `src/lib.rs` containing only:
  ```rust
  //! <one line: what this crate owns>
  //!
  //! Implementation plan: `harness/docs/plans/<NN>-<name>.md`
  #![deny(missing_docs)]
  #![forbid(unsafe_code)]    // omit where a crate genuinely needs unsafe: host-wasm, broker
  ```
- [x] Set `publish = true` on exactly: `orrery-proto`, `orrery-ext-api`, `orrery-provider`, `orrery-session`, `orrery-memory`, `orrery-grader`. Everything else in `core/` gets `publish = false`.
- [x] `orrery-cli` gets `[[bin]] name = "orrery"`.
- [x] **Verify:** `cargo check` passes for all of them.

### Task 4 · Extension crates

One per row of table 2.

- [x] For each: `Cargo.toml`, `orrery.toml`, `README.md`, `src/lib.rs` stub.
- [x] Dependencies on core use **both** halves — `orrery-ext-api = { version = "0.1", path = "../../../core/crates/orrery-ext-api" }`. This is the rule that makes the eventual repo split free.
- [x] Each `orrery.toml` declares `api = "orrery-ext/1"`, `runtime = "native"`, its `[provides]` and its `[requires]` — even though nothing reads them yet. Writing them now means [`06`](06-extension-host.md) has real fixtures to parse on day one.
- [x] `orrery-ext-memory-file` is **not** in any default feature set (see [`12`](12-memory.md)).

### Task 5 · Client crates and packages

- [x] `clients/sdk-rs` (`orrery-client`, `publish = true`), `clients/ratatui` (`orrery-client-ratatui`), `clients/json` (`orrery-client-json`) as empty Rust crates.
- [x] `clients/sdk-ts` (`@orrery/client`) and `clients/ink` (`@orrery/client-ink`) as `package.json` + `README.md` + `tsconfig.json` stubs, private for now.
- [x] `clients/ade/README.md` — a pointer saying the Angular renderer lives in `ade/` and builds on `@orrery/client`.

### Task 6 · `orrery-cli` command tree

- [x] Implement the full clap tree from [`17`](17-cli.md), with every subcommand present.
- [x] Each unimplemented subcommand exits 2 with `not implemented in this build — see harness/docs/plans/<NN>-<name>.md`. The tree is complete from day one and fills in plan by plan.
- [x] **Failing test first.** `cli::help_snapshot` — snapshot `--help` and each subcommand's help.
- [x] **Verify:** `cargo run -p orrery-cli -- --help` prints the tree.

### Task 7 · `xtask`

- [x] `harness/xtask` with subcommands `deps-check`, `typegen`, `wit-check`, `agui-drift`.
- [x] **`deps-check` is implemented for real** — it is the only thing here that enforces a rule. Over `cargo metadata`:
  1. No `core/` crate depends on an `extensions/` or `clients/` crate. Allow-list exactly one exception: `orrery-harness`.
  2. Every `extensions/` → `core/` dependency names a `publish = true` crate **and** carries a `version`.
  3. No `extensions/` crate dev-depends on an unpublished core crate.
- [x] **Failing test first.** Fixture workspaces under `xtask/tests/fixtures/` that violate each rule; assert each is caught and the message names both crates. ([`18`](18-writing-an-extension.md) task 2 owns these tests; write them here and let that plan extend them.)
- [x] `typegen`, `wit-check`, `agui-drift` are stubs that exit 0 with "not implemented", each naming its plan.

### Task 8 · WIT and protocol

- [x] `harness/wit/orrery-extension.wit` — the world sketched in [`14`](14-wasm-wit.md), including the flat-arena surface. It did not have to be final then; it has to exist so `wit-check` has a target and so the arena question is visible rather than deferred silently. **It is final now** — plan 14 froze the arena, corrected `node-kind` to the real twelve variants and gave `wit-check` a real implementation.
- [x] `harness/protocol/package.json` (`@orrery/protocol`, private, `types: protocol.d.ts`) and a `README.md` saying the contents are generated by `cargo xtask typegen` and must not be hand-edited.

### Task 9 · pnpm workspace

- [x] `pnpm-workspace.yaml` — extend the `packages:` list `00a` created:
  ```yaml
  packages:
    - ade
    - harness/protocol
    - harness/clients/sdk-ts
    - harness/clients/ink
    - harness/extensions/node/*
  ```
- [x] `pnpm install --lockfile-only` and commit.

### Task 10 · CI

- [x] `.github/workflows/test.yml` — add a `harness` job, independent of the ADE job:
  ```yaml
  - run: cargo fmt --all -- --check
  - run: cargo clippy --workspace --exclude orrery-ade --exclude orrery-updater -- -D warnings
  - run: cargo test -p orrery-proto
  - run: cargo xtask deps-check
  ```
- [x] Rust cache keyed on the root `Cargo.lock`, shared with the ADE job.
- [x] **Do not** add `cargo test --workspace` — the repo convention is per-crate test runs, and plans add their own jobs as they land.

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

- [x] Every crate in `00-overview.md`'s three tables exists, and every one names its plan file in its `lib.rs`.

---

## Done when

- `cargo test` at the root builds the whole harness and no Tauri code.
- `cargo run -p orrery-cli -- --help` prints the complete command tree.
- `cargo xtask deps-check` passes, and demonstrably fails on each of its three fixture violations.
- `cargo publish --dry-run` succeeds for every `publish = true` crate.
- The ADE still builds and tests exactly as it did after `00a`.

## Open questions

All four decided while executing this plan. Recorded here rather than renegotiated per-PR.

1. **`#![forbid(unsafe_code)]` exceptions.** **Decided: two crates, `orrery-broker` and
   `orrery-host-wasm`, carry `#![deny(unsafe_op_in_unsafe_fn)]` instead of
   `#![forbid(unsafe_code)]`.** Every other crate in the tree — core, extensions and
   clients — forbids `unsafe` outright. The two exceptions are written into their
   `lib.rs` with a comment pointing back at this question, so adding a third is a visible
   diff rather than a habit. Every `unsafe` block in those two crates carries a
   `// SAFETY:` comment; plans 07 and 14 enforce it in review.

2. **Initial version numbers.** **Decided: `0.0.0` for unpublished crates, `0.1.0` for the
   published ones.** The harness and the ADE are separate products with separate release
   cycles, so the harness does not track `0.24.x`. The split makes an accidental publish
   obvious: a crate at `0.0.0` has no business on crates.io, and `publish = false` says so
   twice. The published set is exactly `orrery-proto`, `orrery-ext-api`, `orrery-provider`,
   `orrery-session`, `orrery-memory`, `orrery-grader` — plus `orrery-client` in
   `clients/sdk-rs`, which table 3 also marks published.

3. **`rust-version` floor.** **Decided: `1.86`, with `edition = "2024"`**, both in
   `[workspace.package]`. Edition 2024 needs 1.85; wasmtime and the component model set the
   real floor and track roughly N-2, so 1.86 is the first version that is comfortably above
   both without pinning us to a compiler nobody has yet. The ADE keeps `edition = "2021"`
   and does not inherit this — it is not a member of `[workspace.package]`'s audience.

4. **Does `orrery-cli` belong in `core/crates/`?** **Decided: yes, it stays at
   `core/crates/orrery-cli`**, as table 1 has it. It is `publish = false` like every other
   kernel-internal crate, and it is not a *client* in the sense `clients/` means — it does
   not render anything. It **links** two clients (`orrery-client-ratatui`,
   `orrery-client-json`) and starts a kernel. Moving it to `clients/cli/` would put a
   binary that depends on `orrery-harness` inside the directory whose rule is "one renderer
   per folder", and would make `deps-check`'s rule 1 read strangely — a `clients/` crate
   depending on all of core. Confirmed before anything depends on the path.

## Decisions taken outside the questions above

- **`harness/clippy.toml`.** Clippy uses the first `clippy.toml` it finds walking up from a
  package, so this file shadows the root one for everything under `harness/`. The root file
  bans `std::process::Command::new` and `tauri::Emitter::emit` in favour of ADE modules that
  do not exist here. Spawning in the harness is restricted at the broker instead. See the
  file's own comment.
- **`.cargo/config.toml`.** Adds the `cargo xtask` alias this plan's verification block
  assumes.
- **`deps-check` runs `cargo metadata --no-deps`**, so it resolves nothing against a
  registry and works offline, in CI and on a fixture workspace alike. "Carries a version" is
  read as "the dependency's requirement is not `*`".
- **Six fixture workspaces**, not three: rule 2 has two halves (published target, and a
  version requirement) and rule 1 has an allow-list entry, so there is a fixture for the
  `orrery-harness` exception and a `ok/` fixture that must stay green. `18` extends them.
- **`orrery-guest`**, the wasm guest SDK named in plan 14, is **not** scaffolded here: it is
  in no table of `00-overview.md`, and plan 14 creates it.

---

## State

**Landed, and since outgrown in the way a scaffold should be.** Every directory
this plan created exists and holds real code: `harness/core/crates/*`,
`harness/extensions/`, `harness/clients/`, `harness/xtask/`, `harness/wit/` and
`harness/protocol/`.

The four xtask subcommands it scaffolded are the part worth recording, because
three of them were explicitly stubs that exited 0 with "not implemented":

- `deps-check` — real from the start, and still the enforcement point for the
  dependency direction. Nine tests, one fixture workspace per broken rule.
- `typegen` — **implemented.** Regenerates `harness/protocol` from the
  `orrery-proto` schemars derives.
- `wit-check` — **implemented.** Runs `orrery-wit`'s drift tests: the `.wit`
  parses, the embedded world matches the file on disk, the frozen shapes match
  the Rust, and (since 2026-09-19) the guest SDK's vendored copy is byte-identical
  to the canonical file.
- `agui-drift` — **implemented**, offline by default, `--fetch` for CI.

`publish-check` was added later and is not in this plan's list; it is plan 18's.

No stubs remain from this wave.
