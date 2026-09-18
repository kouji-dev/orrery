# orrery-ext-lsp

Language-server tools: hover, definitions, references and diagnostics over a managed LSP client.

**Manifest field.** `tools` — this crate is a first-party implementation of
`ExtensionDefinition.tools`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
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
