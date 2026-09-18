//! The extension-facing API: manifest, instance, tool definitions, call context, and the mock broker extension authors test against.
//!
//! This is the crate a community extension depends on, and the reason it is
//! published: an author writes a tool, tests it against the mock broker, sees
//! the ledger a real session would show them, and never links the harness.
//!
//! # The field list is the design rule
//!
//! If a subsystem can be replaced, it is a field on [`Provides`]. If it is not a
//! field, it is ours and it is not replaceable. The loop is not a field.
//!
//! # What is here and what is not
//!
//! - [`manifest`] — the contract, in TOML and in memory.
//! - [`instance`] — generation ids and the state machine, the runtime half.
//! - [`tool`] — what a tool declares, and the trait a compiled-in one implements.
//! - [`ctx`] — what a call gets: a ceiling, a cancel token, the broker, `ctx.ui`.
//! - [`broker`] — the only door out of an extension. Implemented in plan 07.
//! - [`ledger`] — what loaded, degraded, failed or was skipped.
//! - [`testing`] — the harness `orrery ext test` runs.
//!
//! The `ExtensionHost` trait and the instance table are **not** here: they name
//! runtimes, spawn children and hold an `Arc<dyn>` per instance, none of which a
//! published extension should have to compile. They live in `orrery-host`.
//!
//! Implementation plan: `harness/docs/plans/06-extension-host.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod broker;
pub mod ctx;
pub mod error;
pub mod instance;
pub mod ledger;
pub mod manifest;
pub mod testing;
pub mod tool;

pub use broker::{
    BrokerError, BrokerFacade, BrokerResult, DeniesEverything, NetRequest, NetResponse, ReadChunk,
    ReadRequest, SpawnOutput, SpawnRequest, WriteRequest,
};
pub use ctx::{CallCtx, SurfaceEmit, SurfaceLog, SurfaceSink, ToolBudget};
pub use error::HostError;
pub use instance::{Generation, InstanceState};
pub use ledger::Ledger;
pub use manifest::{
    ApiVersion, ExtensionManifest, ManifestError, ProcessSpec, Provides, Requirement, RuntimeKind,
    SUPPORTED_API_MAJOR, SingletonSlot,
};
pub use tool::{NativeExtension, ToolDef};
