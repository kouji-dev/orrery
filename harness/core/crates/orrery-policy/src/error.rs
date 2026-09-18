//! What can go wrong before a decision, and what a token redemption can answer.
//!
//! Note what is *not* here: a denial. Denial is a value —
//! [`crate::Decision::Deny`] — not an error.

use std::path::{Path, PathBuf};

/// A rule that did not parse, with the file and line it was written on.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{where_}: {message}{rule_suffix}", where_ = Self::where_of(file, *line), rule_suffix = Self::suffix(rule))]
pub struct ParseError {
    /// The file, when it came from one.
    pub file: PathBuf,
    /// The line, 1-based. Zero when it is not known.
    pub line: u32,
    /// The rule as written.
    pub rule: String,
    /// What is wrong with it.
    pub message: String,
}

impl ParseError {
    /// A parse error with no file behind it.
    #[must_use]
    pub fn bare(message: impl Into<String>, rule: &str) -> Self {
        Self {
            file: PathBuf::new(),
            line: 0,
            rule: rule.to_owned(),
            message: message.into(),
        }
    }

    /// A parse error that names its file and line.
    #[must_use]
    pub fn at(file: &Path, line: u32, message: impl Into<String>, rule: &str) -> Self {
        Self {
            file: file.to_path_buf(),
            line,
            rule: rule.to_owned(),
            message: message.into(),
        }
    }

    /// What is wrong, without the location.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    fn where_of(file: &Path, line: u32) -> String {
        match (file.as_os_str().is_empty(), line) {
            (true, _) => "rule".to_owned(),
            (false, 0) => file.display().to_string(),
            (false, n) => format!("{}:{n}", file.display()),
        }
    }

    fn suffix(rule: &str) -> String {
        if rule.trim().is_empty() {
            String::new()
        } else {
            format!(" (`{rule}`)")
        }
    }
}

/// Something a person should be told at load, that is not fatal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// Which file.
    pub file: PathBuf,
    /// Which line.
    pub line: u32,
    /// The rule it is about.
    pub rule: String,
    /// What to say.
    pub message: String,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: {} (`{}`)",
            self.file.display(),
            self.line,
            self.message,
            self.rule
        )
    }
}

/// Building a rule set went wrong.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    /// A rule did not parse.
    #[error(transparent)]
    Parse(#[from] ParseError),
    /// A pattern did not compile.
    #[error("the pattern in `{rule}` did not compile: {message}")]
    Pattern {
        /// The rule as written.
        rule: String,
        /// What the compiler said.
        message: String,
    },
    /// A `re:` rule appeared under a managed layer that forbids the escape hatch.
    #[error("`{rule}` uses the `re:` escape hatch, which the managed layer forbids")]
    RegexForbidden {
        /// The rule as written.
        rule: String,
    },
}

impl PolicyError {
    pub(crate) fn pattern(rule: &str, message: impl Into<String>) -> Self {
        PolicyError::Pattern {
            rule: rule.to_owned(),
            message: message.into(),
        }
    }
}

/// Why a capability token was not honoured.
///
/// Lives here rather than in `orrery-broker` because the ledger does the
/// checking and the broker depends on this crate, not the other way round. The
/// broker's own error type wraps it.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// It was redeemed already. A token is single-use.
    #[error("this capability token has already been redeemed")]
    Spent,
    /// The call it belonged to was cancelled.
    #[error("this capability token was revoked when its call was cancelled")]
    Revoked,
    /// Its deadline passed.
    #[error("this capability token expired before it was redeemed")]
    Expired,
    /// It was never minted by this ledger.
    #[error("this capability token was not minted here")]
    Unknown,
}

/// A handler misbehaved.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HandlerError {
    /// It returned an error of its own.
    #[error("the permission handler failed: {0}")]
    Failed(String),
    /// It panicked. The call is denied: a handler that panics fails **closed**.
    #[error("the permission handler panicked; the call is denied")]
    Panicked,
    /// It did not answer in time.
    #[error("the permission handler did not answer in time")]
    TimedOut,
}
