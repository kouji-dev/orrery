# Recorded model streams

Six `.jsonl` files, one per shape of turn the harness has to survive. They are replayed by
[`orrery-ext-provider-fixture`](../../../extensions/crates/orrery-ext-provider-fixture) and
consumed by plans 05 (kernel loop), 08 (transport) and 09b/09c (clients), so that a test
about compaction is a test about compaction rather than a test about whichever sentence a
model happened to produce that afternoon.

```
orrery serve --provider fixture:harness/clients/conformance/streams/tool-call.jsonl
```

These are **model** streams — `ModelEvent`s crossing the provider boundary. The AG-UI
event scripts the clients replay are the `<case-name>/events.jsonl` files described in
[`../README.md`](../README.md); the two formats are deliberately different because they
pin different boundaries.

## Format

One JSON object per line. Blank lines and lines starting with `#` are ignored.

A line is either an **event** or a **failure**:

- **Event** — the object is a `ModelEvent` exactly as `orrery-provider` serialises it,
  internally tagged on `t`:
  `{"t":"text-delta","text":"hello"}`, `{"t":"done","stop":"end-turn"}`.
  The fixture format *is* the type; there is no parallel schema to drift from it.
- **Failure** — `{"error":{"code":"rate_limited","retry_after_ms":1200}}`. The stream
  yields that `ProviderError` and then ends, because a real one would. `code` is
  `ProviderError::code()`, so the fixture vocabulary and the audit vocabulary are one
  list. Extra keys carry the variant's fields: `retry_after_ms`, `status`, `elapsed_ms`,
  `tokens`, `max`, `message`.

Either kind may carry `"delay_ms"`, the pause *before* that line is emitted. Delays are
small (5ms) so the corpus stays fast; `never-ends.jsonl` is the one exception, and that is
the point of it.

Every line is parsed when the file is loaded, not when it is reached — a typo is a load
error naming the line, never a surprise in the middle of somebody else's test.

## The six

| File | What it pins |
|---|---|
| `text-turn.jsonl` | A plain text turn: started, two deltas, usage, `end-turn`. The baseline. |
| `tool-call.jsonl` | One read-only tool call, arguments split mid-token across two fragments. Stops on `tool-use`. |
| `tool-call-consent.jsonl` | A destructive `builtin.shell` call (`rm -rf target`) — the one policy must stop to ask about. Also the only fixture with a non-zero `cache_hits`. |
| `max-tokens.jsonl` | A turn truncated by the output ceiling, ending mid-word on `max-tokens`. Nothing downstream may render it as a finished answer. |
| `retryable-error.jsonl` | A stream that dies partway through with a `429` carrying `Retry-After`. Text was already emitted, so the partial turn has to be dealt with. |
| `never-ends.jsonl` | A one-hour delay after the first delta. For cancellation tests: a correct client ends promptly, a broken one hangs the suite. |

Adding a seventh: give it a name that says what it pins, add a row here, and add it to the
replay case in `orrery-ext-provider-fixture/tests/fixture.rs` so it cannot rot.
