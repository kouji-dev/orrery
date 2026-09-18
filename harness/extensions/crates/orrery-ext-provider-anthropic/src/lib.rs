//! The Anthropic Messages provider: request builder, SSE parser, tool-use blocks, cache_control and usage.
//!
//! Hand-written, as §4.5 argues: `reqwest` with the workspace's rustls setup, a
//! `POST /v1/messages` body built out of `serde_json`, and a fifty-line SSE
//! splitter. No vendored SDK — the whole client is smaller than the translation
//! layer one would need.
//!
//! Three rules this crate is built around:
//!
//! - **It never sleeps and never retries.** A 429 becomes
//!   [`ProviderError::RateLimited`] and the kernel decides what that is worth.
//! - **It never sees a credential.** [`auth::ApiKeyAuth`] asks a named grant.
//! - **Dropping the stream aborts the request.** The HTTP response lives in a
//!   task that a cancellation token owns, so cancelling stops the meter rather
//!   than stopping the rendering.
//!
//! Implementation plan: `harness/docs/plans/03-provider-layer.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod auth;
pub mod map;
pub mod oauth;
pub mod request;
pub mod sse;

use std::sync::Arc;

use futures_core::stream::BoxStream;
use futures_util::StreamExt;
use orrery_provider::{
    Capabilities, HeuristicCounter, ModelEvent, ModelRequest, Provider, ProviderAuth,
    ProviderError, TokenCounter,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::auth::{ApiKeyAuth, CredStore};
use crate::map::EventMapper;
use crate::sse::SseParser;

/// The API version header this client speaks.
const API_VERSION: &str = "2023-06-01";
/// The grant this provider asks the broker for.
pub const GRANT: &str = "anthropic";

/// The manifest this extension ships, parsed by the same parser a third
/// party's is.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// Anthropic's Messages API.
#[derive(Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    auth: Arc<ApiKeyAuth>,
    capabilities: Capabilities,
}

impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicProvider")
            .field("base_url", &self.base_url)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl AnthropicProvider {
    /// A provider that takes its key from `store` under the `anthropic` grant.
    #[must_use]
    pub fn new(store: Arc<dyn CredStore>) -> Self {
        install_crypto_provider();
        Self {
            client: reqwest::Client::new(),
            base_url: "https://api.anthropic.com".to_owned(),
            auth: Arc::new(ApiKeyAuth::new(GRANT, store)),
            capabilities: Capabilities {
                tools: true,
                images: true,
                cache: true,
                max_context: 200_000,
                max_output: 64_000,
            },
        }
    }

    /// Point at another origin — a loopback server in a test, or a gateway.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_owned();
        self
    }

    /// Override the declared capabilities, for a model that differs from the
    /// family default.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
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

impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn stream(
        &self,
        req: ModelRequest,
        cancel: CancellationToken,
    ) -> BoxStream<'static, Result<ModelEvent, ProviderError>> {
        let body = match request::build_body(&req, &self.capabilities) {
            Ok(b) => b,
            Err(e) => return Box::pin(futures_util::stream::once(async move { Err(e) })),
        };
        let (tx, rx) = mpsc::channel(32);
        // A *child* token, so that dropping the stream cancels the request even
        // when the caller's own token is still live. The guard below is what
        // turns "the consumer went away" into "the socket closed".
        let child = cancel.child_token();
        let guard = child.clone().drop_guard();
        let client = self.client.clone();
        let url = format!("{}/v1/messages", self.base_url);

        // The key is fetched **inside** the task, not before it: reading a
        // credential is a broker call, so it is async, and doing it here would
        // make `stream` async and stop the kernel holding `Arc<dyn Provider>`.
        let auth = Arc::clone(&self.auth);
        tokio::spawn(async move {
            let key = match auth.key().await {
                Ok(k) => k,
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            };
            pump(client, url, key, body, child, tx).await;
        });

        Box::pin(futures_util::stream::unfold(
            (rx, guard),
            |(mut rx, guard)| async move { rx.recv().await.map(|item| (item, (rx, guard))) },
        ))
    }

    fn counter(&self) -> Arc<dyn TokenCounter> {
        Arc::new(HeuristicCounter::new())
    }

    fn auth(&self) -> Arc<dyn ProviderAuth> {
        Arc::clone(&self.auth) as Arc<dyn ProviderAuth>
    }
}

/// Runs one request to completion, cancellation or failure.
///
/// Every await is inside a `select!` against the token. That is the whole
/// cancellation story: on cancel this function returns, the `Response` it owns
/// is dropped, and `reqwest` closes the connection.
async fn pump(
    client: reqwest::Client,
    url: String,
    key: String,
    body: serde_json::Value,
    cancel: CancellationToken,
    tx: mpsc::Sender<Result<ModelEvent, ProviderError>>,
) {
    let send = client
        .post(&url)
        .header("x-api-key", key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(body.to_string())
        .send();

    let response = tokio::select! {
        r = send => r,
        () = cancel.cancelled() => return,
    };
    let response = match response {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Err(from_reqwest(&e))).await;
            return;
        }
    };

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let text = tokio::select! {
            t = response.text() => t.unwrap_or_default(),
            () = cancel.cancelled() => return,
        };
        let _ = tx
            .send(Err(classify(status, retry_after.as_deref(), &text)))
            .await;
        return;
    }

    let mut parser = SseParser::new();
    let mut mapper = EventMapper::new();
    let mut chunks = response.bytes_stream();

    loop {
        let chunk = tokio::select! {
            c = chunks.next() => c,
            () = cancel.cancelled() => return,
        };
        let Some(chunk) = chunk else { break };
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(Err(from_reqwest(&e))).await;
                return;
            }
        };
        let frames = match parser.push(&chunk) {
            Ok(f) => f,
            Err(e) => {
                let _ = tx.send(Err(e)).await;
                return;
            }
        };
        if !emit(&mut mapper, frames, &tx).await {
            return;
        }
    }

    // A stream that ended without a blank line after its last frame still has
    // that frame; losing it would turn "the turn ended" into "the turn hung".
    let tail = parser.finish();
    let _ = emit(&mut mapper, tail, &tx).await;
}

/// Maps and forwards frames. Returns false when the pump should stop — the
/// consumer went away, or a frame was an error, which ends the stream the way a
/// real failure does.
async fn emit(
    mapper: &mut EventMapper,
    frames: Vec<sse::SseEvent>,
    tx: &mpsc::Sender<Result<ModelEvent, ProviderError>>,
) -> bool {
    for frame in frames {
        match mapper.frame(&frame) {
            Ok(events) => {
                for ev in events {
                    if tx.send(Ok(ev)).await.is_err() {
                        return false;
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(Err(e)).await;
                return false;
            }
        }
    }
    true
}

/// A transport failure, classified.
fn from_reqwest(e: &reqwest::Error) -> ProviderError {
    if e.is_timeout() {
        ProviderError::Timeout { elapsed_ms: 0 }
    } else {
        // Connect, TLS, reset, truncated body: all of them are "try again",
        // and none of them says the request was wrong.
        ProviderError::Connection(e.to_string())
    }
}

/// An HTTP status and body onto the classification the kernel acts on.
///
/// `retry_after` is the raw header. The spec allows a date there as well as a
/// number of seconds; a date we cannot parse is not a reason to stop treating a
/// 429 as a 429.
#[must_use]
pub fn classify(status: u16, retry_after: Option<&str>, body: &str) -> ProviderError {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let field = |k: &str| -> String {
        parsed
            .as_ref()
            .and_then(|v| v["error"][k].as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let kind = field("type");
    let message = field("message");
    let detail = if message.is_empty() {
        format!("HTTP {status}")
    } else {
        message.clone()
    };

    match status {
        429 => ProviderError::RateLimited {
            retry_after_ms: retry_after
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(|s| s.saturating_mul(1_000)),
        },
        401 | 403 => ProviderError::Auth(detail),
        404 => ProviderError::NoSuchModel(detail),
        413 => too_long(&message).unwrap_or(ProviderError::ContextTooLong { tokens: 0, max: 0 }),
        400 | 422 => too_long(&message).unwrap_or(ProviderError::BadRequest(detail)),
        500..=599 => ProviderError::ServerError { status },
        _ if !kind.is_empty() => classify_wire_error(&kind, &message),
        _ => ProviderError::BadRequest(detail),
    }
}

/// An in-stream `error` event's Anthropic error type onto the classification.
///
/// The same vocabulary as an HTTP error body, because it *is* the same
/// vocabulary — Anthropic reports a mid-stream failure with the shape it would
/// have used for a rejected request.
#[must_use]
pub fn classify_wire_error(kind: &str, message: &str) -> ProviderError {
    let detail = if message.is_empty() {
        kind.to_owned()
    } else {
        message.to_owned()
    };
    match kind {
        // 529. Retryable, and the single most common reason a long session
        // stalls.
        "overloaded_error" => ProviderError::ServerError { status: 529 },
        "api_error" => ProviderError::ServerError { status: 500 },
        "rate_limit_error" => ProviderError::RateLimited {
            retry_after_ms: None,
        },
        "authentication_error" | "permission_error" => ProviderError::Auth(detail),
        "not_found_error" => ProviderError::NoSuchModel(detail),
        "request_too_large" => {
            too_long(message).unwrap_or(ProviderError::ContextTooLong { tokens: 0, max: 0 })
        }
        "invalid_request_error" => too_long(message).unwrap_or(ProviderError::BadRequest(detail)),
        _ => ProviderError::BadRequest(detail),
    }
}

/// Anthropic says "prompt is too long: 312000 tokens > 200000 maximum". Pulling
/// the two numbers out is worth the parsing: the kernel compacts to a target,
/// and a target it had to guess is a target it gets wrong.
fn too_long(message: &str) -> Option<ProviderError> {
    let lower = message.to_ascii_lowercase();
    if !(lower.contains("too long") || lower.contains("too large") || lower.contains("max_tokens"))
    {
        return None;
    }
    let numbers: Vec<u64> = message
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    Some(match numbers.as_slice() {
        [tokens, max, ..] => ProviderError::ContextTooLong {
            tokens: *tokens,
            max: *max,
        },
        _ => ProviderError::ContextTooLong { tokens: 0, max: 0 },
    })
}
