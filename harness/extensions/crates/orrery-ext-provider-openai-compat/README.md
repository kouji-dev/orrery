# orrery-ext-provider-openai-compat

An OpenAI-compatible chat-completions provider, for local models behind ollama or vllm.

**Manifest field.** `providers` — this crate is a first-party implementation of
`ExtensionDefinition.providers`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
[`harness/docs/plans/03-provider-layer.md`](../../../docs/plans/03-provider-layer.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.
