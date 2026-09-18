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
