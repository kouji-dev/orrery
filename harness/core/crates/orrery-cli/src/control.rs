//! The kernel's side of the control RPC.
//!
//! `session.attach`, `turn.submit`, `turn.cancel`, `consent.answer` and
//! `intent` arrive here whichever listener carried them — a pipe frame, a
//! `POST /control`, or a plain call in-process. One implementation, because
//! "no privileged client" is a claim about code paths.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 4.

use std::collections::HashMap;
use std::sync::Arc;

use orrery_harness::Harness;
use orrery_proto::{Request, TurnId};
use orrery_transport::listener::ControlHandler;
use orrery_transport::{Hub, TransportError};
use tokio_util::sync::CancellationToken;

use crate::session::{Publisher, Session};

/// One session's control surface.
pub struct KernelControl {
    harness: Arc<Harness>,
    publisher: Arc<Publisher>,
    hub: Hub,
    handle: tokio::runtime::Handle,
    running: std::sync::Mutex<HashMap<TurnId, CancellationToken>>,
}

impl std::fmt::Debug for KernelControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KernelControl").finish_non_exhaustive()
    }
}

impl KernelControl {
    /// A control surface over a built session.
    #[must_use]
    pub fn new(session: &Session) -> Arc<Self> {
        Arc::new(Self {
            harness: session.harness(),
            publisher: session.publisher(),
            hub: session.hub(),
            handle: session.handle(),
            running: std::sync::Mutex::new(HashMap::new()),
        })
    }

    /// Start a turn and return at once with the run id.
    ///
    /// **Never waits for the turn.** A listener calls this from a sync fn while
    /// the client that asked is still being written to; blocking here would
    /// stall the very connection that is meant to be watching.
    pub fn submit(&self, prompt: String) -> TurnId {
        let turn = TurnId::new();
        let cancel = CancellationToken::new();
        self.running
            .lock()
            .expect("running turns")
            .insert(turn, cancel.clone());
        let harness = self.harness.clone();
        let publisher = self.publisher.clone();
        self.handle.spawn(async move {
            let _ = Session::submit(harness, publisher, turn, prompt, cancel).await;
        });
        turn
    }
}

impl ControlHandler for KernelControl {
    fn control(&self, request: Request) -> Result<serde_json::Value, TransportError> {
        match request {
            Request::TurnSubmit { input, .. } => {
                let turn = self.submit(input.text);
                Ok(serde_json::json!({ "turn": turn.to_string() }))
            }
            Request::TurnCancel { turn, .. } => {
                if let Some(cancel) = self.running.lock().expect("running turns").get(&turn) {
                    cancel.cancel();
                }
                Ok(serde_json::Value::Null)
            }
            Request::SessionAttach { since, .. } => {
                // The byte-stream listeners replay at the handshake, where the
                // connection's writer is; this answers the in-process and HTTP
                // callers, for whom `attach` is a question rather than a
                // subscription.
                let frames = self.hub.attach(since.map(|s| s.0)).unwrap_or_default();
                Ok(serde_json::json!({
                    "session": self.harness.session().to_string(),
                    "last_seq": self.hub.last_seq(),
                    "replayed": frames.len(),
                }))
            }
            Request::ConsentAnswer { prompt, answer, .. } => {
                Ok(serde_json::json!({ "accepted": self.hub.answer(prompt, answer) }))
            }
            // `intent`, `command` and `query` are real vocabulary with no
            // implementation in this build. They are refused by name rather
            // than silently accepted, so a client learns it asked for
            // something that will not happen.
            other => Err(TransportError::Codec(format!(
                "`{}` is not implemented in this build — see harness/docs/plans/17-cli.md",
                tag(&other)
            ))),
        }
    }
}

fn tag(request: &Request) -> String {
    serde_json::to_value(request)
        .ok()
        .and_then(|v| {
            v.get("t")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "request".to_owned())
}
