# orrery-ext-provider-anthropic

The Anthropic Messages provider: request builder, SSE parser, tool-use blocks, cache_control and usage.

Hand-written: `reqwest` on the workspace's rustls setup, a `POST /v1/messages` body built
out of `serde_json`, and a fifty-line SSE splitter. No vendored SDK — the whole client is
smaller than the translation layer one would need, and it fails in this harness's
vocabulary instead of somebody else's.

Three things it does that are easy to get wrong:

- **It never sleeps and never retries.** A 429 becomes `ProviderError::RateLimited` and
  the kernel decides what that is worth. A provider that retried on its own would spend a
  budget nobody agreed to.
- **It never sees a credential.** `auth::ApiKeyAuth` asks a named `creds` grant; the key
  is not read off disk, out of config or out of this crate's own memory.
  `EnvCredStore` is a dev-only stand-in, marked `TODO(plan-07)`, and goes when the broker
  lands.
- **Dropping the stream aborts the request.** The response lives in a task owned by a
  child `CancellationToken` with a drop guard, so a cancelled turn closes the socket
  rather than quietly finishing at full price. `tests/cancel.rs` proves it against a
  loopback server that watches for the close.

SSE is parsed by hand rather than with `eventsource-stream`; the reasoning is recorded in
[the plan](../../../docs/plans/03-provider-layer.md), task 5.

**No test makes a network request.** The `.sse` fixtures under `tests/fixtures/` are
recorded responses, committed, and replayed in 1-, 7-, 13- and 64-byte chunks to prove the
parser does not depend on framing.

**Manifest field.** `providers` — this crate is a first-party implementation of
`ExtensionDefinition.providers`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**A community provider is exactly this shape.** An `orrery.toml` naming what it provides
and what it requires — here `net = ["https://api.anthropic.com/**"]` and
`creds = ["anthropic"]`, and the host holds it to both — plus one type implementing
`orrery_provider::Provider`. There is no privileged path a first-party provider takes and
a third-party one cannot. The smallest worked example is
[`orrery-ext-provider-fixture`](../orrery-ext-provider-fixture/README.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.
Implementation plan: [`harness/docs/plans/03-provider-layer.md`](../../../docs/plans/03-provider-layer.md).
