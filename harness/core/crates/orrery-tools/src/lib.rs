//! Tool namespacing, resolution, visibility and dispatch: the only path to a tool.
//!
//! Every tool from every source — builtin, extension, MCP, skill — is held here
//! under a namespaced id, resolved by a documented precedence, shown to the
//! model as a deliberate subset, and dispatched through exactly one function
//! that cannot skip the policy check.
//!
//! # The three things this crate makes true
//!
//! - **One namespace.** A tool is always `ext.name`, and an MCP server is just
//!   an extension called `mcp.<server>`. No second scheme, no special case.
//! - **Ambiguity is a value.** Two extensions claiming `search` both work; the
//!   closer layer takes the short name and the ledger explains why. Nothing
//!   exits 1 over a duplicate name.
//! - **One dispatch path.** The [`ToolHost`] is a private field of [`Registry`]
//!   and no public method returns it, so the policy check in step 4 of
//!   [`Registry::dispatch`] is unavoidable rather than merely customary.
//!
//! Implementation plan: `harness/docs/plans/04-tool-registry.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod budget;
pub mod dispatch;
pub mod error;
pub mod registry;
pub mod resolve;
pub mod visible;

pub use budget::ToolBudget;
pub use orrery_proto::ToolDescriptor;
pub use dispatch::{
    AllowAll, CallCtx, PolicyCheck, PolicyDecision, ToolHost, ToolInterceptor, UnavailableHost,
};
pub use error::ToolError;
pub use registry::{Entry, ExtState, LedgerEntry, Registry, ToolSpec};
pub use resolve::Resolution;
