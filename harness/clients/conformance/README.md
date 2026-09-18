# Client conformance fixtures

Renderer tests are **data first, pixels second**. Every client — `sdk-rs`, `sdk-ts`,
ratatui, Ink, json — replays these fixtures and asserts the same `SurfaceStore` state.

[`../../docs/plans/08-protocol-transport.md`](../../docs/plans/08-protocol-transport.md)
task 1 writes the fixtures. This file fixes their format so it cannot drift.

## Format

One case per directory:

```
<case-name>/
  events.jsonl     # the AG-UI event script, one JSON object per line, in seq order
  expect.json      # the expected SurfaceStore state, per step
  README.md        # one line: what this case pins
```

- `events.jsonl` — line-delimited AG-UI events exactly as `orrery-agui` encodes them.
  Lines are applied in file order; `seq` is present and monotonic.
- `expect.json` — an array. Entry *i* is the full expected `SurfaceStore` state **after**
  applying line *i* of `events.jsonl`, or `null` where a step asserts nothing. Surface
  ids are stable strings, not generated, so the file is diffable.
- Unknown event types must be ignored by a conforming client, not rejected: a case may
  include one deliberately.

A client passes the suite when, for every case, replaying `events.jsonl` produces exactly
the non-`null` states in `expect.json`. Drawing snapshots (ratatui `TestBackend` + `insta`,
Ink `lastFrame()`) live with each client and are a second layer on top of this one.
