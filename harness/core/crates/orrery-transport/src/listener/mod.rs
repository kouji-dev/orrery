//! The three listeners, and the one control surface they share.
//!
//! | Transport | Used by |
//! |---|---|
//! | [`inproc`] | `orrery` (ratatui), `orrery run`, single-shot |
//! | [`pipe`] | `orrery attach` on the same machine |
//! | [`http`] | Ink, any AG-UI client, the ADE later |
//!
//! TCP + TLS is phase 4, same frames, `tokio-rustls`.
//!
//! # What stays ours
//!
//! AG-UI's input path is a run invocation, not a session protocol.
//! `session.attach(since)`, `turn.cancel`, `intent` and `query` have no
//! counterpart in it, so they ride a control RPC — `POST /control` over HTTP,
//! a [`Request`](orrery_proto::Request) frame on the byte-stream transports,
//! and a plain method call in-process. Three spellings, one vocabulary.

pub mod http;
pub mod inproc;
pub mod pipe;

use orrery_proto::Request;

use crate::error::TransportError;

/// What a listener hands a control request to.
///
/// The kernel implements it. Nothing in this crate does: the transport's job is
/// to get a [`Request`] to somebody who can answer it, not to answer it.
pub trait ControlHandler: Send + Sync + 'static {
    /// Handle one control request.
    ///
    /// # Errors
    ///
    /// Whatever the kernel could not do.
    fn control(&self, request: Request) -> Result<serde_json::Value, TransportError>;
}

impl<F> ControlHandler for F
where
    F: Fn(Request) -> Result<serde_json::Value, TransportError> + Send + Sync + 'static,
{
    fn control(&self, request: Request) -> Result<serde_json::Value, TransportError> {
        self(request)
    }
}
