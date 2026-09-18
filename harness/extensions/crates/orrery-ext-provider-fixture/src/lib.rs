//! A scripted provider that replays a recorded ModelEvent stream from a .jsonl file. Deterministic, no API key.
//!
//! This is the provider the rest of the harness tests against. It ignores the
//! request entirely and emits exactly what a file says, at exactly the pace the
//! file says — which is what makes a kernel test about compaction, or a client
//! test about rendering, a test about compaction or rendering rather than a
//! test about whichever sentence a model happened to produce that afternoon.
//!
//! ```text
//! orrery serve --provider fixture:harness/clients/conformance/streams/tool-call.jsonl
//! ```
//!
//! # The file format
//!
//! One JSON object per line. A line is either an event or a failure:
//!
//! - **An event.** The object is a [`ModelEvent`] as `orrery-provider`
//!   serialises it — `{"t":"text-delta","text":"hello"}`. The stream is the
//!   type, not a parallel schema that can drift from it.
//! - **A failure.** `{"error":{"code":"rate_limited","retry_after_ms":1200}}`.
//!   The stream yields that [`ProviderError`] and ends, because a real one
//!   would.
//!
//! Either kind may carry `"delay_ms"`, the pause *before* that line is emitted.
//! Blank lines and lines beginning with `#` are ignored, so a fixture can be
//! commented.
//!
//! Every line is parsed when the file is loaded, not when it is reached: a typo
//! in a fixture is a load error naming the line, never a surprise in the middle
//! of somebody else's test.
//!
//! Implementation plan: `harness/docs/plans/03-provider-layer.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_core::stream::BoxStream;
use orrery_provider::{
    AuthCtx, AuthMethod, AuthState, Capabilities, HeuristicCounter, ModelEvent, ModelRequest,
    Provider, ProviderAuth, ProviderError, TokenCounter,
};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

/// A fixture that could not be loaded.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum FixtureError {
    /// The `--provider` value was not of the form `fixture:<path>`.
    #[error("`{spec}` is not a fixture spec: expected `fixture:<path>`")]
    BadSpec {
        /// What was passed.
        spec: String,
    },
    /// The file would not open.
    #[error("cannot read fixture `{path}`: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// Why not.
        source: std::io::Error,
    },
    /// A line was not a `ModelEvent` and not an error line.
    #[error("{path}, line {line}: {message}")]
    BadLine {
        /// The file.
        path: PathBuf,
        /// One-based, so it matches what an editor shows.
        line: usize,
        /// The parser's complaint.
        message: String,
    },
}

/// How an error line names the failure it wants.
///
/// `code` is [`ProviderError::code`], so the fixture vocabulary and the audit
/// vocabulary are the same list.
#[derive(Debug, Clone, Deserialize)]
struct ErrorSpec {
    code: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    retry_after_ms: Option<u64>,
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    elapsed_ms: Option<u64>,
    #[serde(default)]
    tokens: Option<u64>,
    #[serde(default)]
    max: Option<u64>,
}

impl ErrorSpec {
    fn build(self) -> Result<ProviderError, String> {
        let msg = || self.message.clone().unwrap_or_else(|| "fixture".to_owned());
        Ok(match self.code.as_str() {
            "rate_limited" => ProviderError::RateLimited {
                retry_after_ms: self.retry_after_ms,
            },
            "server" => ProviderError::ServerError {
                status: self.status.unwrap_or(500),
            },
            "timeout" => ProviderError::Timeout {
                elapsed_ms: self.elapsed_ms.unwrap_or(30_000),
            },
            "connection" => ProviderError::Connection(msg()),
            "auth" => ProviderError::Auth(msg()),
            "bad_request" => ProviderError::BadRequest(msg()),
            "refusal" => ProviderError::Refusal(msg()),
            "context_too_long" => ProviderError::ContextTooLong {
                tokens: self.tokens.unwrap_or(0),
                max: self.max.unwrap_or(0),
            },
            "no_such_model" => ProviderError::NoSuchModel(msg()),
            other => return Err(format!("unknown error code `{other}`")),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Meta {
    #[serde(default)]
    delay_ms: Option<u64>,
    #[serde(default)]
    error: Option<ErrorSpec>,
}

/// One parsed line.
#[derive(Debug, Clone)]
struct Step {
    delay_ms: Option<u64>,
    item: Result<ModelEvent, ProviderError>,
}

/// Replays a recorded stream.
#[derive(Debug, Clone)]
pub struct FixtureProvider {
    path: PathBuf,
    steps: Arc<[Step]>,
    capabilities: Capabilities,
}

impl FixtureProvider {
    /// Parse a `--provider` value of the form `fixture:<path>` and load it.
    ///
    /// # Errors
    ///
    /// [`FixtureError::BadSpec`] when the scheme is missing or the path is
    /// empty; otherwise whatever [`FixtureProvider::load`] returns.
    pub fn from_spec(spec: &str) -> Result<Self, FixtureError> {
        let path = spec
            .strip_prefix("fixture:")
            .filter(|p| !p.is_empty())
            .ok_or_else(|| FixtureError::BadSpec {
                spec: spec.to_owned(),
            })?;
        Self::load(Path::new(path))
    }

    /// Read and parse a `.jsonl` stream.
    ///
    /// # Errors
    ///
    /// [`FixtureError::Io`] when the file will not open, [`FixtureError::BadLine`]
    /// when a line is neither an event nor an error line.
    pub fn load(path: &Path) -> Result<Self, FixtureError> {
        let text = std::fs::read_to_string(path).map_err(|source| FixtureError::Io {
            path: path.to_owned(),
            source,
        })?;
        let mut steps = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let bad = |message: String| FixtureError::BadLine {
                path: path.to_owned(),
                line: i + 1,
                message,
            };
            let meta: Meta = serde_json::from_str(line).map_err(|e| bad(e.to_string()))?;
            let item = match meta.error {
                Some(spec) => Err(spec.build().map_err(bad)?),
                None => Ok(serde_json::from_str(line).map_err(|e| bad(e.to_string()))?),
            };
            steps.push(Step {
                delay_ms: meta.delay_ms,
                item,
            });
        }
        Ok(Self {
            path: path.to_owned(),
            steps: steps.into(),
            // Generous on purpose: a fixture exists to exercise the code under
            // test, not to be the thing that rejects it. Tests that want a
            // narrow provider construct one with `with_capabilities`.
            capabilities: Capabilities {
                tools: true,
                images: true,
                cache: true,
                max_context: 200_000,
                max_output: 64_000,
            },
        })
    }

    /// Replace the declared capabilities, for tests about what a provider
    /// cannot do.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// The file this provider replays.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Provider for FixtureProvider {
    fn id(&self) -> &str {
        "fixture"
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn stream(
        &self,
        _req: ModelRequest,
        cancel: CancellationToken,
    ) -> BoxStream<'static, Result<ModelEvent, ProviderError>> {
        let steps = Arc::clone(&self.steps);
        Box::pin(futures_util::stream::unfold(
            (0usize, steps, cancel),
            |(i, steps, cancel)| async move {
                let step = steps.get(i)?.clone();
                if let Some(ms) = step.delay_ms.filter(|m| *m > 0) {
                    // `select!` rather than a sleep followed by a check: the
                    // point of the never-ending fixture is that cancellation
                    // does not wait for the delay to elapse.
                    tokio::select! {
                        () = tokio::time::sleep(Duration::from_millis(ms)) => {}
                        () = cancel.cancelled() => return None,
                    }
                } else if cancel.is_cancelled() {
                    return None;
                }
                let terminal = step.item.is_err();
                // An error line is the end of the stream, as a real one is.
                let next = if terminal { steps.len() } else { i + 1 };
                Some((step.item, (next, steps, cancel)))
            },
        ))
    }

    fn counter(&self) -> Arc<dyn TokenCounter> {
        Arc::new(HeuristicCounter::new())
    }

    fn auth(&self) -> Arc<dyn ProviderAuth> {
        Arc::new(NoAuth)
    }
}

/// The fixture provider needs no credential, and says so rather than
/// pretending to have one.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoAuth;

#[async_trait]
impl ProviderAuth for NoAuth {
    fn methods(&self) -> &[AuthMethod] {
        &[]
    }

    async fn state(&self) -> Result<AuthState, ProviderError> {
        Ok(AuthState::Anonymous)
    }

    async fn login(&self, _ctx: &dyn AuthCtx) -> Result<AuthState, ProviderError> {
        Ok(AuthState::Anonymous)
    }

    async fn refresh(&self) -> Result<AuthState, ProviderError> {
        Ok(AuthState::Anonymous)
    }

    async fn logout(&self) -> Result<(), ProviderError> {
        Ok(())
    }
}
