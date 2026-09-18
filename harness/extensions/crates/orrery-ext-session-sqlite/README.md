# orrery-ext-session-sqlite

The default session backend: SQLite in WAL mode, one transaction per turn append, a writer actor per session.

**Manifest field.** `session` — this crate is a first-party implementation of
`ExtensionDefinition.session`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
[`harness/docs/plans/02-session-store.md`](../../../docs/plans/02-session-store.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.
