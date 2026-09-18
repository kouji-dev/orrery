//! JSON-RPC 2.0 with pluggable framing, async correlation, bidirectional calls and cancellation.
//!
//! # What this is for
//!
//! One connection to a guest carries three things at once: the host calling a
//! tool, the guest calling back into the broker, and a cancellation aimed at
//! exactly one of those. Every decision here follows from that sentence.
//!
//! - [`framing`] — `Content-Length` (moved verbatim from the ADE) and
//!   newline-delimited, behind one enum.
//! - [`message`] — the wire frames, and what they mean once classified.
//! - [`client`] — the peer: a pending map, a handler for the other direction,
//!   and a cancel token per call.
//! - [`server`] — the routing table and cancellation bookkeeping a guest wants.
//! - [`cancel`] — `$/cancel`, which names one request.
//!
//! Implementation plan: `harness/docs/plans/06-extension-host.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod cancel;
pub mod client;
pub mod error;
pub mod framing;
pub mod message;
pub mod server;

pub use client::{Handler, NoHandler, Peer};
pub use error::RpcError;
pub use framing::{Framing, StderrRing};
pub use message::{Envelope, ErrorObject, Id, Incoming};
pub use server::{Method, Router};
