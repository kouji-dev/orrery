# orrery-ext-views-default

The default view bindings: assistant text, tool started and settled, consent prompts and errors.

**Manifest field.** `views` — this crate is a first-party implementation of
`ExtensionDefinition.views`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
[`harness/docs/plans/09-surfaces.md`](../../../docs/plans/09-surfaces.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.
