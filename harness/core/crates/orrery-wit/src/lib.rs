//! The WIT world as data plus the flat-arena surface encoding.
//!
//! One `.wit` definition is the single source of truth for every WASM-target
//! language **and** for what the JSON-RPC paths serialise. That is what keeps
//! this from becoming six half-maintained SDKs.
//!
//! This crate holds two things:
//!
//! - [`WORLD`], the `.wit` text itself, embedded so a test can assert the file
//!   on disk and the crate never drift.
//! - [`arena`], translation #9: a [`Surface`](orrery_proto::surface::Surface)
//!   crosses the boundary flat, because WIT has no recursive types.
//!
//! The host bindings themselves are generated inside `orrery-host-wasm`, where
//! the `wasmtime` dependency lives; this crate stays free of it so that the
//! arena can be linked by anything, including paths with no wasm in them.
//!
//! Implementation plan: `harness/docs/plans/14-wasm-wit.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod arena;

/// The `.wit` world, embedded from `harness/wit/orrery-extension.wit`.
///
/// Embedded rather than read at runtime so that a binary carrying this crate
/// carries the contract it was built against.
pub const WORLD: &str = include_str!("../../../../wit/orrery-extension.wit");

/// The package identifier the world is declared under.
///
/// Bumping the major here is what a breaking change to the arena shape costs,
/// which is the point of writing it down.
pub const PACKAGE: &str = "orrery:extension@1.0.0";
