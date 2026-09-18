//! What a run declares, and the loop that executes it.
//!
//! # Reproducibility is declared, not hoped for
//!
//! A memory provider reading a mutable store and a router bound to a model both
//! make the same case behave differently on two days. So an [`EvalRun`]
//! declares both, the defaults are the reproducible ones — `memory: off`,
//! `router: declared` — and `eval compare` refuses to compare across differing
//! settings without `--force`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use futures_util::StreamExt as _;
use orrery_grader::{CaseRef, EvalOutcome, GradeInput, Grader};
use orrery_proto::{Budget, BudgetKind, ContentBlock, Role, SessionRef, Usage, UserInput};
use orrery_provider::{ModelEvent, ModelRequest, Provider};
use orrery_session::SessionStore;
use orrery_session::algebra::CharsOverFour;
use orrery_session::turn::{NewTurn, TurnKind};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::case::{EvalCase, Suite};
use crate::error::EvalError;
use crate::isolate::Isolator;
use crate::matrix::{Matrix, MatrixPoint};
use crate::report::{ByRole, CostProvenance, EvalResult, RunReport, Timing};
use crate::telemetry::{BoundaryMeter, EstimatorProbe, SharedCounter};

/// How a case's workspace is kept apart from every other case's.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Isolation {
    /// A git worktree per case. What phase 9 ships.
    #[default]
    Worktree,
    /// A container per case. Not implemented; the runner says so by name.
    Container,
}

/// Whether memory is in play, and therefore whether two runs are comparable.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum MemoryMode {
    /// No memory provider. The default, and the reproducible one.
    #[default]
    Off,
    /// A named provider over named scopes. Useful, and labelled as what it is.
    Provider {
        /// Which provider.
        id: String,
        /// Which scopes it may see.
        #[serde(default)]
        scopes: Vec<String>,
    },
}

/// Whether routing is declarative, and therefore whether two runs are
/// comparable.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum RouterMode {
    /// Rules only. The default, and the reproducible one.
    #[default]
    Declared,
    /// An agent decides. Reproducible only as far as that agent is.
    Agent {
        /// Which agent.
        name: String,
    },
}

/// What a run pinned.
///
/// Carried into the report so that a comparison can refuse two runs that do not
/// agree, and so that a person reading a number a year later can see what it
/// was a number *of*.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reproducibility {
    /// The memory mode the run declared.
    #[serde(default)]
    pub memory: MemoryMode,
    /// The router mode the run declared.
    #[serde(default)]
    pub router: RouterMode,
    /// Whether any seed was asked for. A seed is honoured by local models and
    /// is a label for hosted ones; this says only that one was requested.
    #[serde(default)]
    pub seeded: bool,
}

impl Reproducibility {
    /// True when both pluggable parts are in their reproducible default.
    #[must_use]
    pub fn is_pinned(&self) -> bool {
        self.memory == MemoryMode::Off && self.router == RouterMode::Declared
    }

    /// One line for a report header.
    #[must_use]
    pub fn describe(&self) -> String {
        let memory = match &self.memory {
            MemoryMode::Off => "memory off".to_owned(),
            MemoryMode::Provider { id, .. } => format!("memory via {id}"),
        };
        let router = match &self.router {
            RouterMode::Declared => "router declared".to_owned(),
            RouterMode::Agent { name } => format!("router agent {name}"),
        };
        format!("{memory}, {router}")
    }
}

/// One `orrery eval run`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvalRun {
    /// The suite to run.
    pub suite: String,
    /// What to run it against.
    #[serde(default)]
    pub matrix: Matrix,
    /// How many cases at a time.
    #[serde(default = "one")]
    pub concurrency: u32,
    /// How cases are kept apart.
    #[serde(default)]
    pub isolation: Isolation,
    /// Memory. Off unless said otherwise.
    #[serde(default)]
    pub memory: MemoryMode,
    /// Routing. Declarative unless said otherwise.
    #[serde(default)]
    pub router: RouterMode,
}

const fn one() -> u32 {
    1
}

impl EvalRun {
    /// A run of one suite over one matrix, with every default in place.
    #[must_use]
    pub fn new(suite: impl Into<String>, matrix: Matrix) -> Self {
        Self {
            suite: suite.into(),
            matrix,
            concurrency: 1,
            isolation: Isolation::default(),
            memory: MemoryMode::default(),
            router: RouterMode::default(),
        }
    }

    /// Run more than one case at a time.
    #[must_use]
    pub const fn with_concurrency(mut self, concurrency: u32) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// What this run pinned.
    #[must_use]
    pub fn reproducibility(&self) -> Reproducibility {
        Reproducibility {
            memory: self.memory.clone(),
            router: self.router.clone(),
            seeded: !self.matrix.seeds.is_empty(),
        }
    }

    /// Parse a run document.
    ///
    /// # Errors
    ///
    /// [`EvalError::Parse`] naming what TOML disliked.
    pub fn from_toml(text: &str) -> Result<Self, EvalError> {
        toml::from_str(text).map_err(|e| EvalError::Parse {
            what: "run",
            detail: e.to_string(),
        })
    }
}

/// What a case runner is handed.
#[derive(Clone, Copy, Debug)]
pub struct CaseCtx<'a> {
    /// The case.
    pub case: &'a EvalCase,
    /// Which point of the matrix.
    pub point: &'a MatrixPoint,
    /// The private workspace it may change.
    pub workspace: &'a std::path::Path,
}

/// What a case runner produced.
#[derive(Clone, Debug)]
pub struct RunOutput {
    /// Set when a ceiling stopped the case. Not an error: an outcome.
    pub stopped_by: Option<BudgetKind>,
    /// What it cost.
    pub cost: Usage,
    /// Where the tokens went.
    pub by_role: ByRole,
    /// Where the time went.
    pub timing: Timing,
    /// Kernel turns.
    pub turns: u32,
    /// Tool calls.
    pub tool_calls: u32,
    /// The session it wrote.
    pub transcript: SessionRef,
}

/// Something that can run one case: our kernel, or an external agent CLI.
#[async_trait]
pub trait CaseRunner: Send + Sync {
    /// How a report names this runner.
    fn label(&self) -> &str;

    /// Where this runner's cost numbers come from.
    ///
    /// Ours are measured; an adapter's are whatever the tool printed. The
    /// report carries the answer next to every number.
    fn cost_provenance(&self) -> CostProvenance;

    /// Run one case to completion.
    ///
    /// # Errors
    ///
    /// [`EvalError`] when the *runner* failed. A case that failed, or ran out
    /// of budget, is a `RunOutput`, not an error.
    async fn run(&self, ctx: CaseCtx<'_>) -> Result<RunOutput, EvalError>;
}

/// One role's model.
#[derive(Clone)]
pub struct RoleBinding {
    /// The provider to call.
    pub provider: Arc<dyn Provider>,
    /// The model id to ask it for.
    pub model: String,
}

impl std::fmt::Debug for RoleBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoleBinding")
            .field("provider", &self.provider.id())
            .field("model", &self.model)
            .finish()
    }
}

/// The order roles take their pass in.
///
/// Fixed, so that two runs of the same case call the same providers in the same
/// sequence. Only bound roles run.
pub const PASS_ORDER: [Role; 5] = [
    Role::Planner,
    Role::Executor,
    Role::Verifier,
    Role::Compactor,
    Role::Summariser,
];

/// Our own runner: a pass per bound role, with the cost read at the provider
/// boundary.
///
/// The cost path is the whole point. Every event the provider emits goes
/// through [`BoundaryMeter::observe`], which is the only thing in this crate
/// that can put a number into a result, and it only accepts
/// [`ModelEvent::Usage`]. Nothing here counts tokens: the provider's own
/// counter is wrapped in an [`EstimatorProbe`] and never asked, which is what
/// `run::cost_comes_from_telemetry` checks.
pub struct HarnessRunner {
    label: String,
    store: Arc<dyn SessionStore>,
    bindings: HashMap<Role, RoleBinding>,
    probe: Arc<EstimatorProbe<SharedCounter>>,
    max_output_tokens: u64,
}

impl HarnessRunner {
    /// A runner over a session store and at least one role binding.
    ///
    /// # Panics
    ///
    /// When `bindings` is empty: a runner with no model is not a runner.
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        store: Arc<dyn SessionStore>,
        bindings: HashMap<Role, RoleBinding>,
    ) -> Self {
        let first = bindings
            .values()
            .next()
            .expect("a harness runner needs at least one role binding");
        let probe = Arc::new(EstimatorProbe::new(SharedCounter::new(
            first.provider.counter(),
        )));
        Self {
            label: label.into(),
            store,
            bindings,
            probe,
            max_output_tokens: 4096,
        }
    }

    /// The probe standing where a token estimate would have come from.
    ///
    /// It is wired in the provider counter's place and the run never asks it
    /// anything. A test asserts the count is zero, which is how the absence of
    /// an estimation path is proved rather than asserted.
    #[must_use]
    pub fn estimator_probe(&self) -> &EstimatorProbe<SharedCounter> {
        &self.probe
    }

    /// Which roles this runner will take a pass for, in order.
    #[must_use]
    pub fn passes(&self) -> Vec<Role> {
        PASS_ORDER
            .iter()
            .copied()
            .filter(|r| self.bindings.contains_key(r))
            .collect()
    }
}

/// Whether a ceiling has already been reached. Zero means unmetered.
fn exceeded(budget: &Budget, spent: &Usage, turns: u32, started: Instant) -> Option<BudgetKind> {
    if budget.max_turns > 0 && turns >= budget.max_turns {
        return Some(BudgetKind::Turns);
    }
    if budget.max_tokens > 0 && spent.total_tokens() >= budget.max_tokens {
        return Some(BudgetKind::Tokens);
    }
    if let Some(max) = budget.max_micro_usd {
        if spent.micro_usd.unwrap_or(0) >= max {
            return Some(BudgetKind::Usd);
        }
    }
    if budget.wall_clock_ms > 0 && started.elapsed().as_millis() as u64 >= budget.wall_clock_ms {
        return Some(BudgetKind::WallClock);
    }
    None
}

#[async_trait]
impl CaseRunner for HarnessRunner {
    fn label(&self) -> &str {
        &self.label
    }

    fn cost_provenance(&self) -> CostProvenance {
        CostProvenance::MeasuredAtProviderBoundary
    }

    async fn run(&self, ctx: CaseCtx<'_>) -> Result<RunOutput, EvalError> {
        let started = Instant::now();
        let meter = BoundaryMeter::new();

        let session = self
            .store
            .create(&ctx.workspace.display().to_string(), &ctx.point.profile)
            .await?;
        let handle = self.store.open(session).await?;
        let lease = self.store.lease(handle.root).await?;
        self.store
            .append(
                &lease,
                NewTurn::new(TurnKind::User {
                    input: UserInput::text(&ctx.case.prompt),
                }),
            )
            .await?;

        let mut turns = 0u32;
        let mut tool_calls = 0u32;
        let mut model_ms = 0u64;
        let mut stopped_by = None;

        for role in self.passes() {
            let spent = meter.total();
            if let Some(kind) = exceeded(&ctx.case.budget, &spent, turns, started) {
                stopped_by = Some(kind);
                break;
            }
            let binding = &self.bindings[&role];

            // The context the pass sees. `CharsOverFour` fits it to the window;
            // it prices nothing, and nothing downstream reads its number as a
            // cost.
            let view = self
                .store
                .materialise(
                    handle.root,
                    ctx.case.budget.into_token_budget(),
                    &CharsOverFour,
                )
                .await?;

            let model = if ctx.point.model == crate::matrix::DEFAULT_MODEL {
                binding.model.clone()
            } else {
                ctx.point.model.clone()
            };
            let req = ModelRequest::new(
                model,
                Arc::from(view.messages.into_boxed_slice()),
                self.max_output_tokens,
            );

            meter.begin_pass(role);
            let before = meter.total();
            let pass_started = Instant::now();
            let mut stream = binding.provider.stream(req, CancellationToken::new());
            let mut text = String::new();
            while let Some(event) = stream.next().await {
                let event = event.map_err(|e| EvalError::Provider {
                    detail: e.to_string(),
                })?;
                meter.observe(&event);
                match &event {
                    ModelEvent::TextDelta { text: chunk } => text.push_str(chunk),
                    ModelEvent::ToolUseEnd { .. } => tool_calls = tool_calls.saturating_add(1),
                    _ => {}
                }
            }
            meter.end_pass();
            model_ms = model_ms.saturating_add(pass_started.elapsed().as_millis() as u64);

            // The turn's usage is the difference the boundary meter saw across
            // this pass. Still the provider's own number, never a re-count.
            let after = meter.total();
            let usage = Usage {
                input_tokens: after.input_tokens - before.input_tokens,
                output_tokens: after.output_tokens - before.output_tokens,
                cache_hits: after.cache_hits - before.cache_hits,
                micro_usd: match (after.micro_usd, before.micro_usd) {
                    (Some(a), Some(b)) => Some(a.saturating_sub(b)),
                    (Some(a), None) => Some(a),
                    _ => None,
                },
            };

            self.store
                .append(
                    &lease,
                    NewTurn::new(TurnKind::Assistant {
                        content: vec![ContentBlock::Text { text }],
                        usage,
                    }),
                )
                .await?;
            turns = turns.saturating_add(1);
        }

        Ok(RunOutput {
            stopped_by,
            cost: meter.total(),
            by_role: meter.by_role(),
            timing: Timing {
                wall_ms: started.elapsed().as_millis() as u64,
                model_ms,
                tool_ms: 0,
            },
            turns,
            tool_calls,
            transcript: SessionRef {
                session,
                branch: handle.root,
                turn: None,
            },
        })
    }
}

/// A budget's token ceiling, as a context budget.
trait BudgetExt {
    fn into_token_budget(self) -> orrery_proto::TokenBudget;
}

impl BudgetExt for Budget {
    fn into_token_budget(self) -> orrery_proto::TokenBudget {
        orrery_proto::TokenBudget {
            max: if self.max_tokens == 0 {
                u64::MAX
            } else {
                self.max_tokens
            },
            reserve: 0,
        }
    }
}

/// Runs a suite: a runner per profile, a grader per id, one workspace per case.
pub struct EvalRunner {
    runners: BTreeMap<String, Arc<dyn CaseRunner>>,
    graders: BTreeMap<String, Arc<dyn Grader>>,
    isolator: Isolator,
}

impl EvalRunner {
    /// A runner with nothing bound yet.
    #[must_use]
    pub fn new(isolator: Isolator) -> Self {
        Self {
            runners: BTreeMap::new(),
            graders: BTreeMap::new(),
            isolator,
        }
    }

    /// Bind a runner to a profile name. The matrix selects by this name, which
    /// is what lets one point of the matrix be an external CLI.
    #[must_use]
    pub fn with_runner(mut self, profile: impl Into<String>, runner: Arc<dyn CaseRunner>) -> Self {
        self.runners.insert(profile.into(), runner);
        self
    }

    /// Install a grader under its own id.
    #[must_use]
    pub fn with_grader(mut self, grader: Arc<dyn Grader>) -> Self {
        self.graders.insert(grader.id().to_owned(), grader);
        self
    }

    /// Run every case on every point of the matrix.
    ///
    /// Results come back sorted by [`EvalResult::key`], whatever order they
    /// finished in, so a report of a concurrent run diffs against a report of a
    /// serial one.
    ///
    /// # Errors
    ///
    /// [`EvalError`] when the runner itself fails. A failing case is a result.
    pub async fn run(&self, spec: &EvalRun, suite: &Suite) -> Result<RunReport, EvalError> {
        let points = spec.matrix.expand();
        for point in &points {
            if !self.runners.contains_key(&point.profile) {
                return Err(EvalError::NoRunner {
                    profile: point.profile.clone(),
                });
            }
        }
        for case in &suite.cases {
            if !self.graders.contains_key(&case.grade.grader) {
                return Err(EvalError::NoGrader {
                    case: case.id.clone(),
                    grader: case.grade.grader.clone(),
                });
            }
        }

        let jobs: Vec<(&EvalCase, &MatrixPoint)> = points
            .iter()
            .flat_map(|p| suite.cases.iter().map(move |c| (c, p)))
            .collect();

        let concurrency = spec.concurrency.max(1) as usize;
        let mut results: Vec<EvalResult> = futures_util::stream::iter(jobs)
            .map(|(case, point)| self.one(spec, suite, case, point))
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        results.sort_by_key(EvalResult::key);

        Ok(RunReport {
            run_id: format!("run-{}", uuid::Uuid::new_v4().simple()),
            suite: suite.name.clone(),
            reproducibility: spec.reproducibility(),
            results,
        })
    }

    /// One case on one point: isolate, run, grade.
    async fn one(
        &self,
        spec: &EvalRun,
        suite: &Suite,
        case: &EvalCase,
        point: &MatrixPoint,
    ) -> Result<EvalResult, EvalError> {
        let isolator = self.isolator.clone().with_isolation(spec.isolation);
        let workspace = isolator.prepare(case, point)?;
        let runner = &self.runners[&point.profile];
        let started = Instant::now();

        let output = runner
            .run(CaseCtx {
                case,
                point,
                workspace: workspace.path(),
            })
            .await?;

        let (outcome, score, detail, judge_cost) = if output.stopped_by.is_some() {
            // A ceiling is an outcome, not an error, and it is not graded: a
            // half-finished workspace tells a grader nothing true.
            (EvalOutcome::BudgetExceeded, None, None, None)
        } else {
            let grader = &self.graders[&case.grade.grader];
            let graded = grader
                .grade(
                    GradeInput::new(
                        workspace.path(),
                        output.transcript.clone(),
                        CaseRef::new(&suite.name, &case.id),
                    )
                    // The case's own grader block, passed through unread.
                    .with_config(case.grade.config.clone()),
                )
                .await;
            match graded {
                Ok(s) => (s.outcome, s.score, Some(s.detail), s.judge_cost),
                // A grader that could not decide leaves the case in `Error`,
                // which a regression check must not read as the thing under
                // test getting worse.
                Err(e) => (
                    EvalOutcome::Error,
                    None,
                    Some(orrery_proto::Surface::new(
                        orrery_proto::SurfaceKind::Text {
                            value: e.to_string(),
                            style: Some(orrery_proto::TextStyle::Error),
                        },
                    )),
                    None,
                ),
            }
        };

        Ok(EvalResult {
            case: case.id.clone(),
            profile: point.profile.clone(),
            model: point.model.clone(),
            seed: point.seed,
            outcome,
            score,
            cost: output.cost,
            cost_provenance: runner.cost_provenance(),
            judge_cost,
            by_role: output.by_role,
            timing: Timing {
                wall_ms: started.elapsed().as_millis() as u64,
                ..output.timing
            },
            turns: output.turns,
            tool_calls: output.tool_calls,
            transcript: output.transcript,
            detail,
        })
    }
}
