//! The ExtensionHost trait, the instance table keyed by generation id, the load/degrade/unload machine and the native runtime.
//!
//! # What this crate makes true
//!
//! - **Failure after load is as defined as failure during it.** A collection
//!   member degrades, a singleton falls back, and the ledger names both.
//! - **A stale reference is a value.** Generation ids make an unloaded
//!   extension unresolvable, and dispatch answers
//!   [`Outcome::Unloaded`](orrery_proto::Outcome) rather than failing a turn.
//! - **`native` is a runtime, not a back door.** A compiled-in bundle takes the
//!   same manifest, the same policy check and the same ledger as a child
//!   process (translation #14).
//!
//! Implementation plan: `harness/docs/plans/06-extension-host.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod host;
pub mod native;
pub mod state;
pub mod table;
pub mod unload;

pub use host::{ExtensionHost, budget_of, ceiling_of};
pub use native::{NativeHost, NativeRegistry};
pub use state::{CallGuard, InFlight, StateCell};
pub use table::{ExtensionInstance, ExtensionTable};
pub use unload::DEFAULT_GRACE;
