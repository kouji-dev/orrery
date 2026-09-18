//! The facade: builds a Kernel from resolved config, owns the tokio runtime, links the first-party set behind features.
//!
//! Implementation plan: `harness/docs/plans/05-kernel-loop.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]
