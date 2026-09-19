//! The evaluation runner: isolation, results, compare, replay and cross-harness adapters.
//!
//! Running a benchmark is a core capability rather than an extension, because a
//! reproducible run needs what only the kernel has: deterministic session
//! construction, a pinned profile, budget enforcement, real token counts at the
//! provider boundary and an audit trail. An extension can add a *suite*; it
//! cannot make a run reproducible.
//!
//! # The four things this crate makes true
//!
//! - **Cost is read at the provider boundary.** [`telemetry::BoundaryMeter`]
//!   has no method that accepts a `Usage`; the only way in is a
//!   `&ModelEvent`. There is no estimation path to audit because there is no
//!   estimation path to write.
//! - **Reproducibility is declared.** [`run::EvalRun`] names its memory and
//!   router modes, the defaults are the reproducible ones, and
//!   [`compare::compare`] refuses two runs that disagree without `--force`.
//! - **Cases never share state.** [`isolate::CaseWorkspace`] is per case and
//!   cleans up in `Drop`, so a panic takes its workspace with it.
//! - **Somebody else's number is labelled as theirs.**
//!   [`report::CostProvenance`] rides next to every cost, all the way into the
//!   JUnit artefact.
//!
//! # The shape of a run
//!
//! ```text
//! EvalRun ── matrix ──▶ points ─┐
//! Suite   ── cases  ────────────┴▶ Isolator ▶ CaseRunner ▶ Grader ▶ RunReport
//! ```
//!
//! A [`run::CaseRunner`] is either ours ([`run::HarnessRunner`], measured) or a
//! competing CLI ([`adapter::ExternalRunner`], reported). Both write a session
//! into the same store, so the same graders and the same
//! [`replay`] work either way.
//!
//! Implementation plan: `harness/docs/plans/16-eval-runner.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod adapter;
pub mod case;
pub mod compare;
pub mod conformance;
pub mod error;
pub mod isolate;
pub mod junit;
pub mod matrix;
pub mod replay;
pub mod report;
pub mod run;
pub mod telemetry;

pub use adapter::{AdapterEntry, AdapterSpec, ExternalRunner, ParseMode, ReportedCost};
pub use case::{EvalCase, GraderSpec, Suite, WorkspaceSpec};
pub use compare::{CompareError, Comparison, Delta, compare};
pub use conformance::{Singletons, run_conformance};
pub use error::EvalError;
pub use isolate::{CaseWorkspace, Isolator};
pub use junit::{Baseline, exit_code, junit_xml};
pub use matrix::{Matrix, MatrixPoint};
pub use replay::{Replay, replay};
pub use report::{CostProvenance, EvalResult, RoleCost, RunReport, Timing};
pub use run::{
    CaseCtx, CaseRunner, EvalRun, EvalRunner, HarnessRunner, Isolation, MemoryMode,
    Reproducibility, RoleBinding, RouterMode, RunOutput,
};
pub use telemetry::{BoundaryMeter, EstimatorProbe};

// Re-exported so a caller does not have to name `orrery-grader` to read a
// result, and so that the outcome vocabulary has exactly one definition.
pub use orrery_grader::{EvalOutcome, GradeInput, Grader, Score};
