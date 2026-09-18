//! The only path to the outside world: read, write, spawn, net and creds behind a capability token.
//!
//! Implementation plan: `harness/docs/plans/07-policy-broker-audit.md`

#![deny(missing_docs)]
// This crate genuinely needs `unsafe` (see harness/docs/plans/00b-scaffold-workspace.md,
// open question 1): every `unsafe` block carries a `// SAFETY:` comment.
#![deny(unsafe_op_in_unsafe_fn)]
