//! The MCP client and server: an MCP server is an extension id like any other.
//!
//! # No second namespacing scheme
//!
//! A server registers as extension id `mcp.<server>`, so its tools are ordinary
//! [`ToolRef`](orrery_proto::ToolRef)s — `mcp.jira` + `create_issue` — and
//! inherit namespacing, layer precedence, ambiguity resolution, the visible
//! set, the policy check and the audit **unchanged**. There is no MCP-shaped
//! table anywhere in this crate; [`register`] puts tools into
//! `orrery-tools`' registry and stops.
//!
//! # Three decisions, straight from §4.11
//!
//! 1. **Namespaced like everything else** ([`register`]). A server whose tool
//!    collides with an extension's is a resolution event, not a failure.
//! 2. **Behind the same policy.** An MCP server is remote code with network
//!    access: it gets a grant, its calls are audited, and a managed layer can
//!    pin the allowlist. Nothing here can be called except through
//!    [`Registry::dispatch`](orrery_tools::Registry::dispatch).
//! 3. **The connection lifecycle is the core's** ([`health`]). Discovered at
//!    session start, always. Connected when first needed. A dead one degrades
//!    its tools rather than stalling turns.
//!
//! And the rule that keeps an approved manifest meaningful:
//! [`register::reconcile`] re-resolves a `list_changed` against policy and
//! either admits the new tool **and records it** or refuses it. It is never
//! silently available.
//!
//! Implementation plan: `harness/docs/plans/13-skills-mcp.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod client;
pub mod error;
pub mod expose;
pub mod health;
pub mod register;
pub mod transport;

pub use client::{InitializeResult, McpClient, McpTool, PROTOCOL_VERSION, SUPPORTED, ToolResult};
pub use error::McpError;
pub use expose::{McpServerHandle, serve};
pub use health::{Health, McpHost, ServerSpec, Servers};
pub use register::{
    Admission, ListChangedWatch, Reconciliation, ext_id, reconcile, register, tool_ref,
};
pub use transport::{HttpSpec, HttpTransport, StdioSpec, TransportSpec};
