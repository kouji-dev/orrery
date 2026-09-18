//! What the broker can refuse, and why.

use std::path::PathBuf;

use orrery_policy::TokenError;

/// A broker call did not happen.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    /// The token was not good: spent, revoked, expired, or from nowhere.
    #[error(transparent)]
    Token(#[from] TokenError),
    /// The token permits a different aspect than the call being made.
    #[error("this token is for `{held:?}`, not `{wanted:?}`")]
    WrongAspect {
        /// What the token carries.
        held: orrery_proto::Aspect,
        /// What was asked for.
        wanted: orrery_proto::Aspect,
    },
    /// The token was minted for something else.
    #[error("this token is scoped to `{held}`, not `{wanted}`")]
    OutsideScope {
        /// What the token permits.
        held: String,
        /// What was asked for.
        wanted: String,
    },
    /// The budget's ceiling was reached while the work was being done.
    #[error("{what} exceeded its ceiling of {ceiling} {unit}")]
    LimitExceeded {
        /// Which limit.
        what: &'static str,
        /// The ceiling it passed.
        ceiling: u64,
        /// What the ceiling counts.
        unit: &'static str,
    },
    /// It ran out of wall clock.
    #[error("`{what}` did not finish within {ms}ms")]
    TimedOut {
        /// What was being done.
        what: String,
        /// The window it had.
        ms: u64,
    },
    /// The call was cancelled.
    #[error("the call was cancelled")]
    Cancelled,
    /// There is no such credential under that name.
    #[error("no credential is stored under `{0}`")]
    NoCredential(String),
    /// The broker has no transport, which is the default: it never dials on its
    /// own.
    #[error("no network transport is installed; the broker does not dial by default")]
    NoTransport,
    /// This platform cannot do it.
    #[error("{0}")]
    Unsupported(String),
    /// The operating system said no.
    #[error("{path}: {source}")]
    Io {
        /// What was being touched.
        path: PathBuf,
        /// What it said.
        #[source]
        source: std::io::Error,
    },
}

impl BrokerError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        BrokerError::Io {
            path: path.into(),
            source,
        }
    }
}
