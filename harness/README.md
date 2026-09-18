# Orrery Harness

A Rust agent runtime an organisation configures into the coding agent it actually wants:
the loop is assembled from declared parts, extensions run out of process in any language,
permissions are enforced in the core rather than configured around it, and benchmarks are a
built-in capability rather than a throwaway script.

It is a **separate product from Orrery ADE** (the Tauri + Angular app in `ade/`).

## Layout

| Path | What lives there |
|---|---|
| `core/crates/` | Kernel-owned crates: traits, the loop, the hosts, policy, transport. |
| `extensions/crates/` | First-party Rust extensions, written as if a third party owned them. |
| `extensions/node/` | First-party TypeScript extensions and the `@orrery/ext` SDK. |
| `extensions/examples/` | Wasm (Rust, TinyGo) and process samples. |
| `clients/` | Every renderer, one folder each. See `clients/README.md`. |
| `xtask/` | `deps-check` · `typegen` · `wit-check` · `agui-drift`. |
| `wit/` | `orrery-extension.wit`, the component-model world. |
| `protocol/` | Generated schema and `.d.ts` (`@orrery/protocol`). Not hand-edited. |
| `docs/` | The specification and the plans. Start at [`docs/README.md`](docs/README.md). |

## Build

Cargo runs from the **repo root**, not from here. `default-members` excludes the ADE, so:

```bash
cargo build                      # every harness crate, no Tauri code
cargo test -p orrery-proto       # per-crate; never a full-workspace test run
cargo run -p orrery-cli -- --help
cargo xtask deps-check           # the core/extensions direction rule
```

The direction rule `deps-check` enforces: a crate in `extensions/` may depend on `core/`;
**no `core/` crate may depend on an `extensions/` crate** — except `orrery-harness`, the
facade, which links the first-party set behind cargo features.

## Docs

[`docs/README.md`](docs/README.md) indexes the architecture and every implementation plan.
