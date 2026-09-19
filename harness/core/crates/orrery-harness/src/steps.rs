//! The orchestrator's two holes, filled over the kernel and the tool registry.
//!
//! # Why this crate and not either of theirs
//!
//! `orrery-orchestrator` runs steps through [`StepExecutor`] and
//! [`TurnRunner`], **which nothing implemented**, and `orrery-router`'s
//! [`Router::decide`] is sync and returns data. That is deliberate: it is the
//! cut that keeps kernel, orchestrator and router acyclic, and it is also why
//! neither crate was in `cargo tree -p orrery-cli` — every criterion in plan 11
//! was true as a library test and unreachable as a product.
//!
//! This crate is the one allowed to name a kernel *and* an orchestrator, so the
//! implementations belong here. [`KernelSteps`] is the whole of it.
//!
//! # A sub-agent runs on its own branch
//!
//! An `agent` step writes the parent's `User` row, forks a child branch at it,
//! runs a turn on the child's own lease, closes the child and then — parent
//! lease in hand — writes the `BranchResult` join. The child's work is
//! therefore ordinary turn rows on a branch of its own, inspectable and
//! replayable, rather than one opaque tool result: plan 11's fourth invariant.
//!
//! **Why this is not `orrery_orchestrator::subagent::spawn`,** which does
//! exactly that order and is tested for it: `spawn` drives its child through
//! [`TurnRunner::run`](orrery_orchestrator::TurnRunner), which takes
//! `&BranchLease`, while [`Kernel::run_turn`] takes the lease **by value** and
//! keeps it for the length of the turn. No implementation of that trait over
//! this kernel is possible until one of the two signatures moves, so the order
//! is written out here against the store directly and the reconciliation is
//! named in plan 11 rather than left as a silent divergence.
//!
//! # The router is asked, and the answer is audited
//!
//! Escalating to a sub-agent is a *request*. [`KernelSteps::agent`] puts it to
//! the [`Router`] with the signal values behind it, and a
//! [`RouteDecision::Deny`] fails the step with the router's own words instead of
//! running the child anyway. A router built from an empty profile grants, so a
//! workflow with no `[routing]` table behaves exactly as it did before this
//! existed.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_kernel::{Kernel, TurnInput, TurnOutcome};
use orrery_orchestrator::{StepExecutor, StepFailure, StepOutput};
use orrery_proto::{
    AgentScope, BranchId, Budget, CallId, SessionId, Subject, ToolRef, TurnId, UserInput, Usage,
};
use orrery_router::{Proposal, RouteDecision, Router, Rung, Signals};
use orrery_session::{BranchOutcome, NewTurn, SessionStore, TurnKind};
use orrery_tools::{CallCtx, Registry};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

/// What the orchestrator runs its steps through.
pub struct KernelSteps {
    kernel: Arc<Kernel>,
    store: Arc<dyn SessionStore>,
    registry: Arc<Registry>,
    session: SessionId,
    branch: BranchId,
    scope: AgentScope,
    router: Router,
    cancel: CancellationToken,
    /// What the run has spent, for the signals a routing rule reads. Held here
    /// because [`Router::decide`] is pure and remembers nothing between calls.
    state: Mutex<RunState>,
}

#[derive(Default)]
struct RunState {
    spent: Usage,
    /// How many agent steps have run, which is `turns_in_mode`.
    turns: u32,
}

impl std::fmt::Debug for KernelSteps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KernelSteps")
            .field("session", &self.session)
            .field("router", &self.router)
            .finish_non_exhaustive()
    }
}

impl KernelSteps {
    /// Everything a step needs to reach the rest of the system.
    #[must_use]
    pub fn new(
        kernel: Arc<Kernel>,
        store: Arc<dyn SessionStore>,
        registry: Arc<Registry>,
        session: SessionId,
        branch: BranchId,
        scope: AgentScope,
        router: Router,
    ) -> Self {
        Self {
            kernel,
            store,
            registry,
            session,
            branch,
            scope,
            router,
            cancel: CancellationToken::new(),
            state: Mutex::new(RunState::default()),
        }
    }

    /// Stop every step when this is cancelled.
    #[must_use]
    pub fn cancelled_by(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// What the run has spent so far.
    #[must_use]
    pub fn spent(&self) -> Usage {
        self.state.lock().spent
    }

    fn signals(&self, limit: Budget) -> Signals {
        let state = self.state.lock();
        Signals {
            budget_spent: state.spent,
            budget_limit: limit,
            turns_in_mode: state.turns,
            ..Signals::default()
        }
    }

    /// Run one turn on `branch`, and report what it said and what it cost.
    async fn turn_on(
        &self,
        branch: BranchId,
        prompt: &str,
        scope: AgentScope,
    ) -> Result<(TurnId, String, Usage), StepFailure> {
        let lease = self
            .store
            .lease(branch)
            .await
            .map_err(|e| StepFailure::new("branch-busy", e.to_string()))?;
        let outcome = self
            .kernel
            .run_turn(
                lease,
                TurnInput::new(self.session, UserInput::text(prompt), scope),
                self.cancel.clone(),
            )
            .await
            .map_err(|e| StepFailure::new("kernel", e.to_string()))?;
        match outcome {
            TurnOutcome::Completed { turn, usage, text } => Ok((turn, text, usage)),
            // Every other ending is the step's failure, with the kernel's own
            // word for it: a step that swallowed a budget ceiling and carried on
            // would be a workflow that cannot be stopped by a budget.
            TurnOutcome::StoppedByBudget { kind, .. } => Err(StepFailure::new(
                "budget",
                format!("the turn stopped on its {kind:?} ceiling"),
            )),
            TurnOutcome::NeedsLogin { reason } => Err(StepFailure::new("needs-login", reason)),
            TurnOutcome::Cancelled { .. } => {
                Err(StepFailure::new("cancelled", "the turn was cancelled"))
            }
            TurnOutcome::Failed { code, message, .. } => Err(StepFailure::new(code, message)),
            other => Err(StepFailure::new(
                "unknown-outcome",
                format!("this build does not know how to report {other:?}"),
            )),
        }
    }
}

#[async_trait]
impl StepExecutor for KernelSteps {
    async fn agent(
        &self,
        subagent: &str,
        input: serde_json::Value,
        budget: Budget,
    ) -> Result<StepOutput, StepFailure> {
        // 1 · Ask. Climbing to a sub-agent is a request, checked against the
        //     declared rules and audited with the numbers behind it — the
        //     router holds an `Audit`, never an `Option<Audit>`, so this is
        //     recorded whether or not anybody wired a sink.
        let decided = self.router.decide_explained(
            &self.signals(budget),
            &self.scope,
            Some(Proposal::model(Rung::SubAgent).with_agent(subagent)),
        );
        if let RouteDecision::Deny { reason } = &decided.decision {
            return Err(StepFailure::new("routed-deny", reason.clone()));
        }

        // 2 · The parent's row, which is also the point the child forks at.
        //     `store.append` is what mints a real `TurnId`: the kernel's own
        //     turn id names its loop, not a row, so forking at it is
        //     `no such turn`.
        let prompt = match &input {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let parent = self
            .store
            .lease(self.branch)
            .await
            .map_err(|e| StepFailure::new("branch-busy", e.to_string()))?;
        let at = self
            .store
            .append(
                &parent,
                NewTurn::new(TurnKind::User {
                    input: UserInput::text(prompt.clone()),
                }),
            )
            .await
            .map_err(|e| StepFailure::new("append", e.to_string()))?;

        // 3 · Fork. Takes no lease, because forking is a read of the parent —
        //     which is what lets the parent hold its own lease throughout.
        let child = self
            .store
            .branch(at, subagent)
            .await
            .map_err(|e| StepFailure::new("branch", e.to_string()))?;
        let mut scope = self.scope.clone();
        scope.agent = subagent.to_owned();
        scope.branch = child;

        // 4 · The child's turns, on the child's branch.
        let ran = self.turn_on(child, &prompt, scope).await;

        // 5 · Close the child, and 6 · write the join under the parent's lease.
        //     Both happen whether the child worked or not, so a failure leaves
        //     the tree in the same shape a success does.
        let outcome = match &ran {
            Ok((_, text, _)) => BranchOutcome::Completed {
                summary: text.clone(),
            },
            Err(failure) => BranchOutcome::Failed {
                code: failure.code.clone(),
                message: failure.message.clone(),
            },
        };
        if let Ok(lease) = self.store.lease(child).await {
            let _ = self.store.close_branch(lease, outcome.clone()).await;
        }
        self.store
            .append(
                &parent,
                NewTurn::new(TurnKind::BranchResult { child, outcome }),
            )
            .await
            .map_err(|e| StepFailure::new("join", e.to_string()))?;
        drop(parent);

        let (_, text, usage) = ran?;
        {
            let mut state = self.state.lock();
            state.spent += usage;
            state.turns += 1;
        }
        Ok(StepOutput {
            value: serde_json::Value::String(text),
            usage,
        })
    }

    async fn tool(
        &self,
        r#ref: &str,
        input: serde_json::Value,
    ) -> Result<StepOutput, StepFailure> {
        let parsed: ToolRef = r#ref
            .parse()
            .map_err(|e| StepFailure::new("no-such-tool", format!("`{ref}`: {e}", ref = r#ref)))?;
        let ctx = CallCtx::new(
            CallId::new(),
            Subject::Agent,
            self.scope.clone(),
            crate::default_tool_budget(),
        );
        let outcome = self
            .registry
            .dispatch(&parsed, input, ctx)
            .await
            .map_err(|e| StepFailure::new("dispatch", e.to_string()))?;
        // **No model call, and therefore no usage.** `StepOutput::free` is the
        // whole reason a `tool` step exists: it is where the cost comes out.
        match outcome {
            orrery_proto::Outcome::Ok { value, surface } => Ok(StepOutput::free(
                value
                    .or_else(|| surface.and_then(|s| serde_json::to_value(s).ok()))
                    .unwrap_or(serde_json::Value::Null),
            )),
            orrery_proto::Outcome::Truncated { surface, .. } => Ok(StepOutput::free(
                surface
                    .and_then(|s| serde_json::to_value(s).ok())
                    .unwrap_or(serde_json::Value::Null),
            )),
            orrery_proto::Outcome::Denied { reason, .. } => {
                Err(StepFailure::new("denied", reason))
            }
            orrery_proto::Outcome::Cancelled { .. } => {
                Err(StepFailure::new("cancelled", "the call was cancelled"))
            }
            orrery_proto::Outcome::Unloaded { ext } => Err(StepFailure::new(
                "unloaded",
                format!("`{ext}` is no longer loaded"),
            )),
            orrery_proto::Outcome::Failed { code, message } => {
                Err(StepFailure::new(code, message))
            }
            other => Err(StepFailure::new(
                "unknown-outcome",
                format!("this build does not know how to report {other:?}"),
            )),
        }
    }
}
