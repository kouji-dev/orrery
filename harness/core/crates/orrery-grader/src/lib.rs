//! The Grader trait and its input and score types, published so a grader extension never depends on the runner.
//!
//! This crate is deliberately tiny, and the smallness is the design. A grader
//! is an extension; the runner that calls it is core. If the trait lived in the
//! runner, every grader would link the runner, and "add a suite" would mean
//! "depend on the evaluation engine". So the trait, the input it is handed and
//! the score it returns live here on their own, over `orrery-proto` and nothing
//! else.
//!
//! ```
//! use async_trait::async_trait;
//! use orrery_grader::{GradeError, GradeInput, Grader, Score};
//!
//! /// Passes when the workspace has a `PASS` file in it.
//! struct MarkerFile;
//!
//! #[async_trait]
//! impl Grader for MarkerFile {
//!     fn id(&self) -> &str {
//!         "marker-file"
//!     }
//!
//!     async fn grade(&self, input: GradeInput) -> Result<Score, GradeError> {
//!         Ok(if input.workspace.join("PASS").exists() {
//!             Score::pass()
//!         } else {
//!             Score::fail("no PASS file")
//!         })
//!     }
//! }
//! ```
//!
//! Implementation plan: `harness/docs/plans/16-eval-runner.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::path::PathBuf;

use async_trait::async_trait;
use orrery_proto::{SessionRef, Surface, SurfaceKind, TextStyle, Usage};
use serde::{Deserialize, Serialize};

/// How a case came out.
///
/// Four values and no more. This is the vocabulary a report is written in and
/// the vocabulary a regression is computed in, so it is closed on purpose: a
/// fifth outcome would change what every stored baseline means.
///
/// [`BudgetExceeded`](EvalOutcome::BudgetExceeded) is separate from
/// [`Error`](EvalOutcome::Error) because running out of money is the system
/// working, not the system breaking.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvalOutcome {
    /// The grader was satisfied.
    Pass,
    /// The grader was not.
    Fail,
    /// The case never got as far as being graded.
    Error,
    /// A ceiling stopped it.
    BudgetExceeded,
}

impl EvalOutcome {
    /// Whether this is the good one.
    #[must_use]
    pub const fn is_pass(self) -> bool {
        matches!(self, EvalOutcome::Pass)
    }

    /// The stable one-word discriminant, for a report column or a JUnit
    /// attribute.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            EvalOutcome::Pass => "pass",
            EvalOutcome::Fail => "fail",
            EvalOutcome::Error => "error",
            EvalOutcome::BudgetExceeded => "budget-exceeded",
        }
    }
}

/// Which case, in which suite, is being graded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseRef {
    /// The suite's name.
    pub suite: String,
    /// The case's id within it.
    pub case: String,
}

impl CaseRef {
    /// Name a case.
    #[must_use]
    pub fn new(suite: impl Into<String>, case: impl Into<String>) -> Self {
        Self {
            suite: suite.into(),
            case: case.into(),
        }
    }
}

/// Everything a grader is allowed to look at.
///
/// The workspace as the case left it, and the transcript as the store kept it.
/// Not the runner, not the provider, not the config: a grader that could reach
/// those could change what it is grading.
#[derive(Clone, Debug, PartialEq)]
pub struct GradeInput {
    /// The case's workspace, after the run.
    pub workspace: PathBuf,
    /// The session the run produced. Replayable.
    pub transcript: SessionRef,
    /// Which case this is.
    pub case: CaseRef,
}

/// What a grader decided.
#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    /// The verdict.
    pub outcome: EvalOutcome,
    /// A number, when the grader has one. Higher is better.
    pub score: Option<f64>,
    /// Why, in something renderable.
    pub detail: Surface,
    /// What *grading* cost, when grading costs anything.
    ///
    /// Only a judge-model grader fills this in, and it is separate from the
    /// run's own cost on purpose: a judge that costs more than the run it
    /// grades should be visible in the report rather than folded into the
    /// number it is grading.
    pub judge_cost: Option<Usage>,
}

impl Score {
    /// A score with a one-line reason and no number.
    #[must_use]
    pub fn new(outcome: EvalOutcome, detail: impl Into<String>) -> Self {
        Self {
            outcome,
            score: None,
            detail: Surface::new(SurfaceKind::Text {
                value: detail.into(),
                style: Some(match outcome {
                    EvalOutcome::Pass => TextStyle::Success,
                    EvalOutcome::Fail => TextStyle::Warning,
                    _ => TextStyle::Error,
                }),
            }),
            judge_cost: None,
        }
    }

    /// Passed, with nothing more to say.
    #[must_use]
    pub fn pass() -> Self {
        Self::new(EvalOutcome::Pass, "passed")
    }

    /// Failed, with a reason.
    #[must_use]
    pub fn fail(detail: impl Into<String>) -> Self {
        Self::new(EvalOutcome::Fail, detail)
    }

    /// Attach a number.
    #[must_use]
    pub fn with_score(mut self, score: f64) -> Self {
        self.score = Some(score);
        self
    }

    /// Attach what grading itself cost.
    #[must_use]
    pub fn with_judge_cost(mut self, cost: Usage) -> Self {
        self.judge_cost = Some(cost);
        self
    }

    /// Replace the detail with a richer surface.
    #[must_use]
    pub fn with_detail(mut self, detail: Surface) -> Self {
        self.detail = detail;
        self
    }
}

/// Why a grader could not decide.
///
/// Not the same as deciding "fail": a grader that could not run leaves the case
/// [`EvalOutcome::Error`], which a report must not count as a regression in the
/// thing under test.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GradeError {
    /// The grader needed something it was not given.
    #[error("grader `{grader}` cannot run: {detail}")]
    Unavailable {
        /// Which grader.
        grader: String,
        /// What is missing.
        detail: String,
    },
    /// The grader's own configuration is wrong.
    #[error("grader `{grader}` is misconfigured: {detail}")]
    Misconfigured {
        /// Which grader.
        grader: String,
        /// What is wrong.
        detail: String,
    },
    /// Something underneath failed.
    #[error("grader `{grader}` failed: {detail}")]
    Failed {
        /// Which grader.
        grader: String,
        /// What went wrong.
        detail: String,
    },
}

impl GradeError {
    /// Which grader the failure belongs to.
    #[must_use]
    pub fn grader(&self) -> &str {
        match self {
            GradeError::Unavailable { grader, .. }
            | GradeError::Misconfigured { grader, .. }
            | GradeError::Failed { grader, .. } => grader,
        }
    }
}

/// Something that turns a finished case into a [`Score`].
///
/// Object-safe: a runner holds `Arc<dyn Grader>` and picks one by id out of
/// whatever the profile installed.
#[async_trait]
pub trait Grader: Send + Sync {
    /// The id a case selects this grader by.
    fn id(&self) -> &str;

    /// Score one finished case.
    ///
    /// # Errors
    ///
    /// [`GradeError`] when the grader could not reach a verdict at all. A case
    /// the grader is simply unhappy with is `Ok(Score::fail(..))`, not an error.
    async fn grade(&self, input: GradeInput) -> Result<Score, GradeError>;
}
