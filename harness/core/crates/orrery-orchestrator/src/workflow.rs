//! The workflow machine: the thing that actually runs the steps.
//!
//! # Nothing here is a free-running `while`
//!
//! There is exactly one loop construct, [`Step::Loop`], it takes a predicate
//! **and** a cap, and the cap is enforced here — in harness code, on a counter
//! this machine owns — rather than by asking a model nicely to stop. A gate
//! that never passes therefore ends the loop at `max_iterations` with a
//! [`CapReached`] recorded, which is `loops::terminates_on_its_own_cap`.
//!
//! # Phases fire in every step
//!
//! Invariant 3. A [`StepObserver`] is registered once on the [`Runner`] and is
//! called for **every** step whatever agent it is bound to, so an interceptor
//! written for the planner also sees the executor. The per-*pass* phase
//! pipeline is the kernel's (plan 05); this is the step-level half of the same
//! invariant, and it is the half this crate owns.

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::future::BoxFuture;
use futures_util::stream::{FuturesUnordered, StreamExt};
use orrery_proto::{Budget, BudgetKind, Usage};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::budget::WorkflowBudget;
use crate::expr::{self, Env};
use crate::join::Join;
use crate::step::{NamedStep, OnFail, Step};
use crate::typecheck::Checked;

/// What one step produced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepOutput {
    /// Its return value, which later steps `ref`.
    pub value: Value,
    /// What it cost. A tool step costs nothing, which is the point of it.
    pub usage: Usage,
}

impl StepOutput {
    /// A value that cost nothing.
    #[must_use]
    pub fn free(value: Value) -> Self {
        Self {
            value,
            usage: Usage::default(),
        }
    }
}

/// A step that did not work.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct StepFailure {
    /// A stable, machine-readable code.
    pub code: String,
    /// What went wrong, in words.
    pub message: String,
}

impl StepFailure {
    /// A failure with a code and a message.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// How a step reaches the rest of the system.
///
/// The orchestrator sequences steps; it does not know how to run a turn or
/// dispatch a tool. The facade implements this over `orrery-kernel` and
/// `orrery-tools`; a test implements it over a counter, which is how
/// `tool_step_makes_no_model_call` can assert that the provider was never
/// invoked.
#[async_trait]
pub trait StepExecutor: Send + Sync {
    /// Run a sub-agent and return what it concluded.
    async fn agent(
        &self,
        subagent: &str,
        input: Value,
        budget: Budget,
    ) -> Result<StepOutput, StepFailure>;

    /// Call a tool. **No model call.**
    async fn tool(&self, r#ref: &str, input: Value) -> Result<StepOutput, StepFailure>;
}

/// Which step is being run, and under whom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepCtx {
    /// Its name.
    pub step: String,
    /// Its kind: `agent`, `tool`, `parallel`, `loop` or `gate`.
    pub kind: &'static str,
    /// The agent it runs under, when it runs under one.
    pub agent: Option<String>,
    /// Which time round, for a step inside a loop body. One-based.
    pub iteration: u32,
}

/// Something that watches every step.
///
/// Registered once, called for all of them. Sync, and handed only data — the
/// same shape as the kernel's interceptor chain, for the same reason: an
/// observer that could do I/O could stall a run.
pub trait StepObserver: Send + Sync {
    /// Before the step runs.
    fn entered(&self, ctx: &StepCtx);

    /// After it has run. The default is nothing.
    fn left(&self, _ctx: &StepCtx) {}
}

/// A loop that ended on its cap rather than on its predicate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapReached {
    /// Which loop.
    pub step: String,
    /// How many times round it went. Equal to its `max_iterations`.
    pub iterations: u32,
}

/// How a run ended.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Every step ran.
    Completed,
    /// A gate failed with `on_fail = "stop"`.
    StoppedByGate {
        /// Which gate.
        step: String,
    },
    /// A gate failed with `on_fail = "escalate"`: the decision goes up to the
    /// router, which is the rung above.
    Escalated {
        /// Which gate.
        step: String,
    },
    /// The **whole-workflow** ceiling was reached.
    StoppedByBudget {
        /// Which ceiling bit.
        kind: BudgetKind,
    },
    /// A step failed.
    Failed {
        /// Which step.
        step: String,
        /// Its code.
        code: String,
        /// Its message.
        message: String,
    },
    /// Somebody cancelled it.
    Cancelled,
}

/// What a run came to, and everything it produced on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowRun {
    /// How it ended.
    pub outcome: Outcome,
    /// What every step returned.
    pub env: Env,
    /// What the whole workflow spent.
    pub usage: Usage,
    /// Every loop that ended on its cap.
    pub caps: Vec<CapReached>,
    /// How many steps were charged.
    pub steps_run: u32,
}

impl WorkflowRun {
    /// The cap a named loop reached, if it reached one.
    #[must_use]
    pub fn cap(&self, step: &str) -> Option<&CapReached> {
        self.caps.iter().find(|c| c.step == step)
    }
}

/// The machine.
pub struct Runner<'a> {
    exec: &'a dyn StepExecutor,
    observers: Vec<Arc<dyn StepObserver>>,
    cancel: CancellationToken,
}

impl<'a> Runner<'a> {
    /// A runner over an executor.
    #[must_use]
    pub fn new(exec: &'a dyn StepExecutor) -> Self {
        Self {
            exec,
            observers: Vec::new(),
            cancel: CancellationToken::new(),
        }
    }

    /// Watch every step with this. Registered **once**, called for all of them.
    #[must_use]
    pub fn observed_by(mut self, observer: Arc<dyn StepObserver>) -> Self {
        self.observers.push(observer);
        self
    }

    /// Stop when this is cancelled.
    #[must_use]
    pub fn cancelled_by(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// Run a whole workflow under one budget.
    ///
    /// The budget is the workflow's own when it declares one, and `override_`
    /// otherwise — a caller that has already sliced a parent's ceiling passes
    /// it here.
    /// It takes a [`Checked`] and not a [`Workflow`](crate::typecheck::Workflow)
    /// on purpose: translation #5 says an invalid workflow fails at load, and
    /// this signature is what makes the compiler say it.
    pub async fn run(&self, workflow: &Checked, budget: Option<Budget>) -> WorkflowRun {
        let limit = budget.unwrap_or(workflow.budget);
        let mut state = State::new(Env::new(), WorkflowBudget::new(limit));
        let flow = self.sequence(&workflow.steps, &mut state, 0).await;
        WorkflowRun {
            outcome: match flow {
                Flow::Continue => Outcome::Completed,
                Flow::Stop(outcome) => outcome,
            },
            usage: state.budget.spent(),
            steps_run: state.budget.steps(),
            env: state.env,
            caps: state.caps,
        }
    }

    fn announce(&self, ctx: &StepCtx) {
        for observer in &self.observers {
            observer.entered(ctx);
        }
    }

    fn finished(&self, ctx: &StepCtx) {
        for observer in &self.observers {
            observer.left(ctx);
        }
    }

    /// Run a list of steps in order. Recursive, so boxed.
    fn sequence<'s>(
        &'s self,
        steps: &'s [NamedStep],
        state: &'s mut State,
        iteration: u32,
    ) -> BoxFuture<'s, Flow> {
        Box::pin(async move {
            let mut previous: Option<&NamedStep> = None;
            let mut index = 0usize;
            while index < steps.len() {
                let step = &steps[index];
                if self.cancel.is_cancelled() {
                    return Flow::Stop(Outcome::Cancelled);
                }
                // The whole-workflow ceiling, checked before the step and not
                // after it.
                if let Err(exceeded) = state.budget.check() {
                    return Flow::Stop(Outcome::StoppedByBudget {
                        kind: exceeded.kind,
                    });
                }

                let ctx = StepCtx {
                    step: step.name.clone(),
                    kind: step.step.kind(),
                    agent: match &step.step {
                        Step::Agent { subagent, .. } => Some(subagent.clone()),
                        _ => None,
                    },
                    iteration,
                };
                self.announce(&ctx);
                let flow = self.one(step, state, iteration).await;
                self.finished(&ctx);

                match flow {
                    Flow::Continue => {}
                    Flow::Stop(Outcome::StoppedByGate { step: gate }) => {
                        // `retry` is the one control flow a gate has: run the
                        // step before it again, once, and ask again.
                        let retryable = matches!(
                            &step.step,
                            Step::Gate {
                                on_fail: OnFail::Retry,
                                ..
                            }
                        );
                        match (retryable, previous) {
                            (true, Some(prev)) if !state.retried.contains(&gate) => {
                                state.retried.push(gate);
                                let flow = self.one(prev, state, iteration).await;
                                if let Flow::Stop(outcome) = flow {
                                    return Flow::Stop(outcome);
                                }
                                continue; // ask the gate again.
                            }
                            _ => {
                                return Flow::Stop(Outcome::StoppedByGate { step: gate });
                            }
                        }
                    }
                    Flow::Stop(outcome) => return Flow::Stop(outcome),
                }

                previous = Some(step);
                index += 1;
            }
            Flow::Continue
        })
    }

    /// Run one step.
    fn one<'s>(
        &'s self,
        step: &'s NamedStep,
        state: &'s mut State,
        _iteration: u32,
    ) -> BoxFuture<'s, Flow> {
        Box::pin(async move {
            match &step.step {
                Step::Agent { subagent, input } => {
                    let input = match expr::eval(input, &state.env) {
                        Ok(value) => value,
                        Err(e) => return Flow::Stop(failed(&step.name, "expr", &e.to_string())),
                    };
                    let slice = state.budget.slice(1);
                    match self.exec.agent(subagent, input, slice).await {
                        Ok(out) => {
                            state.env.bind(step.name.clone(), out.value);
                            charge(state, out.usage)
                        }
                        Err(e) => Flow::Stop(failed(&step.name, &e.code, &e.message)),
                    }
                }
                Step::Tool { r#ref, input } => {
                    let input = match expr::eval(input, &state.env) {
                        Ok(value) => value,
                        Err(e) => return Flow::Stop(failed(&step.name, "expr", &e.to_string())),
                    };
                    match self.exec.tool(r#ref, input).await {
                        Ok(out) => {
                            state.env.bind(step.name.clone(), out.value);
                            charge(state, out.usage)
                        }
                        Err(e) => Flow::Stop(failed(&step.name, &e.code, &e.message)),
                    }
                }
                Step::Gate { check, on_fail } => {
                    let held = expr::holds(check, &state.env).unwrap_or(false);
                    state
                        .env
                        .bind(step.name.clone(), serde_json::json!({ "passed": held }));
                    if held {
                        return Flow::Continue;
                    }
                    match on_fail {
                        OnFail::Stop | OnFail::Retry => Flow::Stop(Outcome::StoppedByGate {
                            step: step.name.clone(),
                        }),
                        OnFail::Escalate => Flow::Stop(Outcome::Escalated {
                            step: step.name.clone(),
                        }),
                    }
                }
                Step::Parallel { steps, join } => self.parallel(step, steps, *join, state).await,
                Step::Loop {
                    body,
                    until,
                    max_iterations,
                } => {
                    self.looping(step, body, until, *max_iterations, state)
                        .await
                }
            }
        })
    }

    /// The loop. **The cap is enforced here.**
    async fn looping(
        &self,
        step: &NamedStep,
        body: &[NamedStep],
        until: &orrery_proto::Predicate,
        max_iterations: u32,
        state: &mut State,
    ) -> Flow {
        let mut iterations = 0u32;
        let mut satisfied = false;
        // `1..=max_iterations` — a bounded range, not a `while`. Whatever the
        // predicate does, this ends.
        for round in 1..=max_iterations {
            iterations = round;
            match self.sequence(body, state, round).await {
                Flow::Continue => {}
                // A gate inside the body ends the round, not the loop: that is
                // what a verify loop is for.
                Flow::Stop(Outcome::StoppedByGate { .. }) => {}
                Flow::Stop(outcome) => return Flow::Stop(outcome),
            }
            if expr::holds(until, &state.env).unwrap_or(false) {
                satisfied = true;
                break;
            }
        }

        if !satisfied {
            state.caps.push(CapReached {
                step: step.name.clone(),
                iterations,
            });
        }
        state.env.bind(
            step.name.clone(),
            serde_json::json!({
                "iterations": iterations,
                "stopped_by": if satisfied { "predicate" } else { "cap" },
            }),
        );
        Flow::Continue
    }

    /// The parallel step, with the join actually cancelling what it does not
    /// need.
    async fn parallel(
        &self,
        step: &NamedStep,
        branches: &[NamedStep],
        join: Join,
        state: &mut State,
    ) -> Flow {
        if branches.is_empty() {
            state.env.bind(step.name.clone(), serde_json::json!({}));
            return Flow::Continue;
        }
        let wanted = join.wanted(branches.len());
        let n = u32::try_from(branches.len()).unwrap_or(u32::MAX);
        let slice = state.budget.slice(n);

        let mut running = FuturesUnordered::new();
        for branch in branches {
            let env = state.env.clone();
            running.push(async move {
                let mut sub = State {
                    env,
                    budget: WorkflowBudget::new(slice),
                    caps: Vec::new(),
                    retried: Vec::new(),
                };
                let flow = self
                    .sequence(std::slice::from_ref(branch), &mut sub, 0)
                    .await;
                (branch.name.clone(), flow, sub)
            });
        }

        let mut taken: Vec<(String, State)> = Vec::new();
        let mut failure: Option<Flow> = None;
        while let Some((name, flow, sub)) = running.next().await {
            match flow {
                Flow::Continue => taken.push((name, sub)),
                Flow::Stop(outcome) => {
                    // Under `all` one failure is the join's failure. Under a
                    // quorum it is one answer that did not arrive.
                    if join == Join::All {
                        return Flow::Stop(outcome);
                    }
                    failure.get_or_insert(Flow::Stop(outcome));
                }
            }
            if taken.len() >= wanted {
                break;
            }
        }
        // Dropping the rest cancels them: a quorum proceeds with `n` and does
        // not pay for the others (open question 3).
        let cancelled = running.len();
        drop(running);

        if taken.len() < wanted {
            return failure.unwrap_or(Flow::Stop(Outcome::Failed {
                step: step.name.clone(),
                code: "join-unsatisfied".to_owned(),
                message: format!(
                    "`{}` needed {wanted} of {} branches and got {}",
                    step.name,
                    branches.len(),
                    taken.len()
                ),
            }));
        }

        // Merge in declaration order, whatever order they finished in.
        let mut merged = serde_json::Map::new();
        for branch in branches {
            let Some((name, sub)) = taken.iter().find(|(n, _)| *n == branch.name) else {
                continue;
            };
            if let Some(value) = sub.env.get(name) {
                merged.insert(name.clone(), value.clone());
                state.env.bind(name.clone(), value.clone());
            }
            state.caps.extend(sub.caps.iter().cloned());
            if let Err(exceeded) = state.budget.charge(sub.budget.spent()) {
                state.env.bind(step.name.clone(), Value::Object(merged));
                return Flow::Stop(Outcome::StoppedByBudget {
                    kind: exceeded.kind,
                });
            }
        }
        merged.insert("cancelled".to_owned(), Value::from(cancelled));
        state.env.bind(step.name.clone(), Value::Object(merged));
        Flow::Continue
    }
}

/// What the run has so far.
struct State {
    env: Env,
    budget: WorkflowBudget,
    caps: Vec<CapReached>,
    retried: Vec<String>,
}

impl State {
    fn new(env: Env, budget: WorkflowBudget) -> Self {
        Self {
            env,
            budget,
            caps: Vec::new(),
            retried: Vec::new(),
        }
    }
}

/// Whether to carry on.
enum Flow {
    Continue,
    Stop(Outcome),
}

fn charge(state: &mut State, usage: Usage) -> Flow {
    match state.budget.charge(usage) {
        Ok(()) => Flow::Continue,
        Err(exceeded) => Flow::Stop(Outcome::StoppedByBudget {
            kind: exceeded.kind,
        }),
    }
}

fn failed(step: &str, code: &str, message: &str) -> Outcome {
    Outcome::Failed {
        step: step.to_owned(),
        code: code.to_owned(),
        message: message.to_owned(),
    }
}
