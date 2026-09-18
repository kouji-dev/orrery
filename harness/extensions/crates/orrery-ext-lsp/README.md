# orrery-ext-lsp

Language-server tools: hover, definitions, references and diagnostics over a managed LSP client.

**Manifest field.** `tools` — this crate is a first-party implementation of
`ExtensionDefinition.tools`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Implemented: `hover`, `definition`, `references`, `diagnostics`.
Implementation plan:
[`harness/docs/plans/06-extension-host.md`](../../../docs/plans/06-extension-host.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.

## What it asks for, and why

| Capability | Why |
|---|---|
| `read = ["$WORKSPACE/**"]` | A language server is told about files by content; `didOpen` carries the text, which has to be read first. |
| `spawn = ["*"]` | The server binary is named by configuration — `rust-analyzer`, `gopls`, `pyright` — so the program cannot be pinned here. Policy pins it where the name is actually known. |

## Tests

```
cargo test -p orrery-ext-lsp
```

Nothing yet: this crate is a scaffold. When it lands, its tests drive a stub
server over the mock broker — no real toolchain, no network.

## `symbols` is not here

The scaffold's manifest declared it. `workspace/symbol` needs an indexing story
this round did not build, so it is not declared: a manifest that lists a tool the
extension does not have makes the ledger a lie.

## Where the framing lives, and where it should live

Plan 06 puts `Content-Length` framing in `orrery-jsonrpc`. That crate is
`publish = false`, and `cargo xtask deps-check` rule 2 forbids an extension from
depending on an unpublished core crate — a community author has to be able to
build this against crates.io. So `src/framing.rs` is sixty lines, moved from
`ade/src-tauri/src/lsp/transport.rs`. **When `orrery-jsonrpc` publishes, delete
that module and use its**: `Framed` is the only thing in the crate that touches
bytes.

## Tests

```
cargo test -p orrery-ext-lsp
```

Twenty-one cases, and **no process is started and nothing is installed**. The four
verbs run against a fake language server in the test process — the only way
`rust-analyzer`-shaped behaviour (a `LocationLink` rather than a `Location`,
diagnostics pushed some time after `didOpen`) is reachable on a machine that has
no language server on it. Framing is tested separately over `&[u8]`, because it
is the one part a fake cannot stand in for.
