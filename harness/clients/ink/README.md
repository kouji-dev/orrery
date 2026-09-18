# `@orrery/client-ink`

The React/Ink TUI. Spawned by `orrery --ui ink` with `ORRERY_ENDPOINT` in the environment,
or run standalone against `orrery serve`.

It is an AG-UI subscriber like every other renderer, built on
[`@orrery/client`](../sdk-ts). It gets no privileged path into the kernel: it attaches over
the transport exactly as the in-binary ratatui client does.

**Status.** Scaffold only, and `private` until there is something to publish.
Implementation plan: [`harness/docs/plans/09c-client-ink.md`](../../docs/plans/09c-client-ink.md).

Tests are data first, pixels second: the [`../conformance`](../conformance) fixtures assert
`SurfaceStore` state, then `ink-testing-library`'s `lastFrame()` snapshots the drawing.
