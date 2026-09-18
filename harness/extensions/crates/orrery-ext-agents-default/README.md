# orrery-ext-agents-default

The shipped role agents: planner, executor, verifier, compactor and summariser.

**Manifest field.** `agents` — this crate is a first-party implementation of
`ExtensionDefinition.agents`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
[`harness/docs/plans/11-router-roles-orchestrator.md`](../../../docs/plans/11-router-roles-orchestrator.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.
