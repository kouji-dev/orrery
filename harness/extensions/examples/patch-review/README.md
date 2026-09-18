# patch-review — a ported example extension

The `[y/N]` loop, ported. A patch is a `diff` surface; the prompt is a
`question`, whose answer comes back as an **intent** — so the asking step
pauses, not the kernel.

```
review { "path": "src/lib.rs", "hunks": [ { "old_start": 1, "lines": [ { "op": "add", "text": "…" } ] } ] }
```

| Surface | Why it |
|---|---|
| `diff` | Hunks and lines as data. `+`/`-` colouring is the client's. |
| `question` | Three choices, `default: "skip"` — unattended, nothing applies a patch. |
| `stack` | The two belong together and are re-emitted together. |

A patch with no hunks is `Outcome::Failed { code: "empty-patch" }`, not an empty
diff nobody can answer about.
