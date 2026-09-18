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

## The set is exact, and `streams/` is not part of it

Every client asserts the **same** count, from one named constant per language —
`orrery_client::conformance::SCENARIO_COUNT` and `@orrery/client`'s
`conformance.SCENARIO_COUNT`, both currently **17**. It is an equality, not a floor.
Floors were how this drifted: ratatui asked for at least 16 and Ink for at least 10, so
deleting six fixtures would have turned one suite red and left the other quiet. Adding a
scenario means adding a row below and bumping both constants in the same commit; the set is
a contract, not a directory listing.

**`streams/` is deliberately not run by the renderers, and its absence is not a gap.** The
six files under [`streams/`](streams/README.md) are `ModelEvent` scripts crossing the
*provider* boundary, replayed by `orrery-ext-provider-fixture` to drive a whole turn
end-to-end. The files here are AG-UI event scripts crossing the *client* boundary, replayed
against a `SurfaceStore`. Different format, different boundary, different assertions — a
renderer has nothing to do with a `ModelEvent`. `load_all` / `loadAll` therefore read this
directory only and never recurse, which is why the count above is 17 and not 23.

The two sets stay related in the way that matters: every scenario below names the stream it
is derivable from, so the same turn can be driven through the fixture provider or replayed
as events alone.

## The scenarios

Each is derivable from one of [plan 03's six provider streams](streams/README.md), so the
same turn can be driven end-to-end through `orrery-ext-provider-fixture` or replayed as
events alone.

| Scenario | Derived from | What it pins |
|---|---|---|
| `text-only` | `text-turn` | The baseline: a turn that is one assistant message. |
| `tool-call` | `tool-call` | Arguments split mid-token become one code child; the result a second. |
| `tool-args-interleaved` | `tool-call` | Two calls open at once: each input rebuilds from its own `toolCallId`. |
| `streaming-markdown` | `text-turn` | A half-open code fence stays `complete: false`. Also the unknown-event rule. |
| `consent-prompt` | `tool-call-consent` | Consent rides `Custom`, and the answer comes back the same way. |
| `question-surface` | `text-turn` | A `replace` at `/surfaces/<id>` creates as well as swaps. |
| `table-then-resort` | `tool-call` | The cost guard: a re-sort is one op on `/kind/rows`. |
| `seq-gap` | `text-turn` | A lost frame is detected by arithmetic and the event is still applied. |
| `reattach-since` | `text-turn` | Replay picks up mid-turn, into the `(detached)` placeholder turn. |
| `cancel-midturn` | `never-ends` | A stopped turn closes its message as `cancelled`, never as `done`. |
| `custom-with-fallback` | `tool-call` | Payload and fallback are kept side by side; the store picks neither. |
| `tree-surface` | `tool-call` | Opening one node is one op at that node's `expanded`. |
| `diff-surface` | `tool-call` | A second hunk is one op on `/kind/hunks`; the first is not re-sent. |
| `progress-surface` | `never-ends` | A tick is one op on `/kind/done`, never a new surface. |
| `stream-surface` | `tool-call` | A channel is named, opens `running` and is closed by the turn. |
| `task-surface` | `tool-call` | Ticking an item is one op; adding one is an op on the array. |
| `form-surface` | `tool-call` | All five field kinds, and the sequential-prompt degradation rule. |

### One scenario per core surface

Every core surface has a scenario, and every renderer implements every core surface — one
that cannot is not a renderer. [Plan 09](../../docs/plans/09-surfaces.md) task 7 added the
six that were missing; plans 09b and 09c run this whole directory, so a surface with no row
here is a surface two TUIs can quietly disagree about.

| Surface | Scenario |
|---|---|
| `text` | `text-only` |
| `markdown` | `streaming-markdown` — the `complete` flag, explicitly |
| `table` | `table-then-resort` — the cost guard, explicitly |
| `tree` | `tree-surface` |
| `diff` | `diff-surface` |
| `progress` | `progress-surface` |
| `stream` | `stream-surface` |
| `task` | `task-surface` |
| `question` | `question-surface` |
| `form` | `form-surface` |
| `stack` | `tool-call` — a call and its result are children of one stack |
| `custom` | `custom-with-fallback` |

`table-then-resort` and `streaming-markdown` are called out because they are the two cases
renderers get wrong: the first tempts a full rebuild on every sort, and the second tempts a
markdown parser at a half-open fence.

**One known gap.** `stream-surface` cannot assert a stream's *body*:
[`SurfaceKind::Stream`](../../core/crates/orrery-proto/src/surface.rs) names a channel and
carries no text, so the store has nowhere to put the bytes. The scenario pins everything
else about a stream. Closing it means a protocol change, not a fixture.

Adding one: give it a name that says what it pins, add a row here, and make sure it is
derivable from a stream — a scenario nothing can drive end-to-end is a scenario that will
quietly stop describing the harness.

Drawing snapshots (ratatui `TestBackend` + `insta`, Ink `lastFrame()`) live with each client
and are a second layer on top of this one.
