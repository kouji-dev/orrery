//! JUnit output, and the exit code CI actually reads.
//!
//! The baseline is a **committed file** rather than a registry lookup: a
//! regression threshold that can change without a review is not a threshold
//! (plan 16, open question 3).

use std::collections::BTreeMap;

use orrery_grader::EvalOutcome;
use serde::{Deserialize, Serialize};

use crate::report::RunReport;

/// What the suite did last time.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// The suite these outcomes belong to.
    #[serde(default)]
    pub suite: String,
    /// One outcome per [`crate::EvalResult::key`].
    #[serde(default)]
    pub outcomes: BTreeMap<String, EvalOutcome>,
}

impl Baseline {
    /// Take a report as the new baseline.
    #[must_use]
    pub fn from_report(report: &RunReport) -> Self {
        Self {
            suite: report.suite.clone(),
            outcomes: report
                .results
                .iter()
                .map(|r| (r.key(), r.outcome))
                .collect(),
        }
    }

    /// Read a committed baseline.
    ///
    /// # Errors
    ///
    /// Whatever `serde_json` says.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Write one.
    ///
    /// # Errors
    ///
    /// Whatever `serde_json` says.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Every case that passed here and does not pass in `report`.
    #[must_use]
    pub fn regressions(&self, report: &RunReport) -> Vec<String> {
        let mut out: Vec<String> = report
            .results
            .iter()
            .filter(|r| {
                self.outcomes
                    .get(&r.key())
                    .is_some_and(|b| b.is_pass() && !r.outcome.is_pass())
            })
            .map(|r| r.key())
            .collect();
        out.sort();
        out
    }
}

/// The process exit code for `--format junit`.
///
/// With a baseline: non-zero when something that used to pass no longer does.
/// Without one: non-zero when anything failed at all, because "no baseline" is
/// not a reason to call a red suite green.
#[must_use]
pub fn exit_code(report: &RunReport, baseline: Option<&Baseline>) -> i32 {
    match baseline {
        Some(base) => i32::from(!base.regressions(report).is_empty()),
        None => i32::from(report.passed() != report.results.len()),
    }
}

/// The report as a JUnit document.
///
/// A budget-exceeded case is a JUnit `failure` rather than an `error`: it is a
/// statement about the run, not about the harness.
#[must_use]
pub fn junit_xml(report: &RunReport) -> String {
    let failures = report
        .results
        .iter()
        .filter(|r| matches!(r.outcome, EvalOutcome::Fail | EvalOutcome::BudgetExceeded))
        .count();
    let errors = report
        .results
        .iter()
        .filter(|r| r.outcome == EvalOutcome::Error)
        .count();

    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<testsuite name=\"{}\" tests=\"{}\" failures=\"{failures}\" errors=\"{errors}\" \
         id=\"{}\">\n",
        escape(&report.suite),
        report.results.len(),
        escape(&report.run_id),
    ));
    for r in &report.results {
        out.push_str(&format!(
            "  <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\">\n",
            escape(&r.profile),
            escape(&r.key()),
            r.timing.wall_ms as f64 / 1000.0,
        ));
        match r.outcome {
            EvalOutcome::Pass => {}
            EvalOutcome::Error => out.push_str(&format!(
                "    <error message=\"{}\"/>\n",
                escape(r.outcome.tag())
            )),
            _ => out.push_str(&format!(
                "    <failure message=\"{}\"/>\n",
                escape(r.outcome.tag())
            )),
        }
        // The provenance rides along, so a CI artefact cannot lose the one
        // thing that makes two cost numbers incomparable.
        out.push_str(&format!(
            "    <system-out>cost {} tokens, {}</system-out>\n",
            r.cost.total_tokens(),
            escape(&r.cost_provenance.label()),
        ));
        out.push_str("  </testcase>\n");
    }
    out.push_str("</testsuite>\n");
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
