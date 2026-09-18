# orrery-ext-views-default

The default view bindings: assistant text, tool started and settled, consent prompts and errors.

**Manifest field.** `views` — this crate is a first-party implementation of
`ExtensionDefinition.views`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

## Why it is an extension

Unbound loop events are hidden. A client with nothing bound would show nothing at all,
which is broken rather than minimal — so something has to ship the floor. The tempting
shortcut is to install it inside the kernel, and then there is one view path for the five
bindings everybody needs and a different one for everybody else's.

So the floor loads the way a third-party bundle does: an [`orrery.toml`](orrery.toml), a
manifest parsed by the same parser, a load through `orrery-host`, an entry in the ledger,
and a deny rule that switches it off. `tests/views.rs` asserts exactly that, because the
claim is about the loading path and not about the bindings.

The binding *content* is [`orrery_surface::floor()`](../../../core/crates/orrery-surface/src/view.rs),
kept next to the surface vocabulary it is written in so the two cannot drift. **Nothing in
the kernel calls it**; this extension is the only thing that does.

## What it binds

| Event | Surface | Placement |
|---|---|---|
| `assistant.text` | `markdown`, `complete` false while streaming | inline |
| `tool.started` | `text`, muted, status `running` | inline |
| `tool.settled` | the outcome's own surface, or a styled line per outcome | inline |
| `consent.request` | `question` with the four consent answers and a `deny` default | inline |
| `error` | `text`, error style, naming the code and how far it reaches | inline |

A profile moves or silences any of them without touching this crate:

```toml
[views."tool.settled"] placement = "footer"
[views."tool.started"] placement = "hidden"
```

Implementation plan:
[`harness/docs/plans/09-surfaces.md`](../../../docs/plans/09-surfaces.md).
