//! The HTTP side, behind a trait, so the tests never open a socket.
//!
//! # Why this is injectable and Anthropic's is not
//!
//! The Anthropic provider talks to exactly one origin and its tests reach it
//! through a loopback server. This crate exists for *whatever you are running*
//! — ollama on a laptop, vllm on a box you own — and the machine running the
//! suite has none of them. A recorded byte stream replayed through a trait is
//! the only way the parser, the mapper and the cancellation path get covered at
//! all, so the seam is the design rather than a testing convenience.
//!
//! [`RecordedTransport`] is not test-only scaffolding either: it is what
//! `orrery ext test` and the eval runner replay a captured session with.

use futures_core::stream::BoxStream;
use orrery_provider::ProviderError;
use tokio_util::sync::CancellationToken;

/// One request this crate makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    /// The absolute URL.
    pub url: String,
    /// Headers, in order. Never contains a credential unless one was granted.
    pub headers: Vec<(String, String)>,
    /// The JSON body.
    pub body: Vec<u8>,
}

/// What a transport reports, in order.
///
/// [`HttpEvent::Head`] arrives exactly once and first. A transport that yields
/// a body chunk without it is treated as a `200`, because that is what a
/// recorded stream with no status line means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpEvent {
    /// The status line and headers.
    Head {
        /// The status code.
        status: u16,
        /// The response headers, in order.
        headers: Vec<(String, String)>,
    },
    /// Some bytes of the body. Frame boundaries mean nothing.
    Chunk(Vec<u8>),
}

/// How this provider reaches a server.
///
/// **Dropping the returned stream must abort the request.** That is what makes
/// cancellation stop costing money rather than stop showing it — the same rule
/// [`Provider::stream`](orrery_provider::Provider::stream) states.
pub trait ChatTransport: Send + Sync + 'static {
    /// Make one request.
    fn send(
        &self,
        req: HttpRequest,
        cancel: CancellationToken,
    ) -> BoxStream<'static, Result<HttpEvent, ProviderError>>;
}

/// A transport that replays committed bytes and never opens a socket.
///
/// The bytes are handed out in slices of [`chunk_size`](Self::with_chunk_size),
/// so a test gets frames split across chunk boundaries — which is the bug an
/// SSE parser actually has — without writing a mock stream per case.
#[derive(Clone, Debug)]
pub struct RecordedTransport {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    chunk_size: usize,
}

impl RecordedTransport {
    /// Replay this body as a `200`.
    #[must_use]
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type".to_owned(), "text/event-stream".to_owned())],
            body: body.into(),
            chunk_size: 7,
        }
    }

    /// Replay a failure: a status, its headers, and whatever the server said.
    #[must_use]
    pub fn failing(status: u16, headers: Vec<(String, String)>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers,
            body: body.into(),
            chunk_size: usize::MAX,
        }
    }

    /// Hand the body out in slices of this size. Small is the interesting case.
    #[must_use]
    pub const fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = if chunk_size == 0 { 1 } else { chunk_size };
        self
    }
}

impl ChatTransport for RecordedTransport {
    fn send(
        &self,
        _req: HttpRequest,
        cancel: CancellationToken,
    ) -> BoxStream<'static, Result<HttpEvent, ProviderError>> {
        let head = HttpEvent::Head {
            status: self.status,
            headers: self.headers.clone(),
        };
        let chunks: Vec<Vec<u8>> = self
            .body
            .chunks(self.chunk_size.min(self.body.len().max(1)))
            .map(<[u8]>::to_vec)
            .collect();
        Box::pin(futures_util::stream::unfold(
            (Some(head), chunks.into_iter(), cancel),
            |(head, mut chunks, cancel)| async move {
                // Checked before every yield: a cancelled replay must stop
                // producing, or a cancellation test passes against a transport
                // that ignores cancellation.
                if cancel.is_cancelled() {
                    return None;
                }
                if let Some(head) = head {
                    return Some((Ok(head), (None, chunks, cancel)));
                }
                let next = chunks.next()?;
                Some((Ok(HttpEvent::Chunk(next)), (None, chunks, cancel)))
            },
        ))
    }
}

/// The real thing: `reqwest`, with the workspace's TLS setup.
#[cfg(feature = "http")]
mod real {
    use futures_core::stream::BoxStream;
    use futures_util::StreamExt;
    use orrery_provider::ProviderError;
    use tokio_util::sync::CancellationToken;

    use super::{ChatTransport, HttpEvent, HttpRequest};

    /// `reqwest` behind the [`ChatTransport`] seam.
    #[derive(Clone, Debug, Default)]
    pub struct ReqwestTransport {
        client: reqwest::Client,
    }

    impl ReqwestTransport {
        /// A transport over a fresh client.
        #[must_use]
        pub fn new() -> Self {
            install_crypto_provider();
            Self::default()
        }

        /// A transport over a client the embedder configured — a proxy, a
        /// timeout, a root store of its own.
        #[must_use]
        pub fn with_client(client: reqwest::Client) -> Self {
            install_crypto_provider();
            Self { client }
        }
    }

    /// The workspace pins rustls with `ring` and no provider from reqwest, so
    /// somebody has to install one. Doing it here, once, keeps the ADE and the
    /// harness on a single crypto stack — and `install_default` failing means
    /// another crate already did it, which is the outcome we wanted anyway.
    fn install_crypto_provider() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });
    }

    impl ChatTransport for ReqwestTransport {
        fn send(
            &self,
            req: HttpRequest,
            cancel: CancellationToken,
        ) -> BoxStream<'static, Result<HttpEvent, ProviderError>> {
            let client = self.client.clone();
            let (tx, rx) = tokio::sync::mpsc::channel(32);
            // A *child* token, so that dropping the stream aborts the request
            // even when the caller's own token is still live. The guard below
            // is what turns "the consumer went away" into "the socket closed".
            let child = cancel.child_token();
            let guard = child.clone().drop_guard();
            tokio::spawn(async move { pump(client, req, child, tx).await });
            Box::pin(futures_util::stream::unfold(
                (rx, guard),
                |(mut rx, guard)| async move { rx.recv().await.map(|i| (i, (rx, guard))) },
            ))
        }
    }

    /// Runs one request to completion, cancellation or failure. Every await is
    /// inside a `select!` against the token: on cancel this returns, the
    /// `Response` it owns is dropped, and `reqwest` closes the connection.
    async fn pump(
        client: reqwest::Client,
        req: HttpRequest,
        cancel: CancellationToken,
        tx: tokio::sync::mpsc::Sender<Result<HttpEvent, ProviderError>>,
    ) {
        let mut builder = client.post(&req.url).body(req.body);
        for (k, v) in req.headers {
            builder = builder.header(k, v);
        }
        let response = tokio::select! {
            r = builder.send() => r,
            () = cancel.cancelled() => return,
        };
        let response = match response {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(Err(crate::from_reqwest(&e))).await;
                return;
            }
        };
        let head = HttpEvent::Head {
            status: response.status().as_u16(),
            headers: response
                .headers()
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_owned(),
                        v.to_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect(),
        };
        if tx.send(Ok(head)).await.is_err() {
            return;
        }
        let mut chunks = response.bytes_stream();
        loop {
            let chunk = tokio::select! {
                c = chunks.next() => c,
                () = cancel.cancelled() => return,
            };
            let Some(chunk) = chunk else { return };
            let event = match chunk {
                Ok(b) => Ok(HttpEvent::Chunk(b.to_vec())),
                Err(e) => Err(crate::from_reqwest(&e)),
            };
            let fatal = event.is_err();
            if tx.send(event).await.is_err() || fatal {
                return;
            }
        }
    }
}

#[cfg(feature = "http")]
pub use real::ReqwestTransport;
