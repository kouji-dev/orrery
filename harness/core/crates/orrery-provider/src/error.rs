//! The failure classification — which *is* the contract.
//!
//! Provider code never sleeps and never retries: it says what happened, and the
//! kernel decides whether to spend more budget on it. That only works if the
//! classification is honest, so every variant is either retryable or terminal
//! and `is_retryable` is a total function over them.

/// Everything a provider can fail with.
///
/// Split into two halves on purpose. The retryable half is worth backing off
/// on; the terminal half is not, and pretending otherwise burns a turn budget
/// on a request that will never succeed.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProviderError {
    // ---- retryable -------------------------------------------------------
    /// The provider asked us to slow down. Retryable **with or without** a
    /// `Retry-After`: a 429 that omits the header is still a 429.
    #[error("rate limited{}", .retry_after_ms.map(|m| format!(", retry after {m}ms")).unwrap_or_default())]
    RateLimited {
        /// What the provider asked us to wait, when it said.
        retry_after_ms: Option<u64>,
    },
    /// A 5xx. The request was well-formed; the other end was not well.
    #[error("server error {status}")]
    ServerError {
        /// The HTTP status.
        status: u16,
    },
    /// No response inside the deadline.
    #[error("timeout after {elapsed_ms}ms")]
    Timeout {
        /// How long we waited.
        elapsed_ms: u64,
    },
    /// The socket never got there: DNS, TLS, a reset.
    #[error("connection: {0}")]
    Connection(String),

    // ---- terminal --------------------------------------------------------
    /// No credential, a rejected credential, or an expired one.
    #[error("authentication: {0}")]
    Auth(String),
    /// The provider rejected the request itself. Retrying sends the same bytes.
    #[error("bad request: {0}")]
    BadRequest(String),
    /// The model declined. A product decision, not a transport failure.
    #[error("refused: {0}")]
    Refusal(String),
    /// Separate from [`ProviderError::BadRequest`] because the kernel's answer
    /// is different: compact the context and try again rather than fail the
    /// turn.
    #[error("context too long: {tokens} > {max}")]
    ContextTooLong {
        /// What we tried to send.
        tokens: u64,
        /// What the model accepts.
        max: u64,
    },
    /// The model id is not one this provider serves.
    #[error("model {0} not available")]
    NoSuchModel(String),
}

impl ProviderError {
    /// True when the kernel may back off and try again, charged to the turn
    /// budget.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimited { .. }
            | Self::ServerError { .. }
            | Self::Timeout { .. }
            | Self::Connection(_) => true,
            Self::Auth(_)
            | Self::BadRequest(_)
            | Self::Refusal(_)
            | Self::ContextTooLong { .. }
            | Self::NoSuchModel(_) => false,
        }
    }

    /// A stable, machine-readable code for the audit log and the wire.
    ///
    /// Stable across providers: an `anthropic` 429 and an `openai` 429 both
    /// read `rate_limited`, so a dashboard does not need a per-vendor table.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::RateLimited { .. } => "rate_limited",
            Self::ServerError { .. } => "server",
            Self::Timeout { .. } => "timeout",
            Self::Connection(_) => "connection",
            Self::Auth(_) => "auth",
            Self::BadRequest(_) => "bad_request",
            Self::Refusal(_) => "refusal",
            Self::ContextTooLong { .. } => "context_too_long",
            Self::NoSuchModel(_) => "no_such_model",
        }
    }
}
