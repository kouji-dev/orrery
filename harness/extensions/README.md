# Orrery extensions

Every crate under `crates/` is written as if a third party owned it, so a community
extension is literally the same crate in someone else's repo.

## How to write one

Filled in by [`../docs/plans/18-writing-an-extension.md`](../docs/plans/18-writing-an-extension.md).

The rules that already hold:

- Extensions depend on core through **published** crates only: `orrery-ext-api`,
  `orrery-proto`, and the trait crates (`orrery-provider`, `orrery-session`,
  `orrery-memory`, `orrery-grader`).
- Dependencies are declared with **both** halves —
  `orrery-ext-api = { version = "0.1", path = "../../../core/crates/orrery-ext-api" }`.
  `version` makes them publishable, `path` makes them build in-tree.
- No extension reaches into `core/` by relative path for anything else: no shared
  `build.rs`, no `include!`, no dev-dependency on `orrery-kernel`.
- Each extension carries its own `README.md`, `orrery.toml` and `CHANGELOG.md`.

`cargo xtask deps-check` enforces the first three.

## Layout

| Path | What |
|---|---|
| `crates/` | First-party Rust extensions, `runtime = "native"`. |
| `node/` | First-party TypeScript extensions and the `@orrery/ext` SDK. |
| `examples/` | Wasm (Rust, TinyGo) and process samples. |

## Installing one, and what the registry does not cover

`orrery install <source>` takes seven forms (plan 15's Architecture table).
**Only one of them is verified**: a bare registry name resolves against a signed
index, which pins a `sha256` and mirrors the manifest's `requires` so a
capability change is visible without downloading the package. The other six —
`github:`, a git URL, `crate:`, `npm:`, a local path, and `--link` — have no
signature and no hash to check against, so they install `pinned: false`, say so
in the supply-chain ledger, and are refused outright under a managed
`registry.unpinned = "refuse"`. The rule in one line: **the registry is how you
trust an extension; the other sources are how you try one.**

Worth saying plainly, because the registry does not solve it: a git or local
install pulls a **transitive dependency tree nobody here reviews**. We pin the
extension; its crates.io or npm dependencies are pinned by its own lockfile,
which is not part of the review. A malicious transitive dependency is still a
hole. The wasm runtime closes it — a guest reaches the host only through the
`wit` world — and `native`, `node`, `python` and `process` do not. That is the
threat model; implying otherwise would be the bug.
