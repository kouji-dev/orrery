# Orrery clients

**One renderer per folder.** Every renderer is a whole AG-UI client; none is privileged.
The one compiled into the `orrery` binary attaches over the in-process transport exactly as
an external one attaches over a pipe.

| Folder | Crate / package | What it owns |
|---|---|---|
| `sdk-rs/` | `orrery-client` | Rust `AguiSession` + `SurfaceStore`. Pure data, no drawing. |
| `sdk-ts/` | `@orrery/client` | The same two things in TS, on stock `@ag-ui/client`. |
| `ratatui/` | `orrery-client-ratatui` | Default TUI, linked into the `orrery` binary. |
| `json/` | `orrery-client-json` | Line-delimited AG-UI events. Backs `run --json`, CI, evals. |
| `ink/` | `@orrery/client-ink` | React/Ink TUI, spawned as a Node process. |
| `ade/` | pointer | The Angular + kouji-ui renderer is built inside `ade/`. |
| `conformance/` | fixtures | Event scripts + expected `SurfaceStore` state. Every client runs them. |

## AG-UI state flows outward only

From [`../docs/plans/08-protocol-transport.md`](../docs/plans/08-protocol-transport.md):
`orrery-agui` is an **encoder only**. Frames become AG-UI events on the way out; nothing
decodes AG-UI back into kernel state. A client's `SurfaceStore` is a projection it rebuilds
from the event stream — it is never authoritative, and a client never mutates shared state
by emitting AG-UI. Everything a client wants to *do* travels the other way, as a `Request`
on the control channel.

`seq` is assigned once, per session, at the differ's output — never per connection — so a
re-attaching client asks for `since = <last seq it saw>` and gets exactly the gap.
