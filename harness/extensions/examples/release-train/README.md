# release-train — a ported example extension

A release script's checklist, its `\r`-redrawn bar and its ASCII timeline,
ported. The first two are core surfaces. The third is not — so this extension
ships a **custom** one, and the fallback that has to stand in for it.

```
status { "release": "v0.25.0", "stages": [ { "id": "ink", "label": "ink client", "status": "pending" } ] }
```

| Surface | Why it |
|---|---|
| `task` | A checklist with stable item ids, so one item can be patched. |
| `progress` | `done`/`total`. No bar, no width, no carriage return. |
| `custom` (`example-release-train.timeline`) | Stages on a time axis: the vocabulary has no such surface, and inventing one for every extension's pet visual is what `custom` exists to avoid. |

**The fallback rule this extension keeps:** it names everything the payload
names. Every stage, with its state, plus where the train has got to. `--json`
prints it beside the payload, so a lazy fallback shows up in CI (§6.2), and
`ported::the_fallback_is_informative` asserts it stage by stage.
