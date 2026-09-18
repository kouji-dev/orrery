//! What a run produced, and where each number came from.
//!
//! # Two kinds of cost, never mixed
//!
//! Our own runs read their cost at the provider boundary: the meter in
//! [`crate::telemetry`] sums what the provider *said*, event by event, and
//! there is no code path that estimates one. An external CLI has no such
//! boundary for us to stand at, so what we get is whatever that tool chose to
//! print. [`CostProvenance`] carries the difference all the way into the
//! report, and [`RunReport::render`] prints it, because two numbers of
//! different provenance shown side by side without a label are a lie of
//! omission.

use std::collections::BTreeMap;

use orrery_grader::EvalOutcome;
use orrery_proto::{Role, SessionRef, Surface, Usage};
use serde::{Deserialize, Serialize};

use crate::matrix::MatrixPoint;
use crate::run::Reproducibility;

/// Where a cost number came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CostProvenance {
    /// Summed from the provider's own usage events, as they arrived, by the
    /// kernel's telemetry. Nothing estimated it.
    MeasuredAtProviderBoundary,
    /// Read out of what an external agent CLI printed. It is that tool's
    /// number, not our measurement, and it is not comparable with ours.
    ReportedByTool {
        /// Which tool reported it.
        tool: String,
    },
}

impl CostProvenance {
    /// Whether we measured this ourselves.
    #[must_use]
    pub const fn is_measured(&self) -> bool {
        matches!(self, CostProvenance::MeasuredAtProviderBoundary)
    }

    /// The words a report prints next to the number.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            CostProvenance::MeasuredAtProviderBoundary => {
                "measured at the provider boundary".to_owned()
            }
            CostProvenance::ReportedByTool { tool } => format!("reported by {tool}"),
        }
    }
}

/// What one role spent.
///
/// Attribution is **by step, not by model**: a role bound to the same model as
/// the main loop still has its own line, because what is being attributed is
/// which part of the loop spent the tokens.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleCost {
    /// What it spent.
    pub usage: Usage,
    /// How many provider passes it took to spend it.
    pub passes: u32,
}

impl RoleCost {
    /// Add one pass's spend.
    pub fn add(&mut self, usage: Usage) {
        self.usage += usage;
        self.passes = self.passes.saturating_add(1);
    }
}

/// Cost per role, in a stable order.
///
/// Not a `BTreeMap<Role, RoleCost>`, for a boring reason with a good outcome:
/// [`Role`] is `#[non_exhaustive]` and derives no `Ord`, and adding one to a
/// crate this plan does not own is not this plan's business. Keying by the
/// role's own wire tag gives the same determinism, serialises as the obvious
/// JSON object, and survives a role being added later.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ByRole(BTreeMap<String, RoleCost>);

impl ByRole {
    /// Nothing attributed yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Note that a role is about to take a pass, spending nothing yet.
    pub fn touch(&mut self, role: Role) {
        self.0.entry(role_tag(role).to_owned()).or_default();
    }

    /// Attribute one pass's spend.
    pub fn add(&mut self, role: Role, usage: Usage) {
        self.0
            .entry(role_tag(role).to_owned())
            .or_default()
            .add(usage);
    }

    /// Count a pass that spent nothing, so a silent role still shows up.
    pub fn count_pass(&mut self, role: Role) {
        let entry = self.0.entry(role_tag(role).to_owned()).or_default();
        if entry.passes == 0 {
            entry.passes = 1;
        }
    }

    /// What one role spent.
    #[must_use]
    pub fn get(&self, role: Role) -> Option<&RoleCost> {
        self.0.get(role_tag(role))
    }

    /// How many roles are attributed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether nothing is attributed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Every attributed role, by tag, in a stable order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &RoleCost)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

impl std::ops::Index<&Role> for ByRole {
    type Output = RoleCost;

    fn index(&self, role: &Role) -> &RoleCost {
        self.get(*role)
            .unwrap_or_else(|| panic!("no cost attributed to {}", role_tag(*role)))
    }
}

/// A role's stable wire tag, as `orrery-proto` serialises it.
#[must_use]
pub fn role_tag(role: Role) -> &'static str {
    match role {
        Role::Planner => "planner",
        Role::Executor => "executor",
        Role::Verifier => "verifier",
        Role::Compactor => "compactor",
        Role::Summariser => "summariser",
        Role::Router => "router",
        Role::Grader => "grader",
        // `Role` is `#[non_exhaustive]`: a role added later is attributed under
        // a name rather than dropped on the floor.
        _ => "other",
    }
}

/// Where the time went.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timing {
    /// Start to finish.
    pub wall_ms: u64,
    /// Inside a provider call.
    pub model_ms: u64,
    /// Inside a tool.
    pub tool_ms: u64,
}

/// One case, on one point of the matrix.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvalResult {
    /// The case id.
    pub case: String,
    /// The profile it ran under.
    pub profile: String,
    /// The model it ran against.
    pub model: String,
    /// The seed, when the point carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// How it came out.
    pub outcome: EvalOutcome,
    /// The grader's number, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// What it cost.
    pub cost: Usage,
    /// Where that number came from. Never assume; it is written down.
    pub cost_provenance: CostProvenance,
    /// What *grading* cost, when a judge model was involved. Kept apart from
    /// `cost` on purpose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge_cost: Option<Usage>,
    /// Where the tokens actually went.
    #[serde(default)]
    pub by_role: ByRole,
    /// Where the time went.
    pub timing: Timing,
    /// How many kernel turns it took.
    pub turns: u32,
    /// How many tool calls it made.
    pub tool_calls: u32,
    /// The session it produced, replayable with [`crate::replay`].
    pub transcript: SessionRef,
    /// The grader's explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Surface>,
}

impl EvalResult {
    /// The key a comparison lines two results up by.
    #[must_use]
    pub fn key(&self) -> String {
        match self.seed {
            Some(seed) => format!("{}·{}/{}@{seed}", self.case, self.profile, self.model),
            None => format!("{}·{}/{}", self.case, self.profile, self.model),
        }
    }

    /// The matrix point this result belongs to.
    #[must_use]
    pub fn point(&self) -> MatrixPoint {
        MatrixPoint {
            profile: self.profile.clone(),
            model: self.model.clone(),
            seed: self.seed,
        }
    }
}

/// Everything one `orrery eval run` produced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    /// The run's id, so `eval compare` and `eval replay` can name it.
    pub run_id: String,
    /// The suite that was run.
    pub suite: String,
    /// What was pinned, and what was not.
    pub reproducibility: Reproducibility,
    /// One per case per matrix point, in a deterministic order.
    pub results: Vec<EvalResult>,
}

impl RunReport {
    /// An empty report for a suite.
    #[must_use]
    pub fn new(run_id: impl Into<String>, suite: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            suite: suite.into(),
            reproducibility: Reproducibility::default(),
            results: Vec::new(),
        }
    }

    /// How many cases passed.
    #[must_use]
    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.outcome.is_pass()).count()
    }

    /// Find one result by case and profile.
    #[must_use]
    pub fn result(&self, case: &str, profile: &str) -> Option<&EvalResult> {
        self.results
            .iter()
            .find(|r| r.case == case && r.profile == profile)
    }

    /// Serialise. This is what a baseline file holds.
    ///
    /// # Errors
    ///
    /// Whatever `serde_json` says.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Read one back.
    ///
    /// # Errors
    ///
    /// Whatever `serde_json` says.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// A plain-text table, one line per result.
    ///
    /// Every line that carries a cost carries its provenance with it. That is
    /// the whole reason this renderer exists rather than a `{:?}`.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = format!(
            "run {} · suite {} · {}\n",
            self.run_id,
            self.suite,
            self.reproducibility.describe()
        );
        for r in &self.results {
            out.push_str(&format!(
                "{:<28} {:<10} {:>6} tok  {:>9} µUSD  [{}]\n",
                r.key(),
                r.outcome.tag(),
                r.cost.total_tokens(),
                r.cost.micro_usd.unwrap_or(0),
                r.cost_provenance.label(),
            ));
            for (role, cost) in r.by_role.iter() {
                out.push_str(&format!(
                    "    {:<12} {:>6} tok over {} pass(es)\n",
                    role,
                    cost.usage.total_tokens(),
                    cost.passes,
                ));
            }
            if let Some(judge) = r.judge_cost {
                out.push_str(&format!(
                    "    {:<12} {:>6} tok  (the judge, not the run)\n",
                    "grader",
                    judge.total_tokens(),
                ));
            }
        }
        out
    }
}
