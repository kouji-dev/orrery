# `@orrery/client-ink`

The React/Ink TUI. Spawned by `orrery --ui ink` with `ORRERY_ENDPOINT` in the environment,
or run standalone against `orrery serve`.

It is an AG-UI subscriber like every other renderer, built on
[`@orrery/client`](../sdk-ts). It gets no privileged path into the kernel: it attaches over
the transport exactly as the in-binary ratatui client does. Its only dependencies on this
repository are `@orrery/protocol` (generated types) and `@orrery/client` (the SDK), and
[`test/conformance.test.tsx`](test/conformance.test.tsx) fails the build if that ever stops
being true. That is the point of it: if a third party could not build this client from the
published spec, §5.6's promise would be a claim rather than a fact.

Implementation plan: [`harness/docs/plans/09c-client-ink.md`](../../docs/plans/09c-client-ink.md).

## The `ORRERY_ENDPOINT` contract

Copy this and you have written a third-party Orrery client.

| What | How |
|---|---|
| Where the kernel is | `--endpoint <url>`, else `$ORRERY_ENDPOINT`. Nothing else. |
| Which session | `--session <id>`, else `$ORRERY_SESSION`, else `default`. |
| Endpoint syntax | `http://[token@]host:port`. The token rides the authority, never a query string — a query string ends up in logs, in shell history and in `ps`. |
| Attach | `POST /control` `{"t":"session.attach","id":<uuid>,"session":<id>[,"since":<seq>]}`. |
| Subscribe | `POST /run` with an AG-UI `RunAgentInput`; the response is SSE, one `data:` line per event. |
| Resume | Every frame carries `seq`. A hole means frames were lost: re-attach with `since` = the last `seq` you actually saw. `merged_from` marks a coalesced frame and is **not** a hole. |
| Write path | `intent`, `consent.answer`, `turn.cancel` — all `POST /control`. A client never writes state: AG-UI's shared state is bidirectional, ours is not, and an edit a person makes comes back as an `intent` the kernel validates. |
| Node | 20.19 or newer (`engines`). Older Node gets a sentence, not a stack trace. |

```sh
orrery serve --listen 127.0.0.1:0          # prints an endpoint
pnpm -C harness/clients/ink start --endpoint http://127.0.0.1:PORT
```

## What it draws

One component per core surface in [`src/surfaces`](src/surfaces), each a pure function of a
`Surface`. No component holds the session: an interaction bubbles up to `<App>`, which is
the only thing that does. Settled turns go through Ink's `<Static>` — printed once into
native scrollback, never redrawn — so selection and scrolling stay the terminal's, and only
the live turn, the consent bar, the footer and the composer are redrawn. That is the same
hybrid the ratatui client gets from `insert_before`.

Adding a `SurfaceKind` to the protocol is a **compile error** here: `COMPONENTS` is a mapped
type over the union and the switch's default arm is typed `never`.

## Custom renderers

The one thing this client can do that the ratatui one will not:

```ts
import { registerRenderer } from "@orrery/client-ink";

registerRenderer("buildgraph.flamegraph", Flamegraph);
```

Without a registered renderer a `custom` surface draws its `fallback`, which is mandatory on
the wire precisely so that branch is never blank. Loading a third party's bundle needs the
signed registry and a `render` grant and is plan 15's: see `TODO(plan-15)` in
[`src/registry.ts`](src/registry.ts).

## Tests

Data first, pixels second: the [`../conformance`](../conformance) fixtures — the same files
the Rust SDK replays — assert `SurfaceStore` state at every checkpoint, then
`ink-testing-library`'s `lastFrame()` snapshots the drawing, wide and narrow.

```sh
pnpm -C harness/clients/ink test
```

Nothing in the suite calls a model or opens a non-local socket. The smoke test stands a
loopback listener up in-process and replays a recorded fixture down it.
