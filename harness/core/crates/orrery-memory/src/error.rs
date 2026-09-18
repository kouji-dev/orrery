//! What memory can refuse, and why.
//!
//! A refusal is a **value**, like a policy decision: memory is never
//! load-bearing, so nothing here ends a turn. The kernel logs it, records it and
//! carries on.

/// Something memory would not or could not do.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MemError {
    /// The provider does not keep this scope at all, and says so rather than
    /// succeeding silently.
    #[error("`{provider}` does not support the `{scope}` scope")]
    UnsupportedScope {
        /// Which provider.
        provider: String,
        /// Which scope, by name: `global`, `session`, `branch`, …
        scope: String,
    },
    /// The kernel refused before the store was reached: a permission rule, the
    /// visibility rule, or a scope that no longer exists.
    #[error("{reason}")]
    Denied {
        /// What was asked for, in rule-grammar form — `mem.write(global)`.
        request: String,
        /// Why, in words a person can act on.
        reason: String,
    },
    /// The store tried and something went wrong.
    #[error("`{provider}`: {message}")]
    Backend {
        /// Which provider.
        provider: String,
        /// What went wrong.
        message: String,
    },
}

impl MemError {
    /// A kernel-side refusal.
    #[must_use]
    pub fn denied(request: impl Into<String>, reason: impl std::fmt::Display) -> Self {
        Self::Denied {
            request: request.into(),
            reason: reason.to_string(),
        }
    }

    /// A store-side failure from anything that renders.
    #[must_use]
    pub fn backend(provider: impl Into<String>, message: impl std::fmt::Display) -> Self {
        Self::Backend {
            provider: provider.into(),
            message: message.to_string(),
        }
    }
}
