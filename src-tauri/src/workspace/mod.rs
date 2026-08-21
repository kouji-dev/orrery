//! Workspace state: what the user had open — tabs, pane trees, per-agent
//! selections, scroll/view positions. ONE JSON document persisted as a single
//! row in a key/value `workspace` table in the existing sqlite DB, exactly like
//! settings. The backend is a pure passthrough (`serde_json::Value`): the
//! FRONTEND owns the document schema, so new fields never need Rust changes,
//! and a malformed row degrades to "fresh workspace", never a startup failure.

pub mod commands;
pub mod service;

pub use service::WorkspaceService;
