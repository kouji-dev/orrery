# orrery-ext-tools-builtin

The built-in tool bundle: read, write, edit, bash, grep and glob, shipped as a first-party extension like any other.

**Manifest field.** `tools` — this crate is a first-party implementation of
`ExtensionDefinition.tools`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
[`harness/docs/plans/06-extension-host.md`](../../../docs/plans/06-extension-host.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.
