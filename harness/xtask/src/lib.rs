//! Repo automation for the Orrery harness: `deps-check`, `typegen`,
//! `wit-check` and `agui-drift`.
//!
//! `deps-check`, `typegen` and `agui-drift` are implemented; `wit-check` is a
//! stub that names its plan.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod agui_drift;
pub mod deps_check;
pub mod typegen;
