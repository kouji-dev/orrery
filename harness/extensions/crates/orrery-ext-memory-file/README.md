# orrery-ext-memory-file

The reference memory provider: file-backed, global and session scopes only. Not in any default feature set.

**Manifest field.** `memory` — this crate is a first-party implementation of
`ExtensionDefinition.memory`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
[`harness/docs/plans/12-memory.md`](../../../docs/plans/12-memory.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.
