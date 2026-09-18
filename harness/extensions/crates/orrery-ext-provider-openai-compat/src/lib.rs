//! An OpenAI-compatible chat-completions provider, for local models behind ollama or vllm.
//!
//! This is the crate that makes "run a local model" possible at all: ollama,
//! vllm, llama.cpp's server, LM Studio and every hosted gateway that speaks the
//! same nine keys are one `base_url` apart from each other.
//!
//! Three rules this crate is built around, the same three the Anthropic
//! provider states:
//!
//! - **It never sleeps and never retries.** A 429 becomes
//!   [`ProviderError::RateLimited`] and the kernel decides what that is worth.
//! - **It never sees a credential.** [`auth::OptionalKeyAuth`] asks a named
//!   grant, and a server that wants no key is
//!   [`AuthState::Anonymous`](orrery_provider::AuthState::Anonymous) rather
//!   than a login prompt nobody can satisfy.
//! - **Dropping the stream aborts the request.** The HTTP side lives behind
//!   [`ChatTransport`], and both implementations stop producing when the token
//!   fires.
//!
//! # Why the transport is a trait
//!
//! No machine running this suite has ollama on it. Every test here replays a
//! committed byte stream through [`RecordedTransport`], so the parser, the
//! mapper, the failure classification and the cancellation path are all covered
//! without a socket. See [`transport`].
//!
//! Implementation plan: `harness/docs/plans/03-provider-layer.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod auth;
pub mod map;
pub mod request;
pub mod sse;
pub mod transport;

use std::sync::Arc;

use futures_core::stream::BoxStream;
use futures_util::StreamExt;
use orrery_provider::{
    Capabilities, HeuristicCounter, ModelEvent, ModelRequest, Provider, ProviderAuth,
    ProviderError, TokenCounter,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::auth::{CredStore, OptionalKeyAuth};
use crate::map::EventMapper;
use crate::sse::SseParser;
use crate::transport::{ChatTransport, HttpEvent, HttpRequest};

/// The manifest this extension ships, parsed by the same parser a third party's
/// is.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// The grant this provider asks the broker for.
pub const GRANT: &str = "openai-compat";

/// What a locally-served model is assumed to do until a profile says otherwise.
///
/// Conservative on purpose: a 8k window and no images is what a small model on
/// a laptop actually has, and a context assembler that believed 128k would
/// build a request the server rejects. Override with
/// [`with_capabilities`](OpenAiCompatProvider::with_capabilities).
#[must_use]
pub const fn default_capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        images: false,
        // No compatible server exposes a prompt-cache breakpoint, so
        // `cache_breakpoint` is ignored. The trait documents that as a no-op.
        cache: false,
        max_context: 8_192,
        max_output: 4_096,
    }
}

/// An OpenAI-compatible chat-completions endpoint.
#[derive(Clone)]
pub struct OpenAiCompatProvider {
    id: String,
    base_url: String,
    model_prefix: Option<String>,
    transport: Arc<dyn ChatTransport>,
    auth: Arc<OptionalKeyAuth>,
    capabilities: Capabilities,
}

impl std::fmt::Debug for OpenAiCompatProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatProvider")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl OpenAiCompatProvider {
    /// A provider over a transport of the caller's choosing.
    ///
    /// `base_url` is the server's root — `http://localhost:11434/v1` for
    /// ollama, `http://localhost:8000/v1` for vllm. A trailing slash is
    /// trimmed; `/chat/completions` is appended.
    #[must_use]
    pub fn new(
        base_url: impl Into<String>,
        transport: Arc<dyn ChatTransport>,
        store: Arc<dyn CredStore>,
    ) -> Self {
        Self {
            id: "openai-compat".to_owned(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            model_prefix: None,
            transport,
            auth: Arc::new(OptionalKeyAuth::new(GRANT, store)),
            capabilities: default_capabilities(),
        }
    }

    /// A provider over the real HTTP transport.
    #[cfg(feature = "http")]
    #[must_use]
    pub fn over_http(base_url: impl Into<String>, store: Arc<dyn CredStore>) -> Self {
        Self::new(
            base_url,
            Arc::new(transport::ReqwestTransport::new()),
            store,
        )
    }

    /// Report a different provider id — `ollama`, `vllm`, whatever the profile
    /// calls this endpoint. The router selects on it, so one build can serve
    /// two local servers at once.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Declare what this endpoint's model can do.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Only serve models whose id starts with this.
    ///
    /// A router with two local endpoints has to be able to say which one owns
    /// `qwen2.5-coder`, and the alternative is a round trip to `/v1/models`
    /// that a server behind a cold start will not answer quickly.
    #[must_use]
    pub fn serving(mut self, prefix: impl Into<String>) -> Self {
        self.model_prefix = Some(prefix.into());
        self
    }

    /// Where a completion is posted.
    #[must_use]
    pub fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn http_request(&self, body: &serde_json::Value) -> HttpRequest {
        let mut headers = vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            ("accept".to_owned(), "text/event-stream".to_owned()),
        ];
        // A local server wants no key, and sending an empty bearer is how you
        // get a 401 from one that would otherwise have answered.
        if let Some(key) = self.auth.key_if_present() {
            headers.push(("authorization".to_owned(), format!("Bearer {key}")));
        }
        HttpRequest {
            url: self.endpoint(),
            headers,
            body: body.to_string().into_bytes(),
        }
    }
}

impl Provider for OpenAiCompatProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn stream(
        &self,
        req: ModelRequest,
        cancel: CancellationToken,
    ) -> BoxStream<'static, Result<ModelEvent, ProviderError>> {
        if let Some(prefix) = &self.model_prefix
            && !req.model.starts_with(prefix.as_str())
        {
            let model = req.model.clone();
            return once_err(ProviderError::NoSuchModel(model));
        }
        let body = match request::build_body(&req, &self.capabilities) {
            Ok(b) => b,
            Err(e) => return once_err(e),
        };

        let (tx, rx) = mpsc::channel(32);
        // A *child* token, so that dropping the stream cancels the request even
        // when the caller's own token is still live.
        let child = cancel.child_token();
        let guard = child.clone().drop_guard();
        let transport = Arc::clone(&self.transport);
        let http = self.http_request(&body);

        tokio::spawn(async move {
            pump(transport, http, child, tx).await;
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

fn once_err(e: ProviderError) -> BoxStream<'static, Result<ModelEvent, ProviderError>> {
    Box::pin(futures_util::stream::once(async move { Err(e) }))
}

/// Reads one response to completion, cancellation or failure.
async fn pump(
    transport: Arc<dyn ChatTransport>,
    req: HttpRequest,
    cancel: CancellationToken,
    tx: mpsc::Sender<Result<ModelEvent, ProviderError>>,
) {
    let mut events = transport.send(req, cancel.clone());
    let mut parser = SseParser::new();
    let mut mapper = EventMapper::new();
    // `None` until the head arrives. A transport that yields a body chunk
    // without one is a recorded stream, which is a 200 by construction.
    let mut status: Option<u16> = None;
    let mut error_body: Vec<u8> = Vec::new();
    let mut retry_after: Option<String> = None;

    loop {
        let next = tokio::select! {
            e = events.next() => e,
            () = cancel.cancelled() => return,
        };
        let Some(next) = next else { break };
        let event = match next {
            Ok(e) => e,
            Err(e) => {
                let _ = tx.send(Err(e)).await;
                return;
            }
        };
        match event {
            HttpEvent::Head {
                status: code,
                headers,
            } => {
                status = Some(code);
                retry_after = headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
                    .map(|(_, v)| v.clone());
            }
            HttpEvent::Chunk(bytes) => {
                // A failing response is not SSE: it is one JSON object, and it
                // has to be read whole before it can be classified.
                if status.is_some_and(|s| !(200..300).contains(&s)) {
                    error_body.extend_from_slice(&bytes);
                    continue;
                }
                let frames = match parser.push(&bytes) {
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
        }
    }

    if let Some(code) = status.filter(|s| !(200..300).contains(s)) {
        let text = String::from_utf8_lossy(&error_body).into_owned();
        let _ = tx
            .send(Err(classify(code, retry_after.as_deref(), &text)))
            .await;
        return;
    }

    // A stream that ended without a blank line after its last frame still has
    // that frame; losing it would turn "the turn ended" into "the turn hung".
    let tail = parser.finish();
    if !emit(&mut mapper, tail, &tx).await {
        return;
    }
    // And a server that closed without `[DONE]` still left tool calls open.
    for ev in mapper.close() {
        if tx.send(Ok(ev)).await.is_err() {
            return;
        }
    }
}

/// Maps and forwards frames. Returns false when the pump should stop.
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

/// An HTTP status onto the classification the kernel acts on.
///
/// The codes are stable across providers on purpose: an `anthropic` 429 and an
/// `openai-compat` 429 both read `rate_limited`, so a dashboard needs no
/// per-vendor table.
#[must_use]
pub fn classify(status: u16, retry_after: Option<&str>, body: &str) -> ProviderError {
    let message = message_of(body).unwrap_or_else(|| body.trim().to_owned());
    match status {
        401 | 403 => ProviderError::Auth(message),
        404 => ProviderError::NoSuchModel(message),
        408 => ProviderError::Timeout { elapsed_ms: 0 },
        429 => ProviderError::RateLimited {
            // `Retry-After` is seconds on this wire. A malformed one is not a
            // reason to fail the turn — the kernel has its own backoff.
            retry_after_ms: retry_after
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(|s| s.saturating_mul(1_000)),
        },
        // A context overflow is a 400 on this wire, with the numbers only in
        // prose. Reporting it as `BadRequest` would make the kernel fail the
        // turn instead of compacting, so the shape of the message is what
        // separates the two.
        400 if is_context_overflow(&message) => ProviderError::ContextTooLong {
            tokens: 0,
            max: 0,
        },
        400..=499 => ProviderError::BadRequest(message),
        500..=599 => ProviderError::ServerError { status },
        other => ProviderError::BadRequest(format!("unexpected status {other}: {message}")),
    }
}

/// An error object inside a chunk, classified the same way a status is.
#[must_use]
pub fn classify_wire_error(kind: &str, code: &str, message: &str) -> ProviderError {
    let text = if message.is_empty() {
        format!("{kind} {code}").trim().to_owned()
    } else {
        message.to_owned()
    };
    match (kind, code) {
        (_, "rate_limit_exceeded") | ("rate_limit_error", _) => ProviderError::RateLimited {
            retry_after_ms: None,
        },
        (_, "context_length_exceeded") => ProviderError::ContextTooLong { tokens: 0, max: 0 },
        ("invalid_request_error", _) if is_context_overflow(&text) => {
            ProviderError::ContextTooLong { tokens: 0, max: 0 }
        }
        ("authentication_error" | "permission_error", _) => ProviderError::Auth(text),
        ("server_error" | "api_error" | "internal_server_error", _) => {
            ProviderError::ServerError { status: 500 }
        }
        ("model_not_found" | "not_found_error", _) => ProviderError::NoSuchModel(text),
        _ => ProviderError::BadRequest(text),
    }
}

/// Whether a 400 is really "your context is too long".
///
/// Matched on words rather than a code because no two compatible servers agree
/// on the code, and the kernel's answer — compact and retry — is worth more
/// than the elegance of a table.
fn is_context_overflow(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    (m.contains("context") && (m.contains("length") || m.contains("window") || m.contains("size")))
        || m.contains("too many tokens")
        || m.contains("maximum context")
}

/// `{"error": {"message": "…"}}`, or `{"error": "…"}`, or neither.
fn message_of(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = v.get("error")?;
    error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .or_else(|| error.as_str())
        .map(ToOwned::to_owned)
}

/// A `reqwest` failure onto the classification.
#[cfg(feature = "http")]
#[must_use]
pub fn from_reqwest(e: &reqwest::Error) -> ProviderError {
    if e.is_timeout() {
        return ProviderError::Timeout { elapsed_ms: 0 };
    }
    ProviderError::Connection(e.to_string())
}
