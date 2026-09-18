//! The default session backend: SQLite in WAL mode, one transaction per turn append, a writer actor per session.
//!
//! Implementation plan: `harness/docs/plans/02-session-store.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]
