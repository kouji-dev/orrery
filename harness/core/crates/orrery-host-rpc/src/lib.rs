//! Extension hosts that run a child process: node, python and plain process children, contained per platform.
//!
//! # What a child process buys, and what it does not
//!
//! It buys a real boundary for file descriptors and for the host's memory: a
//! guest cannot corrupt the harness's heap and cannot see its handles. It does
//! **not** buy an OS boundary — the child runs with this process's privileges
//! (translation #13). The SDK's loader hook is a convenience, not a guarantee;
//! wasm (plan 14) is the runtime with a real boundary.
//!
//! - [`contain`] — Job Object on Windows, process group on unix. The one module
//!   in the harness that writes `unsafe`.
//! - [`spawn`] — starting a guest and holding on to it.
//! - [`protocol`] — what the two sides say to each other.
//! - [`broker_bridge`] — the guest calling back into the broker.
//! - [`node`] — the host itself.
//!
//! Implementation plan: `harness/docs/plans/06-extension-host.md`

#![deny(missing_docs)]
#![deny(unsafe_code)]

pub mod broker_bridge;
#[allow(unsafe_code, reason = "there is no safe AssignProcessToJobObject")]
pub mod contain;
pub mod node;
pub mod protocol;
pub mod spawn;

pub use broker_bridge::BrokerBridge;
pub use contain::Containment;
pub use node::RpcHost;
pub use spawn::Guest;
