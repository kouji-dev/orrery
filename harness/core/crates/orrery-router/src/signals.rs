//! The cheap signals a routing rule is allowed to read.
//!
//! # What is deliberately absent
//!
//! Everything here is a number somebody already has. A rule may not read a
//! model's opinion, the contents of a file, an embedding, a similarity score or
//! anything else that has to be fetched or generated, because a router that
//! costs a model call to run is a router nobody can afford to run on every
//! pass — and because a decision that depends on a model is not reproducible,
//! which is what eval comparison needs.
//!
//! That rule is enforced by the type rather than by review: [`Signals`] is
//! `Copy`. A handle (`Arc`, a channel, a store), a future, a boxed trait object
//! and a `String` are all not `Copy`, so none of them can be added without the
//! `signals_are_cheap` test failing to compile.
//!
//! [`ABSENT`] lists what was considered and left out, so the next person does
//! not have to guess whether it was an oversight.

use std::collections::BTreeMap;

use orrery_proto::{Aspect, Budget, Usage};
use serde::{Deserialize, Serialize};

/// What a rule may **not** read, and why.
///
/// Asserted against in `tests/rules.rs` only as documentation — the real
/// enforcement is the `Copy` bound.
pub const ABSENT: [(&str, &str); 5] = [
    ("the transcript", "reading it to decide costs a model call"),
    (
        "file contents",
        "an I/O handle would have to live in `Signals`",
    ),
    (
        "an embedding or a similarity score",
        "generated, therefore paid for",
    ),
    (
        "a model's own confidence",
        "a proposal is data the router judges, not a signal it trusts",
    ),
    (
        "wall-clock time",
        "read by the caller and folded into `budget_spent`, so `decide` stays pure",
    ),
];

/// Which permission set a step is running under.
///
/// A mode is **not** a UI state. `plan` is read-only, `execute` is the full
/// granted set and `review` never writes, and entering one is an ordinary
/// capability request — see [`crate::mode`].
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Read-only. The default, because the cheap rung is the default rung.
    #[default]
    Plan,
    /// The full granted set.
    Execute,
    /// Reads and reports. Never writes.
    Review,
}

impl Mode {
    /// The word a `mode(...)` rule is written with.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Plan => "plan",
            Mode::Execute => "execute",
            Mode::Review => "review",
        }
    }

    /// Whether this mode admits an aspect at all.
    ///
    /// This is the mode's own ceiling, applied **before** the policy engine
    /// ever sees the call. It never widens anything — a mode that admits an
    /// aspect still has to clear the rules.
    ///
    /// `plan` is read-only: no `write`, no `mem.write`, and no `spawn` either,
    /// since a process is how a plan-mode agent would change something anyway.
    /// `review` **never writes**, but it does spawn — a verifier that cannot
    /// run the tests is not a verifier — and what that process may touch is the
    /// broker's question, under a capability token, not the mode's.
    #[must_use]
    pub const fn permits(self, aspect: Aspect) -> bool {
        match self {
            Mode::Execute => true,
            Mode::Plan => !matches!(aspect, Aspect::Write | Aspect::MemWrite | Aspect::Spawn),
            Mode::Review => !matches!(aspect, Aspect::Write | Aspect::MemWrite),
        }
    }

    /// Every mode there is.
    #[must_use]
    pub const fn all() -> [Mode; 3] {
        [Mode::Plan, Mode::Execute, Mode::Review]
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the last gate came out.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateOutcome {
    /// It passed.
    Passed,
    /// It failed — which is the signal that justifies a bounded loop.
    Failed,
}

impl GateOutcome {
    /// `1` for passed, `0` for failed, so a rule can compare it like anything
    /// else.
    #[must_use]
    pub const fn as_f64(self) -> f64 {
        match self {
            GateOutcome::Passed => 1.0,
            GateOutcome::Failed => 0.0,
        }
    }
}

/// Everything a routing rule may read.
///
/// `Copy`, on purpose. See the module note.
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Signals {
    /// What this turn has spent so far.
    pub budget_spent: Usage,
    /// What it is allowed to spend.
    pub budget_limit: Budget,
    /// How many turns have run in the current mode.
    pub turns_in_mode: u32,
    /// Which mode is in force.
    pub mode: Mode,
    /// How many reads this turn has made.
    pub reads: u32,
    /// How many writes.
    pub writes: u32,
    /// How many lines the working diff touches.
    pub diff_lines: u32,
    /// How the last gate came out, when one has run.
    pub last_gate: Option<GateOutcome>,
    /// How many times the same call has been made with the same input.
    pub repeated_identical_calls: u32,
}

impl Signals {
    /// Tokens still unspent, floored at zero.
    #[must_use]
    pub const fn remaining_tokens(&self) -> u64 {
        self.budget_limit
            .max_tokens
            .saturating_sub(self.budget_spent.total_tokens())
    }

    /// How much of the token budget has gone, in `0.0..=1.0`. An unset limit
    /// reads as `0.0`: nothing has been spent *of nothing*.
    #[must_use]
    pub fn budget_fraction(&self) -> f64 {
        if self.budget_limit.max_tokens == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let spent = self.budget_spent.total_tokens() as f64;
        #[allow(clippy::cast_precision_loss)]
        let limit = self.budget_limit.max_tokens as f64;
        (spent / limit).min(1.0)
    }

    /// The numbers behind a decision, keyed by the name a rule writes.
    ///
    /// This is what lands in the audit, so "why five and not two" is answerable
    /// afterwards from the stream alone.
    #[must_use]
    pub fn values(&self) -> BTreeMap<String, f64> {
        let mut out = BTreeMap::new();
        out.insert("budget_fraction".to_owned(), self.budget_fraction());
        #[allow(clippy::cast_precision_loss)]
        out.insert(
            "tokens_spent".to_owned(),
            self.budget_spent.total_tokens() as f64,
        );
        #[allow(clippy::cast_precision_loss)]
        out.insert(
            "tokens_remaining".to_owned(),
            self.remaining_tokens() as f64,
        );
        out.insert("turns_in_mode".to_owned(), f64::from(self.turns_in_mode));
        out.insert("reads".to_owned(), f64::from(self.reads));
        out.insert("writes".to_owned(), f64::from(self.writes));
        out.insert("diff_lines".to_owned(), f64::from(self.diff_lines));
        out.insert(
            "repeated_identical_calls".to_owned(),
            f64::from(self.repeated_identical_calls),
        );
        if let Some(gate) = self.last_gate {
            out.insert("last_gate".to_owned(), gate.as_f64());
        }
        out
    }
}
