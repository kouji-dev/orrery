//! App settings: ONE serde-camelCase JSON document persisted as a single row in
//! a key/value `settings` table in the existing sqlite DB. The struct carries
//! `#[serde(default)]` on every field so documents written by NEWER versions
//! (unknown keys) or OLDER versions (missing keys) both load cleanly — settings
//! are never a reason the app fails to start.

pub mod commands;
pub mod model;
pub mod service;

// Re-exported for consumers of the type (the spawn-path settings reads, tests,
// and the T2/T3 frontend-facing surface). Production code only touches the
// document through commands/service today, so allow the not-yet-used re-export
// rather than churn the path when the first direct consumer lands.
#[allow(unused_imports)]
pub use model::Settings;
pub use service::SettingsService;
