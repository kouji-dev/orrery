//! The async peer: one connection, many calls, each cancellable on its own.
//!
//! # Why the ADE's client could not carry this
//!
//! `ade/src-tauri/src/lsp/client.rs` is a synchronous `recv_timeout`
//! request/response over two OS threads per server. Three things it cannot do,
//! and all three are load-bearing here:
//!
//! - it cannot **stream** a delta;
//! - it cannot hold a long-lived **guest→broker callback** open, because the
//!   direction is baked in;
//! - it has no **per-call cancellation**: `turn.cancel` must reach one in-flight
//!   tool call, not the connection.
//!
//! What did carry over is the design: a pending map keyed by id, a stderr ring,
//! and an error shape that distinguishes "it said no" from "it is gone".

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::cancel;
use crate::error::RpcError;
use crate::framing::{Framing, read_frame, write_frame};
use crate::message::{Envelope, Id, Incoming};

/// What answers the frames that arrive unasked for.
///
/// Both directions of the connection are the same code, which is what makes a
/// guest→broker callback ordinary rather than a special case: the host is a
/// peer with a handler, and so is the guest.
#[async_trait]
pub trait Handler: Send + Sync {
    /// Answer a request from the far side.
    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let _ = params;
        Err(RpcError::method_not_found(method))
    }

    /// Take note of a notification. There is no answer, by definition.
    async fn notify(&self, method: &str, params: Value) {
        let _ = (method, params);
    }
}

/// A handler that answers nothing: the right thing for a peer that only calls.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoHandler;

#[async_trait]
impl Handler for NoHandler {}

type Pending = Arc<Mutex<HashMap<Id, oneshot::Sender<Result<Value, RpcError>>>>>;

/// One end of a JSON-RPC connection.
///
/// Cheap to clone: every clone talks over the same connection, which is what
/// lets a tool call and a cancellation be issued from different tasks.
#[derive(Clone)]
pub struct Peer {
    outbound: mpsc::UnboundedSender<Envelope>,
    pending: Pending,
    next_id: Arc<AtomicU64>,
    /// Set when the read loop ends.
    ///
    /// Needed because the outbound channel outlives the connection: the peer
    /// handle itself holds a sender, so `send` keeps succeeding after the child
    /// is gone and a new call would wait for a reply that cannot come. Without
    /// this flag the *second* call to a crashed extension waits out its whole
    /// wall-clock budget instead of failing at once.
    closed: Arc<AtomicBool>,
}

impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Peer")
            .field("pending", &self.pending.lock().len())
            .finish_non_exhaustive()
    }
}

impl Peer {
    /// Start a peer over a reader and a writer, with a handler for whatever the
    /// far side asks unprompted.
    ///
    /// Two tasks: one pumps outbound frames, one reads inbound ones and either
    /// completes a pending call or spawns the handler. The handler is spawned
    /// rather than awaited inline, so a slow callback cannot stall the replies
    /// to the other calls on the same connection.
    pub fn spawn<R, W>(reader: R, writer: W, framing: Framing, handler: Arc<dyn Handler>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (outbound, mut rx) = mpsc::unbounded_channel::<Envelope>();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        let closed = Arc::new(AtomicBool::new(false));
        let peer = Self {
            outbound: outbound.clone(),
            pending: pending.clone(),
            next_id: Arc::new(AtomicU64::new(1)),
            closed: closed.clone(),
        };

        // Outbound.
        let mut writer = writer;
        tokio::spawn(async move {
            while let Some(envelope) = rx.recv().await {
                let body = match serde_json::to_vec(&envelope) {
                    Ok(body) => body,
                    Err(e) => {
                        tracing::error!(target: "orrery.jsonrpc", error = %e, "unserialisable frame");
                        continue;
                    }
                };
                if let Err(e) = write_frame(&mut writer, framing, &body).await {
                    tracing::debug!(target: "orrery.jsonrpc", error = %e, "the connection went away");
                    return;
                }
            }
        });

        // Inbound.
        let replies = peer.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            loop {
                match read_frame(&mut reader, framing).await {
                    Ok(Some(body)) => {
                        let envelope: Envelope = match serde_json::from_slice(&body) {
                            Ok(envelope) => envelope,
                            Err(e) => {
                                // One bad frame is not a dead connection.
                                tracing::warn!(
                                    target: "orrery.jsonrpc",
                                    error = %e,
                                    "unparseable frame, ignored"
                                );
                                continue;
                            }
                        };
                        dispatch(envelope, &pending, &replies, &handler);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::debug!(target: "orrery.jsonrpc", error = %e, "read failed");
                        break;
                    }
                }
            }
            // The connection is gone. Every pending call is told, once, rather
            // than waiting on a reply that cannot arrive — and every *future*
            // call is told immediately rather than waiting out its budget.
            closed.store(true, Ordering::SeqCst);
            let waiting: Vec<_> = pending.lock().drain().map(|(_, tx)| tx).collect();
            for tx in waiting {
                let _ = tx.send(Err(RpcError::Closed));
            }
        });

        peer
    }

    /// Call the far side and wait for its answer.
    ///
    /// `cancel` belongs to **this call**. When it fires, a `$/cancel`
    /// notification naming this request goes out and the call returns
    /// [`RpcError::Cancelled`] — the connection, and every other call on it,
    /// carries on.
    ///
    /// # Errors
    ///
    /// [`RpcError`]: the far side's refusal, a cancellation, or a closed
    /// connection.
    pub async fn call(
        &self,
        method: &str,
        params: Value,
        cancel_token: &CancellationToken,
    ) -> Result<Value, RpcError> {
        if !self.is_connected() {
            return Err(RpcError::Closed);
        }

        let id = Id::Num(self.next_id.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id.clone(), tx);

        // The read loop may have ended between the check above and the insert,
        // in which case it has already drained the map and will not see ours.
        if !self.is_connected() {
            self.pending.lock().remove(&id);
            return Err(RpcError::Closed);
        }

        if self
            .outbound
            .send(Envelope::request(id.clone(), method, params))
            .is_err()
        {
            self.pending.lock().remove(&id);
            return Err(RpcError::Closed);
        }

        tokio::select! {
            answer = rx => answer.unwrap_or(Err(RpcError::Closed)),
            () = cancel_token.cancelled() => {
                self.pending.lock().remove(&id);
                self.notify(cancel::METHOD, cancel::params(&id));
                Err(RpcError::Cancelled)
            }
        }
    }

    /// Tell the far side something. There is no answer and no waiting.
    pub fn notify(&self, method: &str, params: Value) {
        let _ = self.outbound.send(Envelope::notification(method, params));
    }

    /// How many calls are waiting for an answer.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.pending.lock().len()
    }

    /// Whether the connection is still there.
    ///
    /// False as soon as the read loop ends, which is the moment a child dies —
    /// not whenever the last handle is dropped.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        !self.closed.load(Ordering::SeqCst) && !self.outbound.is_closed()
    }
}

/// Route one inbound frame.
fn dispatch(envelope: Envelope, pending: &Pending, replies: &Peer, handler: &Arc<dyn Handler>) {
    match envelope.classify() {
        Incoming::Result { id, result } => complete(pending, &id, Ok(result)),
        Incoming::Failure { id, error } => complete(pending, &id, Err(error.into())),
        Incoming::Request { id, method, params } => {
            let handler = handler.clone();
            let replies = replies.clone();
            tokio::spawn(async move {
                let answer = handler.request(&method, params).await;
                let frame = match answer {
                    Ok(result) => Envelope::result(id, result),
                    Err(e) => Envelope::failure(id, e.to_object()),
                };
                let _ = replies.outbound.send(frame);
            });
        }
        Incoming::Notification { method, params } => {
            let handler = handler.clone();
            tokio::spawn(async move { handler.notify(&method, params).await });
        }
        Incoming::Junk { why } => {
            tracing::warn!(target: "orrery.jsonrpc", why, "frame ignored");
        }
    }
}

/// Hand an answer to whoever is waiting for it.
fn complete(pending: &Pending, id: &Id, answer: Result<Value, RpcError>) {
    match pending.lock().remove(id) {
        Some(tx) => {
            let _ = tx.send(answer);
        }
        // An answer to a call that was cancelled, or that nobody made. Not an
        // error: the cancel raced the reply, which is normal.
        None => tracing::debug!(target: "orrery.jsonrpc", %id, "answer for an unknown call"),
    }
}

/// Read a child's stderr into a ring, so a chatty guest cannot grow the host.
pub async fn pump_stderr<R>(reader: R, ring: Arc<crate::framing::StderrRing>)
where
    R: AsyncRead + Unpin,
{
    crate::framing::pump_stderr(reader, ring).await;
}

/// Read a line at a time from anything, for a transport that needs it.
pub async fn read_line<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<String>> {
    let mut line = String::new();
    match reader.read_line(&mut line).await? {
        0 => Ok(None),
        _ => Ok(Some(line)),
    }
}
