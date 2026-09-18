/**
 * `@orrery/client` — an AG-UI session and a `SurfaceStore`. Pure data, no drawing.
 *
 * The same two types as `clients/sdk-rs`, against the same conformance fixtures.
 * The session half sits on stock `@ag-ui/client`, which is what makes "existing
 * AG-UI clients work unmodified" a thing this repo checks rather than claims.
 *
 * ## A client never writes state
 *
 * AG-UI's shared state is bidirectional by design; ours is not. `StateSnapshot`
 * and `StateDelta` flow outward only, and an edit a person makes arrives back as
 * an `intent` the kernel validates. Anyone porting a component that expects to
 * write state directly needs to know that.
 *
 * Implementation plan: `harness/docs/plans/08-protocol-transport.md`
 */

export * from "./events.js";
export * from "./store.js";
export * from "./session.js";
export * as conformance from "./conformance.js";
