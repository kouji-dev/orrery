//! The far half: what a guest implements, and the router that hands it work.
//!
//! There is no separate "server" type, because there is no separate server.
//! Both ends of an extension connection are a [`Peer`](crate::Peer) with a
//! [`Handler`](crate::Handler): the host calls `tool/call` on the guest, and
//! the guest calls `broker/read` back on the host, over the same connection, at
//! the same time. What this module adds is the routing table most handlers
//! want, and the cancellation bookkeeping every guest needs.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::cancel;
use crate::client::Handler;
use crate::error::RpcError;
use crate::message::Id;

/// One method's implementation.
#[async_trait]
pub trait Method: Send + Sync {
    /// Do the thing.
    ///
    /// `cancel` is this call's own token: it fires when the caller sends a
    /// `$/cancel` naming this request, and not when anything else on the
    /// connection is cancelled.
    async fn call(&self, params: Value, cancel: CancellationToken) -> Result<Value, RpcError>;
}

/// A handler that dispatches by method name and honours `$/cancel`.
///
/// The cancellation half is why this exists rather than a bare `match` in every
/// guest: a request's token has to be *registered* before the handler runs and
/// dropped after, or a `$/cancel` that arrives while the call is starting is
/// silently lost.
#[derive(Default)]
pub struct Router {
    methods: HashMap<String, Arc<dyn Method>>,
    live: Arc<Mutex<HashMap<Id, CancellationToken>>>,
}

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Router")
            .field("methods", &self.methods.len())
            .field("in_flight", &self.live.lock().len())
            .finish()
    }
}

impl Router {
    /// An empty router.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a method.
    #[must_use]
    pub fn on(mut self, method: impl Into<String>, implementation: Arc<dyn Method>) -> Self {
        self.methods.insert(method.into(), implementation);
        self
    }

    /// The token for one request, registered so a later `$/cancel` can find it.
    #[must_use]
    pub fn register(&self, id: Id) -> CancellationToken {
        let token = CancellationToken::new();
        self.live.lock().insert(id, token.clone());
        token
    }

    /// Forget a request that has settled.
    pub fn settle(&self, id: &Id) {
        self.live.lock().remove(id);
    }

    /// Stop one in-flight request. Returns whether there was one.
    pub fn cancel(&self, id: &Id) -> bool {
        match self.live.lock().remove(id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// How many requests this side is carrying.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.live.lock().len()
    }
}

#[async_trait]
impl Handler for Router {
    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let Some(implementation) = self.methods.get(method) else {
            return Err(RpcError::method_not_found(method));
        };
        // A request whose id the transport did not hand us cannot be cancelled
        // by id; it still gets a token, so the shape of every call is the same.
        implementation.call(params, CancellationToken::new()).await
    }

    async fn notify(&self, method: &str, params: Value) {
        if let Some(id) = cancel::requested(method, &params) {
            let found = self.cancel(&id);
            tracing::debug!(target: "orrery.jsonrpc", %id, found, "cancellation received");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Router;
    use crate::message::Id;

    #[test]
    fn a_cancellation_finds_its_request_and_only_its_request() {
        let router = Router::new();
        let one = router.register(Id::Num(1));
        let two = router.register(Id::Num(2));

        assert!(router.cancel(&Id::Num(2)));
        assert!(two.is_cancelled());
        assert!(!one.is_cancelled(), "call 1 is untouched");
        assert_eq!(router.in_flight(), 1);

        assert!(
            !router.cancel(&Id::Num(2)),
            "twice is not twice as cancelled"
        );
    }
}
