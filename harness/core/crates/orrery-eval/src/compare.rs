//! Comparing two runs, and refusing to when they are not comparable.
//!
//! Two runs with different reproducibility settings are two different
//! experiments. Showing their numbers next to each other invites exactly the
//! conclusion the settings invalidate, so [`compare`] refuses by default and
//! the refusal says which setting differs. `--force` is for the person who has
//! read that sentence and wants the comparison anyway.

use orrery_grader::EvalOutcome;
use serde::{Deserialize, Serialize};

use crate::report::RunReport;

/// Why two runs cannot be compared.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CompareError {
    /// The runs pinned different things.
    #[error(
        "run `{a}` and run `{b}` are not comparable: {detail}. \
         A memory provider or a bound router changes what a case does, so the \
         numbers are of different experiments — pass --force to compare anyway."
    )]
    Incomparable {
        /// The first run.
        a: String,
        /// The second.
        b: String,
        /// Which setting differs.
        detail: String,
    },
    /// They are not even the same suite.
    #[error("run `{a}` ran suite `{suite_a}` and run `{b}` ran suite `{suite_b}`")]
    DifferentSuites {
        /// The first run.
        a: String,
        /// Its suite.
        suite_a: String,
        /// The second run.
        b: String,
        /// Its suite.
        suite_b: String,
    },
}

/// One case that moved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    /// The case and matrix point, as [`crate::EvalResult::key`] spells it.
    pub key: String,
    /// How it came out before.
    pub before: EvalOutcome,
    /// How it comes out now.
    pub after: EvalOutcome,
    /// The change in tokens, `after - before`.
    pub token_delta: i64,
}

/// What changed between two runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    /// Cases that passed before and do not now.
    pub regressions: Vec<Delta>,
    /// Cases that failed before and pass now.
    pub improvements: Vec<Delta>,
    /// Cases only one of the two runs has.
    pub unmatched: Vec<String>,
    /// Total tokens, `after - before`.
    pub token_delta: i64,
    /// Set when the two runs were compared despite disagreeing.
    pub forced: bool,
}

impl Comparison {
    /// Whether anything got worse.
    #[must_use]
    pub fn has_regressions(&self) -> bool {
        !self.regressions.is_empty()
    }

    /// One line per change.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.forced {
            out.push_str("(forced: these runs pinned different things)\n");
        }
        for d in &self.regressions {
            out.push_str(&format!(
                "REGRESSED {} {} -> {}\n",
                d.key,
                d.before.tag(),
                d.after.tag()
            ));
        }
        for d in &self.improvements {
            out.push_str(&format!(
                "improved  {} {} -> {}\n",
                d.key,
                d.before.tag(),
                d.after.tag()
            ));
        }
        for key in &self.unmatched {
            out.push_str(&format!("unmatched {key}\n"));
        }
        out.push_str(&format!("tokens {:+}\n", self.token_delta));
        out
    }
}

/// Compare `after` against `before`.
///
/// # Errors
///
/// [`CompareError::Incomparable`] when the runs pinned different things and
/// `force` is false; [`CompareError::DifferentSuites`] always, because no flag
/// makes two suites the same suite.
pub fn compare(
    before: &RunReport,
    after: &RunReport,
    force: bool,
) -> Result<Comparison, CompareError> {
    if before.suite != after.suite {
        return Err(CompareError::DifferentSuites {
            a: before.run_id.clone(),
            suite_a: before.suite.clone(),
            b: after.run_id.clone(),
            suite_b: after.suite.clone(),
        });
    }

    let mut differences = Vec::new();
    if before.reproducibility.memory != after.reproducibility.memory {
        differences.push(format!(
            "memory is `{}` in one and `{}` in the other",
            describe_memory(&before.reproducibility.memory),
            describe_memory(&after.reproducibility.memory)
        ));
    }
    if before.reproducibility.router != after.reproducibility.router {
        differences.push(format!(
            "router is `{}` in one and `{}` in the other",
            describe_router(&before.reproducibility.router),
            describe_router(&after.reproducibility.router)
        ));
    }
    if !differences.is_empty() && !force {
        return Err(CompareError::Incomparable {
            a: before.run_id.clone(),
            b: after.run_id.clone(),
            detail: differences.join("; "),
        });
    }

    let mut out = Comparison {
        forced: !differences.is_empty(),
        ..Comparison::default()
    };

    for a in &before.results {
        let key = a.key();
        let Some(b) = after.results.iter().find(|r| r.key() == key) else {
            out.unmatched.push(key);
            continue;
        };
        let delta = Delta {
            key,
            before: a.outcome,
            after: b.outcome,
            token_delta: b.cost.total_tokens() as i64 - a.cost.total_tokens() as i64,
        };
        out.token_delta += delta.token_delta;
        if a.outcome.is_pass() && !b.outcome.is_pass() {
            out.regressions.push(delta);
        } else if !a.outcome.is_pass() && b.outcome.is_pass() {
            out.improvements.push(delta);
        }
    }
    for b in &after.results {
        if !before.results.iter().any(|a| a.key() == b.key()) {
            out.unmatched.push(b.key());
        }
    }
    out.unmatched.sort();
    out.unmatched.dedup();
    Ok(out)
}

fn describe_memory(mode: &crate::run::MemoryMode) -> String {
    match mode {
        crate::run::MemoryMode::Off => "off".to_owned(),
        crate::run::MemoryMode::Provider { id, .. } => id.clone(),
    }
}

fn describe_router(mode: &crate::run::RouterMode) -> String {
    match mode {
        crate::run::RouterMode::Declared => "declared".to_owned(),
        crate::run::RouterMode::Agent { name } => format!("agent {name}"),
    }
}
