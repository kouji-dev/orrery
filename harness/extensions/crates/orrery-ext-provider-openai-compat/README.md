# orrery-ext-provider-openai-compat

An OpenAI-compatible chat-completions provider, for local models behind ollama or vllm.

**Manifest field.** `providers` — this crate is a first-party implementation of
`ExtensionDefinition.providers`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Implemented. Implementation plan:
[`harness/docs/plans/03-provider-layer.md`](../../../docs/plans/03-provider-layer.md),
phase 5.

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

Every case replays a committed `.sse` file under `tests/fixtures/` through
`RecordedTransport`, in seven-byte slices so a frame split across a chunk
boundary is the default rather than a case somebody remembered to write. **No
socket is opened.** That is why the HTTP side is the `ChatTransport` trait: no
machine running this suite has ollama on it.

Turn the `http` feature off and the crate is the parser, the mapper and
`RecordedTransport` — which is all a fixture replay needs, and no TLS stack.
