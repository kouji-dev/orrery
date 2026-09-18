# Client conformance fixtures

Renderer tests are **data first, pixels second**. Every client — `sdk-rs`, `sdk-ts`,
ratatui, Ink, json — replays these fixtures and asserts the same `SurfaceStore` state.

[`../../docs/plans/08-protocol-transport.md`](../../docs/plans/08-protocol-transport.md)
task 1 writes the fixtures. This file fixes their format so it cannot drift. They are the
contract between five implementations, which is why they came before any of them.

## Format

One JSONL file per scenario, `<scenario>.jsonl`, in this directory. Each line is one of:

| Line | Meaning |
|---|---|
| `{"ev": <frame>}` | An AG-UI event, with its `seq`. Apply it. |
| `{"expect": <state>}` | The **complete** `SurfaceStore` state after every `ev` so far. |
| `# …` | A comment. Ignored. |
| *(blank)* | Ignored. |

A client passes a scenario when, after each `expect` line, its store serialises to exactly
that state. Every line is parsed when the file is loaded, not when it is reached — a typo
is a load error naming the line, never a surprise in the middle of somebody else's test.

### `ev` — the frame

An AG-UI event exactly as [`orrery-agui`](../../core/crates/orrery-agui) encodes it:
internally tagged on `type` with `SCREAMING_SNAKE_CASE` names and camelCase fields, plus
one field of ours flattened alongside:

```json
{"seq":3,"type":"TEXT_MESSAGE_CONTENT","messageId":"msg-1","delta":"The workspace has "}
```

`seq` is assigned once, per session, at the differ's output — never per connection — so it
is on the frame rather than implied by the order a socket happened to deliver things in. A
client detects a lost frame by arithmetic.

**Unknown events must be ignored, not rejected.** A scenario may carry an event `type` or a
`Custom` `name` this client has never heard of; it still consumes its sequence number.

### `expect` — the state

```json
{
  "last_seq": 6,
  "live": null,
  "gaps": [],
  "turns": [
    {
      "id": "turn-1",
      "settled": true,
      "cancelled": false,
      "error": null,
      "surfaces": [{"id": "msg-1", "status": "done", "kind": {"t": "markdown", "value": "…", "complete": true}}],
      "prompts": []
    }
  ]
}
```

- `live` — the turn the live region shows, or `null` when nothing is in flight.
- `gaps` — every `{expected, got}` the store has detected. A renderer's cue to re-attach.
- `turns` — in arrival order. `surfaces` are in insertion order, not sorted: a table that
  arrived before a diff is drawn before it.
- `kind` is [`SurfaceKind`](../../core/crates/orrery-proto/src/surface.rs), verbatim.
- Ids are opaque strings. They are stable and readable rather than generated, so the file
  is diffable; nothing may parse them.

## The scenarios

Each is derivable from one of [plan 03's six provider streams](streams/README.md), so the
same turn can be driven end-to-end through `orrery-ext-provider-fixture` or replayed as
events alone.

| Scenario | Derived from | What it pins |
|---|---|---|
| `text-only` | `text-turn` | The baseline: a turn that is one assistant message. |
| `tool-call` | `tool-call` | Arguments split mid-token become one code child; the result a second. |
| `streaming-markdown` | `text-turn` | A half-open code fence stays `complete: false`. Also the unknown-event rule. |
| `consent-prompt` | `tool-call-consent` | Consent rides `Custom`, and the answer comes back the same way. |
| `question-surface` | `text-turn` | A `replace` at `/surfaces/<id>` creates as well as swaps. |
| `table-then-resort` | `tool-call` | The cost guard: a re-sort is one op on `/kind/rows`. |
| `seq-gap` | `text-turn` | A lost frame is detected by arithmetic and the event is still applied. |
| `reattach-since` | `text-turn` | Replay picks up mid-turn, into the `(detached)` placeholder turn. |
| `cancel-midturn` | `never-ends` | A stopped turn closes its message as `cancelled`, never as `done`. |
| `custom-with-fallback` | `tool-call` | Payload and fallback are kept side by side; the store picks neither. |

Adding one: give it a name that says what it pins, add a row here, and make sure it is
derivable from a stream — a scenario nothing can drive end-to-end is a scenario that will
quietly stop describing the harness.

Drawing snapshots (ratatui `TestBackend` + `insta`, Ink `lastFrame()`) live with each client
and are a second layer on top of this one.
