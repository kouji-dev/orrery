//! The built-in graders: command, assertion and model.
//!
//! One extension, three graders, and it depends on `orrery-grader` rather than
//! on the runner — which is the whole reason that trait lives in a crate of its
//! own. Nothing here can see an `EvalRun`, a matrix or a report.
//!
//! | Grader | How it scores | Use for |
//! |---|---|---|
//! | [`command`] | Exit code of a script | SWE-style patch benchmarks |
//! | [`assertion`] | Declared checks on files, diffs, tool calls | Behaviour and safety |
//! | [`model`] | A judge model with a rubric, **with its own cost counted** | Open-ended quality |
//!
//! # Two rules the three of them share
//!
//! - **Everything is brokered.** The command grader spawns through the broker,
//!   the assertion grader reads through it. A grader is somebody else's command
//!   line out of somebody else's suite file, and the one thing it must not be
//!   is the trusted process.
//! - **A grader that could not look is not a grader that said no.** A denial, a
//!   missing store or an unparseable config is [`GradeError::Unavailable`] or
//!   [`GradeError::Misconfigured`], which the runner records as
//!   [`EvalOutcome::Error`] — never as the case failing.
//!
//! Implementation plan: `harness/docs/plans/16-eval-runner.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod assertion;
pub mod command;
pub mod model;

use std::sync::Arc;

use orrery_ext_api::BrokerFacade;
use orrery_grader::Grader;
use orrery_provider::Provider;
use orrery_session::SessionStore;

pub use assertion::{AssertionConfig, AssertionGrader, Check};
pub use command::{CommandConfig, CommandGrader};
pub use model::{ModelConfig, ModelGrader};

// Re-exported so a caller that installs these graders does not have to name
// `orrery-grader` as well.
pub use orrery_grader::{EvalOutcome, GradeError, GradeInput, Score};

/// Every grader this extension provides, ready to install.
///
/// The judge is optional because a judge costs money: a harness with no
/// provider bound for grading gets the two free graders rather than a failure
/// at load time.
#[must_use]
pub fn graders(
    broker: Arc<dyn BrokerFacade>,
    store: Option<Arc<dyn SessionStore>>,
    judge: Option<(Arc<dyn Provider>, String)>,
) -> Vec<Arc<dyn Grader>> {
    let mut assertion = AssertionGrader::new(Arc::clone(&broker));
    if let Some(store) = store {
        assertion = assertion.with_transcript(store);
    }
    let mut out: Vec<Arc<dyn Grader>> =
        vec![Arc::new(CommandGrader::new(broker)), Arc::new(assertion)];
    if let Some((provider, model)) = judge {
        out.push(Arc::new(ModelGrader::new(provider, model)));
    }
    out
}
