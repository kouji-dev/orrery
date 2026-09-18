//! What can go wrong, and what deliberately cannot.
//!
//! There is no `BudgetExceeded` variant here, and that is the point: a case
//! that ran out of budget produced a *result*, not an error, and it is
//! [`EvalOutcome::BudgetExceeded`](orrery_grader::EvalOutcome::BudgetExceeded).
//! An error is the runner failing, never the thing under test failing.

use std::path::PathBuf;

use orrery_grader::GradeError;

/// A runner failure.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    /// A file would not open, a directory would not be made.
    #[error("{what} `{path}`: {source}")]
    Io {
        /// What was being attempted.
        what: &'static str,
        /// Where.
        path: PathBuf,
        /// Why not.
        source: std::io::Error,
    },
    /// The isolation mode is not implemented yet.
    #[error(
        "{what} is not supported yet — phase 9 ships worktree isolation only \
         (plan 16, open question 1)"
    )]
    Unsupported {
        /// Which capability was asked for.
        what: &'static str,
    },
    /// A suite or a run file would not parse.
    #[error("cannot read the {what}: {detail}")]
    Parse {
        /// Which document.
        what: &'static str,
        /// The parser's complaint.
        detail: String,
    },
    /// The matrix named a profile nothing is bound to.
    #[error("no runner is bound to profile `{profile}` — bind one with `with_runner`")]
    NoRunner {
        /// The unbound profile.
        profile: String,
    },
    /// A case named a grader nothing is bound to.
    #[error("case `{case}` wants grader `{grader}`, which is not installed")]
    NoGrader {
        /// The case.
        case: String,
        /// The grader it wanted.
        grader: String,
    },
    /// The session store refused.
    #[error("session store: {0}")]
    Session(#[from] orrery_session::SessionError),
    /// The provider failed in a way the runner does not retry.
    #[error("provider: {detail}")]
    Provider {
        /// What it said.
        detail: String,
    },
    /// An external agent CLI could not be driven.
    #[error("adapter `{adapter}`: {detail}")]
    Adapter {
        /// Which adapter.
        adapter: String,
        /// What went wrong.
        detail: String,
    },
    /// A grader could not reach a verdict.
    #[error(transparent)]
    Grade(#[from] GradeError),
}

impl EvalError {
    /// An I/O failure with the path that caused it.
    pub(crate) fn io(what: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        EvalError::Io {
            what,
            path: path.into(),
            source,
        }
    }
}
