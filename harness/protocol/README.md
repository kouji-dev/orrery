# `@orrery/protocol`

**Generated. Do not hand-edit anything in this directory except this file.**

`protocol.schema.json` and `protocol.d.ts` are produced by:

```bash
cargo xtask typegen
```

which walks the `schemars` derives on every `orrery-proto` type, writes
`protocol.schema.json`, and runs `json-schema-to-typescript` over it to produce
`protocol.d.ts`. Both are committed, and CI runs `typegen` and fails on
`git diff --exit-code` — so an edit here is reverted by the next run, silently, and
a change to the wire format that was not made in Rust is a red build.

The JSON Schema is needed anyway — for `ToolDef` input validation, for
`ParamSchema<P>`, and for the promise that a third party can build against a
published spec — so TypeScript is a by-product rather than a second generator.
That is why this is not `ts-rs` or `specta`.

**Source of truth:** [`harness/core/crates/orrery-proto`](../core/crates/orrery-proto).
**Implementation plan:** [`harness/docs/plans/01-proto-shared-types.md`](../docs/plans/01-proto-shared-types.md).

**Status.** Generated. `protocol.schema.json` is emitted by `schemars` from a
root type that names `Request`, `Event`, `Surface`, `SurfacePatch`, `Grant`,
`Budget`, `Usage`, `Message` and `LoadOutcome`; `protocol.d.ts` is
`json-schema-to-typescript` over it, pinned to one version so the drift gate
does not flap. `cargo test -p xtask` runs the gate.
