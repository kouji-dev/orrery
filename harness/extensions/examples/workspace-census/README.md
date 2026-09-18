# workspace-census — a ported example extension

`xtask deps-check` prints a report. This is the same report **described** rather
than printed: a section holding a markdown summary, a table of crates and a
muted footer.

```
census { "crates": [ { "name": "orrery-proto", "area": "core", "published": true } ] }
```

| Surface | Why it |
|---|---|
| `stack` (section) | One foldable thing in the transcript, not three loose ones. |
| `markdown` | A sentence with two numbers in it. `complete: true` — nothing is streaming. |
| `table` | Rows under headers. Column widths are the client's business. |
| `text` (muted) | `style` is a hint about meaning; which colour that is, is the client's. |

It asks for no capabilities and imports no drawing library. The phase-4
snapshots live in `harness/clients/ported`.
