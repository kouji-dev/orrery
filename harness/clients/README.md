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
| `ported/` | `orrery-ported` | Phase 4: the three ported example extensions, run through `orrery-host` and turned into frames. Dev-only; the renderers' test suites draw what it produces. |

## AG-UI state flows outward only

From [`../docs/plans/08-protocol-transport.md`](../docs/plans/08-protocol-transport.md):
`orrery-agui` is an **encoder only**. Frames become AG-UI events on the way out; nothing
decodes AG-UI back into kernel state. A client's `SurfaceStore` is a projection it rebuilds
from the event stream — it is never authoritative, and a client never mutates shared state
by emitting AG-UI. Everything a client wants to *do* travels the other way, as a `Request`
on the control channel.

`seq` is assigned once, per session, at the differ's output — never per connection — so a
re-attaching client asks for `since = <last seq it saw>` and gets exactly the gap.

A **coalesced** frame is not a gap. A client that asked for a slow tick gets merged frames,
and a merged frame carries `merged_from` as well as `seq`; contiguity is checked against
`merged_from`, not `seq`. Without it a slow client would re-attach every tick.

## The fixtures are the contract

[`conformance/`](conformance) holds ten scenarios as line-delimited AG-UI frames plus the
`SurfaceStore` state expected at each checkpoint. `orrery-client` and `@orrery/client` both
run them; ratatui, Ink and `json` run them on top of their own drawing snapshots. A green
run in one language means nothing on its own — that is the whole point of there being two.
