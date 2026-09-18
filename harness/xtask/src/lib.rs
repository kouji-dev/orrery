//! Repo automation for the Orrery harness: `deps-check`, `publish-check`,
//! `typegen`, `wit-check` and `agui-drift`.
//!
//! All five are implemented. `wit-check` delegates to `orrery-wit`'s `.wit`
//! drift tests, which sit next to the Rust side they compare against.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod agui_drift;
pub mod deps_check;
pub mod publish_check;
pub mod typegen;
