//! The wasmtime extension host: one Store per instance, resource limits, epoch deadlines and zero WASI preopens.
//!
//! Implementation plan: `harness/docs/plans/14-wasm-wit.md`

#![deny(missing_docs)]
// This crate genuinely needs `unsafe` (see harness/docs/plans/00b-scaffold-workspace.md,
// open question 1): every `unsafe` block carries a `// SAFETY:` comment.
#![deny(unsafe_op_in_unsafe_fn)]
