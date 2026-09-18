# orrery-ext-provider-openai-compat

An OpenAI-compatible chat-completions provider, for local models behind ollama or vllm.

**Manifest field.** `providers` — this crate is a first-party implementation of
`ExtensionDefinition.providers`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
[`harness/docs/plans/03-provider-layer.md`](../../../docs/plans/03-provider-layer.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.

## What it asks for, and why

| Capability | Why |
|---|---|
| `net = ["http://localhost/**", "https://**"]` | The whole point is a server you chose — ollama on localhost, vllm on your own host — so the endpoint cannot be pinned here the way the Anthropic provider's is. Pin it in policy, where the address is actually known. |
| `creds = ["openai-compat"]` | Local servers usually want no key; hosted compatible ones do. Asking by name costs nothing when there is no key to hand over. |

The wide `https://**` is the honest cost of "compatible with whatever you are
running", and it is the one provider request in this tree that policy genuinely
has to narrow.

## Tests

```
cargo test -p orrery-ext-provider-openai-compat
```

Nothing yet: this crate is a scaffold. When it lands it is tested the way the
Anthropic provider is — committed byte streams through the parser, no request.
