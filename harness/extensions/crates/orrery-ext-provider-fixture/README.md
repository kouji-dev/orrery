# orrery-ext-provider-fixture

A scripted provider that replays a recorded ModelEvent stream from a .jsonl file. Deterministic, no API key.

```
orrery serve --provider fixture:harness/clients/conformance/streams/tool-call.jsonl
```

It ignores the request and emits exactly what the file says, at exactly the pace the file
says. That is what makes a kernel test about compaction a test about compaction, rather
than a test about whichever sentence a model happened to produce that afternoon.

The corpus it replays — six streams covering a plain turn, a tool call, a call that trips
consent, a truncated turn, a retryable failure and a stream that never ends — lives in
[`harness/clients/conformance/streams/`](../../../clients/conformance/streams/README.md),
which also documents the line format.

**Manifest field.** `providers` — this crate is a first-party implementation of
`ExtensionDefinition.providers`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**A community provider is exactly this shape.** An `orrery.toml` declaring what it
provides and what it requires, one type implementing `orrery_provider::Provider`
(`id`, `capabilities`, `stream`, `counter`, `auth`), and nothing else. There is no
registration macro, no vendored SDK and no privileged path: this crate requires
`read = ["$WORKSPACE/**"]` and no network, and the host holds it to that. Copy it,
change the five functions, and the harness cannot tell the difference.

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.
Implementation plan: [`harness/docs/plans/03-provider-layer.md`](../../../docs/plans/03-provider-layer.md).
