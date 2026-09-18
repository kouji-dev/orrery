# orrery-ext-provider-fixture

A scripted provider that replays a recorded ModelEvent stream from a .jsonl file. Deterministic, no API key.

**Manifest field.** `providers` — this crate is a first-party implementation of
`ExtensionDefinition.providers`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
[`harness/docs/plans/03-provider-layer.md`](../../../docs/plans/03-provider-layer.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.
