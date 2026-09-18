# orrery-ext-graders

The built-in graders: command, assertion and model.

**Manifest field.** `graders` — this crate is a first-party implementation of
`ExtensionDefinition.graders`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
[`harness/docs/plans/16-eval-runner.md`](../../../docs/plans/16-eval-runner.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.
