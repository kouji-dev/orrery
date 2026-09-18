//! One loop, ten phases, two exits.
//!
//! ```text
//! auth ──> turn.start ──> [ context.build ──> provider.before ──> stream
//!                           ──> provider.after ──> tools? ] ──> turn.end
//! ```
//!
//! # The two exits, and the second one is the kernel's
//!
//! A turn ends because the model stopped asking for tools, or because a ceiling
//! was reached. Both are [`TurnOutcome`] values. Neither is an error, because an
//! error would have to be rendered by whoever caught it and then two layers
//! would describe the same stop in two different ways.
//!
//! # The pass is appended before the next one begins
//!
//! Step 7 of §4.6, and the reason a killed harness resumes instead of
//! restarting: the assistant turn and every tool result are in the tree before
//! the loop goes round again.

use std::sync::Arc;

use futures_util::StreamExt as _;
use orrery_audit::{Audit, AuditEvent, CallOutcome};
use orrery_provider::{
    CompletedToolCall, ModelEvent, ModelRequest, Provider, ProviderError, StopReason,
    ToolCallAccumulator,
};
use orrery_proto::{
    AgentScope, Budget, BudgetKind, CallId, CancelReason, ContentBlock, ExtId, Message,
    MessageRole, Outcome, Seq, SessionId, Subject, TokenBudget, ToolRef, TurnId, Usage, UserInput,
};
use orrery_session::{BranchLease, NewTurn, SessionStore, TurnKind};
use orrery_tools::{CallCtx, Registry, Resolution, ToolBudget};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::budget::{PriceTable, TurnBudget};
use crate::context::{
    CompactPlan, Compacted, Compactor, ContextDraft, MemoryRecall, NoCompactor, NoMemory, Section,
    input_message, recalled_message, tools_section,
};
use crate::error::KernelError;
use crate::intercept::{ChainOutcome, InterceptCtx, InterceptorSet, MatchCtx};
use crate::lifecycle::{LifecycleCtx, LifecyclePoint, LifecycleSet};
use crate::phase::{
    ContextBuild, ContextCompact, ProviderAfter, ProviderBefore, ToolAfter, ToolBefore,
    ToolResolve, TurnEnd, TurnStart,
};
use crate::retry::RetryPolicy;

/// Which pass of which turn.
///
/// Open question 3, decided: **`(TurnId, u32)`, not an opaque id.** The audit
/// wants to group by pass and a plain index sorts naturally, prints readably
/// (`…-4f2a#2`) and needs no generator. An opaque id would buy uniqueness
/// across turns, which the `TurnId` half already provides.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PassId {
    /// The turn this pass belongs to.
    pub turn: TurnId,
    /// Which pass, one-based.
    pub index: u32,
}

impl std::fmt::Display for PassId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.turn, self.index)
    }
}

/// What a session resolved to, for an interceptor at `session.start`.
///
/// TODO(plan-10): this is the shape the kernel needs, not the whole of what
/// plan 10's `ResolvedConfig` will hold. It is here rather than borrowed from
/// `orrery-ext-api` because the kernel does not depend on the extension API and
/// should not have to.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ResolvedManifest {
    /// The profile the session was created from.
    pub profile: String,
    /// The workspace root.
    pub workspace: String,
    /// Which extensions are loaded, by id.
    pub extensions: Vec<String>,
    /// Which model the profile binds.
    pub model: String,
}

/// What a turn was asked to do.
#[derive(Clone, Debug, PartialEq)]
pub struct TurnInput {
    /// Which session.
    pub session: SessionId,
    /// What was submitted.
    pub input: UserInput,
    /// Who is acting, and what they may see.
    pub scope: AgentScope,
    /// Whose permissions apply.
    pub subject: Subject,
}

impl TurnInput {
    /// A turn submitted by the main agent.
    #[must_use]
    pub fn new(session: SessionId, input: UserInput, scope: AgentScope) -> Self {
        Self {
            session,
            input,
            scope,
            subject: Subject::Agent,
        }
    }

    /// A turn submitted by a named sub-agent.
    #[must_use]
    pub fn as_sub_agent(mut self) -> Self {
        self.subject = Subject::SubAgent(self.scope.agent.clone());
        self
    }
}

/// What a turn came to, for an interceptor at `turn.end`.
#[derive(Clone, Debug, PartialEq)]
pub struct TurnSummary {
    /// Which turn.
    pub turn: TurnId,
    /// How many passes it took.
    pub passes: u32,
    /// What it cost.
    pub usage: Usage,
    /// The assistant's last word.
    pub text: String,
    /// How many tools it called.
    pub tool_calls: u32,
}

/// What one pass produced.
#[derive(Clone, Debug, PartialEq)]
pub struct PassResult {
    /// Which pass.
    pub pass: PassId,
    /// Everything the model said, accumulated.
    pub text: String,
    /// Everything it thought, when the provider exposed it.
    pub thinking: String,
    /// What it wants run.
    pub calls: Vec<CompletedToolCall>,
    /// What the pass cost.
    pub usage: Usage,
    /// Why the stream ended.
    pub stop: Option<StopReason>,
    /// How many attempts it took, retries included.
    pub attempts: u32,
}

impl PassResult {
    /// The assistant turn this pass appends.
    #[must_use]
    pub fn content(&self) -> Vec<ContentBlock> {
        let mut content = Vec::new();
        if !self.thinking.is_empty() {
            content.push(ContentBlock::Thinking {
                text: self.thinking.clone(),
            });
        }
        if !self.text.is_empty() {
            content.push(ContentBlock::Text {
                text: self.text.clone(),
            });
        }
        for call in &self.calls {
            content.push(ContentBlock::ToolUse {
                call: call.call,
                name: call.name.clone(),
                input: call.input.clone(),
            });
        }
        content
    }
}

/// A name the model emitted, before it is a tool.
///
/// The kernel's, not [`orrery_policy::PendingCall`]: this is what
/// `tool.resolve` may rewrite — the *name* — and the policy engine's is what a
/// rule matches. Keeping them apart is what lets the kernel not depend on the
/// policy crate at all.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingCall {
    /// The call.
    pub call: CallId,
    /// The name as the model wrote it.
    pub name: String,
    /// The arguments as the model wrote them.
    pub input: serde_json::Value,
}

/// A resolved call, before the policy check.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolInput {
    /// The call.
    pub call: CallId,
    /// Which tool it resolved to.
    pub r#ref: ToolRef,
    /// The arguments, which an interceptor may still rewrite.
    pub input: serde_json::Value,
}

/// How a turn came out.
///
/// Four of the five are ordinary endings. None of them is an `Err`.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum TurnOutcome {
    /// The model stopped asking for tools.
    Completed {
        /// Which turn.
        turn: TurnId,
        /// What it cost.
        usage: Usage,
        /// The assistant's last word.
        text: String,
    },
    /// A ceiling was reached.
    StoppedByBudget {
        /// Which turn.
        turn: TurnId,
        /// Which ceiling.
        kind: BudgetKind,
        /// What it cost before it stopped.
        usage: Usage,
    },
    /// Somebody, or something, stopped it.
    Cancelled {
        /// Which turn.
        turn: TurnId,
        /// Why.
        reason: CancelReason,
        /// What it cost before it stopped.
        usage: Usage,
    },
    /// The provider needs a credential it has not got.
    ///
    /// Returned **before any context is built and before anything is
    /// appended**: a login prompt belongs at the start of a turn, not stalled
    /// halfway through one.
    NeedsLogin {
        /// What to tell the person.
        reason: String,
    },
    /// The provider could not be made to answer.
    ///
    /// Retry exhaustion lands here, and so does a terminal provider error. A
    /// value rather than an `Err` for the same reason as everything else: the
    /// partial turn is already in the tree and the client renders one shape.
    Failed {
        /// Which turn.
        turn: TurnId,
        /// A stable, machine-readable code.
        code: String,
        /// What went wrong, in words.
        message: String,
        /// What it cost before it failed.
        usage: Usage,
    },
}

impl TurnOutcome {
    /// What the turn cost, whatever it came to.
    #[must_use]
    pub fn usage(&self) -> Usage {
        match self {
            TurnOutcome::Completed { usage, .. }
            | TurnOutcome::StoppedByBudget { usage, .. }
            | TurnOutcome::Cancelled { usage, .. }
            | TurnOutcome::Failed { usage, .. } => *usage,
            TurnOutcome::NeedsLogin { .. } => Usage::default(),
        }
    }
}

/// One cancellation tree per turn: a token per pass, a token per call, and one
/// place that remembers *why*.
///
/// The reason matters because a client renders "you stopped it" and "it ran out
/// of money" differently, and by the time the loop unwinds, a bare
/// [`CancellationToken`] can no longer tell them apart.
#[derive(Clone, Debug)]
pub struct TurnCancel {
    token: CancellationToken,
    reason: Arc<Mutex<Option<CancelReason>>>,
}

impl TurnCancel {
    /// A tree rooted at the token the caller holds.
    #[must_use]
    pub fn new(token: CancellationToken) -> Self {
        Self {
            token,
            reason: Arc::new(Mutex::new(None)),
        }
    }

    /// A child token that dies with its parent and shares its reason.
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            token: self.token.child_token(),
            reason: Arc::clone(&self.reason),
        }
    }

    /// Stop, and say why.
    pub fn cancel(&self, reason: CancelReason) {
        self.reason.lock().get_or_insert(reason);
        self.token.cancel();
    }

    /// The token itself, for whatever wants to `select!` on it.
    #[must_use]
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Whether this has been stopped.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Why it stopped.
    ///
    /// [`CancelReason::User`] when nothing said otherwise, because the only way
    /// to cancel a turn without going through [`TurnCancel::cancel`] is for
    /// whoever holds the root token to fire it.
    #[must_use]
    pub fn reason(&self) -> CancelReason {
        self.reason.lock().unwrap_or(CancelReason::User)
    }
}

/// Somewhere to say "this call is over, take back what it was given".
///
/// A typed hole so the kernel does not depend on `orrery-policy`: the harness
/// wires it to the [`TokenLedger`](orrery_policy::TokenLedger) the broker
/// redeems against, and a cancelled tool's next broker call then fails
/// `Revoked` rather than succeeding on a grant nobody wants any more.
pub trait CallRevoker: Send + Sync {
    /// Revoke everything minted for this call.
    fn revoke(&self, call: CallId);
}

/// The revoker of a kernel that has not been given one.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoRevoker;

impl CallRevoker for NoRevoker {
    fn revoke(&self, _call: CallId) {}
}

/// Everything the loop needs that is not a subsystem.
#[derive(Clone, Debug)]
pub struct KernelConfig {
    /// The model id, in the provider's own vocabulary.
    pub model: String,
    /// Section 1 of the system prompt.
    pub system_prompt: String,
    /// Section 2: the role binding, when there is one.
    pub agent_prompt: Option<String>,
    /// Section 4: the active skills, already rendered.
    pub skills: Vec<String>,
    /// What a turn may spend.
    pub budget: Budget,
    /// What one tool call may spend.
    pub tool_budget: ToolBudget,
    /// The output ceiling for one pass.
    pub max_output_tokens: u64,
    /// Sampling temperature, when the caller wants one.
    pub temperature: Option<f32>,
    /// Stop sequences.
    pub stop: Vec<String>,
    /// How hard to try when the provider says "later".
    pub retry: RetryPolicy,
    /// How many times to compact before giving up.
    ///
    /// Open question 2, decided: **2**. One is not enough — the first attempt
    /// summarises the oldest half and a turn whose *recent* history is what
    /// overflows still will not fit. Three has never helped in practice: if two
    /// summaries have not made room, what does not fit is the newest turn plus
    /// the prompt, and a third pass over the same material costs another model
    /// call to discover that again. Stopping with `Tokens` is the honest
    /// answer, and it is a value the client can act on.
    pub compaction_attempts: u32,
    /// What to keep free under the context window for the model's own output.
    pub context_reserve_tokens: u64,
    /// What a model's tokens cost. Empty until plan 10.
    pub prices: PriceTable,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            model: "fixture".to_owned(),
            system_prompt: "You are Orrery, a coding harness.".to_owned(),
            agent_prompt: None,
            skills: Vec::new(),
            budget: Budget {
                max_turns: 16,
                max_tokens: 0,
                wall_clock_ms: 0,
                max_micro_usd: None,
            },
            tool_budget: ToolBudget::new(30_000, 1 << 20),
            max_output_tokens: 4_096,
            temperature: None,
            stop: Vec::new(),
            retry: RetryPolicy::default(),
            compaction_attempts: 2,
            context_reserve_tokens: 4_096,
            prices: PriceTable::empty(),
        }
    }
}

/// The loop, and everything it runs against.
///
/// **Not generic** over [`Provider`] or [`SessionStore`]: both are `Arc<dyn>`.
/// A kernel generic over its provider would be a different type per provider,
/// and every crate holding one would become generic too — for no benefit, since
/// the call is a network round trip either way.
pub struct Kernel {
    store: Arc<dyn SessionStore>,
    provider: Arc<dyn Provider>,
    registry: Arc<Registry>,
    interceptors: Arc<InterceptorSet>,
    lifecycle: Arc<LifecycleSet>,
    compactor: Arc<dyn Compactor>,
    memory: Arc<dyn MemoryRecall>,
    revoker: Arc<dyn CallRevoker>,
    audit: Audit,
    config: KernelConfig,
}

impl std::fmt::Debug for Kernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kernel")
            .field("provider", &self.provider.id())
            .field("model", &self.config.model)
            .field("tools", &self.registry.len())
            .field("interceptors", &self.interceptors)
            .finish_non_exhaustive()
    }
}

impl Kernel {
    /// A kernel over a store, a provider and a tool registry.
    #[must_use]
    pub fn new(
        store: Arc<dyn SessionStore>,
        provider: Arc<dyn Provider>,
        registry: Arc<Registry>,
        config: KernelConfig,
    ) -> Self {
        Self {
            store,
            provider,
            registry,
            interceptors: Arc::new(InterceptorSet::new()),
            lifecycle: Arc::new(LifecycleSet::new()),
            compactor: Arc::new(NoCompactor),
            memory: Arc::new(NoMemory),
            revoker: Arc::new(NoRevoker),
            audit: orrery_audit::null(),
            config,
        }
    }

    /// Use this interceptor set.
    #[must_use]
    pub fn with_interceptors(mut self, set: Arc<InterceptorSet>) -> Self {
        self.interceptors = set;
        self
    }

    /// Use these lifecycle handlers.
    #[must_use]
    pub fn with_lifecycle(mut self, set: Arc<LifecycleSet>) -> Self {
        self.lifecycle = set;
        self
    }

    /// Compact with this, when the context does not fit.
    #[must_use]
    pub fn with_compactor(mut self, compactor: Arc<dyn Compactor>) -> Self {
        self.compactor = compactor;
        self
    }

    /// Recall from this at `context.build`.
    #[must_use]
    pub fn with_memory(mut self, memory: Arc<dyn MemoryRecall>) -> Self {
        self.memory = memory;
        self
    }

    /// Take back a cancelled call's capabilities through this.
    #[must_use]
    pub fn with_revoker(mut self, revoker: Arc<dyn CallRevoker>) -> Self {
        self.revoker = revoker;
        self
    }

    /// Record what happens in this stream.
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = audit;
        self
    }

    /// The store, for a caller that wants to read the tree back.
    #[must_use]
    pub fn store(&self) -> &Arc<dyn SessionStore> {
        &self.store
    }

    /// The tool registry.
    #[must_use]
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// The configuration this kernel was built with.
    #[must_use]
    pub fn config(&self) -> &KernelConfig {
        &self.config
    }

    /// Run one turn to its end.
    ///
    /// # Errors
    ///
    /// [`KernelError`] only when the harness itself broke — the store would not
    /// answer, the registry could not carry a call. A refusal, a ceiling, a
    /// cancellation and a login prompt are all [`TurnOutcome`] values.
    pub async fn run_turn(
        &self,
        lease: BranchLease,
        input: TurnInput,
        cancel: CancellationToken,
    ) -> Result<TurnOutcome, KernelError> {
        let cancel = TurnCancel::new(cancel);
        let turn = TurnId::new();
        let branch = lease.branch();

        // Task 9. The `provider.before` gate's auth check, hoisted to the top of
        // the turn: the answer cannot change between here and there, and asking
        // for a login *after* appending a turn and building a context means a
        // client has to unwind both.
        if let Some(reason) = self.needs_login().await {
            return Ok(TurnOutcome::NeedsLogin { reason });
        }

        let mut budget = TurnBudget::new(self.config.budget)
            .priced(self.config.model.clone(), self.config.prices.clone());

        // 1 · turn.start.
        let pass = PassId { turn, index: 0 };
        let (input, chain) = self.interceptors.run::<TurnStart>(
            &InterceptCtx {
                session: input.session,
                turn,
                pass,
                agent: &input.scope.clone(),
                budget_spent: &Usage::default(),
            },
            &MatchCtx::for_agent(input.scope.agent.clone()),
            input,
        );
        if let ChainOutcome::Denied { reason } = chain {
            return Ok(TurnOutcome::Failed {
                turn,
                code: "turn-denied".to_owned(),
                message: reason,
                usage: Usage::default(),
            });
        }

        self.store
            .append(
                &lease,
                NewTurn::new(TurnKind::User {
                    input: input.input.clone(),
                }),
            )
            .await?;

        let mut state = TurnState {
            turn,
            passes: 0,
            text: String::new(),
            tool_calls: 0,
        };

        let outcome = self
            .loop_until_done(&lease, &input, &mut budget, &mut state, &cancel)
            .await?;

        // turn.end: a verdict on the summary, then the handlers that may not
        // change it.
        let summary = TurnSummary {
            turn,
            passes: state.passes,
            usage: *budget.spent(),
            text: state.text.clone(),
            tool_calls: state.tool_calls,
        };
        let (_summary, _chain) = self.interceptors.run::<TurnEnd>(
            &InterceptCtx {
                session: input.session,
                turn,
                pass: PassId {
                    turn,
                    index: state.passes,
                },
                agent: &input.scope,
                budget_spent: budget.spent(),
            },
            &MatchCtx::for_agent(input.scope.agent.clone()),
            summary,
        );
        self.lifecycle
            .fire(
                LifecyclePoint::TurnEnd,
                &LifecycleCtx {
                    session: input.session,
                    branch,
                    turn: Some(turn),
                    agent: input.scope.clone(),
                    usage: *budget.spent(),
                },
            )
            .await;

        Ok(outcome)
    }

    /// Whether the provider is in a state that a turn cannot start from.
    async fn needs_login(&self) -> Option<String> {
        match self.provider.auth().state().await {
            Ok(orrery_provider::AuthState::NeedsLogin { reason }) => Some(reason),
            Ok(orrery_provider::AuthState::Expired) => {
                Some(format!("the credential for `{}` has expired", self.provider.id()))
            }
            Ok(_) => None,
            // The broker could not be reached. That is not "signed out", and
            // reporting it as one would send a person to a login flow that will
            // not help; let the pass fail with what actually happened.
            Err(e) => {
                tracing::warn!(
                    target: "orrery.kernel.auth",
                    provider = self.provider.id(),
                    error = %e,
                    "could not read the provider's auth state"
                );
                None
            }
        }
    }

    /// Steps 2 to 8, until something stops them.
    async fn loop_until_done(
        &self,
        lease: &BranchLease,
        input: &TurnInput,
        budget: &mut TurnBudget,
        state: &mut TurnState,
        cancel: &TurnCancel,
    ) -> Result<TurnOutcome, KernelError> {
        loop {
            if cancel.is_cancelled() {
                return Ok(TurnOutcome::Cancelled {
                    turn: state.turn,
                    reason: cancel.reason(),
                    usage: *budget.spent(),
                });
            }
            if let Some(kind) = budget.check() {
                return Ok(self.stopped(state.turn, kind, budget));
            }

            budget.begin_pass();
            state.passes = state.passes.saturating_add(1);
            let pass = PassId {
                turn: state.turn,
                index: state.passes,
            };
            let pass_cancel = cancel.child();

            // 2 · context.build, with compaction if it does not fit.
            let draft = match self.build_context(lease, input, pass, budget, &pass_cancel).await? {
                Ok(draft) => draft,
                Err(kind) => return Ok(self.stopped(state.turn, kind, budget)),
            };

            // 3 · provider.before.
            let request = self.request_from(&draft);
            let (request, chain) = self.interceptors.run::<ProviderBefore>(
                &self.ctx(input, pass, budget),
                &MatchCtx::for_agent(input.scope.agent.clone()),
                request,
            );
            if let ChainOutcome::Denied { reason } = chain {
                return Ok(TurnOutcome::Failed {
                    turn: state.turn,
                    code: "request-denied".to_owned(),
                    message: reason,
                    usage: *budget.spent(),
                });
            }

            // 4 · the stream, with retry.
            let result = match self.stream_pass(request, pass, budget, &pass_cancel).await {
                Ok(result) => result,
                Err(PassFailure::Cancelled) => {
                    // The partial text is still worth keeping: it is what the
                    // person saw on their screen.
                    self.append_partial(lease, state).await?;
                    return Ok(TurnOutcome::Cancelled {
                        turn: state.turn,
                        reason: cancel.reason(),
                        usage: *budget.spent(),
                    });
                }
                Err(PassFailure::Provider { error, attempts }) => {
                    self.audit.append(AuditEvent::Content {
                        action: "provider.failed".to_owned(),
                        content: orrery_audit::ContentRef::new(
                            pass.to_string(),
                            error.code().to_owned(),
                            &format!("{error} after {attempts} attempt(s)"),
                        ),
                    });
                    return Ok(TurnOutcome::Failed {
                        turn: state.turn,
                        code: error.code().to_owned(),
                        message: format!("{error} (after {attempts} attempt(s))"),
                        usage: *budget.spent(),
                    });
                }
            };
            budget.charge(&result.usage);
            self.audit.append(AuditEvent::ModelRequest {
                model: self.config.model.clone(),
                input_tokens: result.usage.input_tokens,
                output_tokens: result.usage.output_tokens,
            });

            // 5 · provider.after.
            let (result, chain) = self.interceptors.run::<ProviderAfter>(
                &self.ctx(input, pass, budget),
                &MatchCtx::for_agent(input.scope.agent.clone()),
                result,
            );
            if let ChainOutcome::Denied { reason } = chain {
                return Ok(TurnOutcome::Failed {
                    turn: state.turn,
                    code: "response-denied".to_owned(),
                    message: reason,
                    usage: *budget.spent(),
                });
            }

            if !result.text.is_empty() {
                state.text = result.text.clone();
            }

            // 7 · append the pass before the next one begins.
            let content = result.content();
            if !content.is_empty() {
                self.store
                    .append(
                        lease,
                        NewTurn::new(TurnKind::Assistant {
                            content,
                            usage: result.usage,
                        }),
                    )
                    .await?;
            }

            // 6 · text only ends the turn; tool calls go round again.
            if result.calls.is_empty() {
                return Ok(TurnOutcome::Completed {
                    turn: state.turn,
                    usage: *budget.spent(),
                    text: state.text.clone(),
                });
            }

            for call in &result.calls {
                state.tool_calls = state.tool_calls.saturating_add(1);
                if let Some(stop) = self
                    .run_one_call(lease, input, pass, budget, call, cancel, &pass_cancel)
                    .await?
                {
                    return Ok(stop);
                }
            }
        }
    }

    /// One tool call, start to appended.
    ///
    /// Returns `Some` when the call ended the turn — a ceiling, or a
    /// cancellation — and `None` when the loop should carry on.
    #[allow(clippy::too_many_arguments)]
    async fn run_one_call(
        &self,
        lease: &BranchLease,
        input: &TurnInput,
        pass: PassId,
        budget: &mut TurnBudget,
        call: &CompletedToolCall,
        cancel: &TurnCancel,
        pass_cancel: &TurnCancel,
    ) -> Result<Option<TurnOutcome>, KernelError> {
        let ctx = self.ctx(input, pass, budget);
        let matching = MatchCtx::for_tool(input.scope.agent.clone(), call.name.clone());

        // tool.resolve.
        let pending = PendingCall {
            call: call.call,
            name: call.name.clone(),
            input: call.input.clone(),
        };
        let (pending, chain) = self
            .interceptors
            .run::<ToolResolve>(&ctx, &matching, pending);
        if let Some(outcome) = final_outcome(chain) {
            self.append_result(lease, call.call, unresolved(&pending.name), outcome)
                .await?;
            return Ok(None);
        }

        // The registry resolves the name. `Unknown` is a value the model can act
        // on — it wrote the name — so it settles rather than raising.
        let resolved = match self.registry.resolve(&pending.name, &input.scope) {
            Resolution::Ok { r#ref } => r#ref,
            Resolution::Ambiguous { chose, .. } => chose,
            Resolution::Unknown { name, did_you_mean } => {
                let message = if did_you_mean.is_empty() {
                    format!("there is no tool called `{name}` in this agent's tool set")
                } else {
                    format!(
                        "there is no tool called `{name}`; did you mean {}?",
                        did_you_mean.join(", ")
                    )
                };
                self.append_result(
                    lease,
                    call.call,
                    unresolved(&name),
                    Outcome::Failed {
                        code: "no-such-tool".to_owned(),
                        message,
                    },
                )
                .await?;
                return Ok(None);
            }
            // `Resolution` is `#[non_exhaustive]`. A variant this crate has not
            // been taught about is not a licence to dispatch on a guess: the
            // call settles, and the model is told the name did not resolve.
            other => {
                self.append_result(
                    lease,
                    call.call,
                    unresolved(&pending.name),
                    Outcome::Failed {
                        code: "unresolvable".to_owned(),
                        message: format!(
                            "`{}` resolved to something this kernel does not understand ({other:?})",
                            pending.name
                        ),
                    },
                )
                .await?;
                return Ok(None);
            }
        };

        // tool.before. A `Deny` here narrows: the chain runs before the policy
        // check and nothing re-checks after it, so an interceptor can refuse
        // what policy allowed and can never allow what policy refused.
        let tool_input = ToolInput {
            call: call.call,
            r#ref: resolved.clone(),
            input: pending.input,
        };
        let (tool_input, chain) = self
            .interceptors
            .run::<ToolBefore>(&ctx, &matching, tool_input);
        if let Some(outcome) = final_outcome(chain) {
            let outcome = self.after(&ctx, &matching, outcome);
            self.append_result(lease, call.call, resolved, outcome).await?;
            return Ok(None);
        }

        // A ceiling before the dispatch, not after it.
        if let Some(kind) = budget.check() {
            self.append_result(
                lease,
                call.call,
                resolved,
                Outcome::Cancelled {
                    reason: CancelReason::Budget,
                },
            )
            .await?;
            return Ok(Some(self.stopped(pass.turn, kind, budget)));
        }

        let call_cancel = pass_cancel.child();
        let call_ctx = CallCtx::new(
            call.call,
            input.subject.clone(),
            input.scope.clone(),
            self.config.tool_budget,
        );
        let outcome = tokio::select! {
            biased;
            () = call_cancel.token().cancelled() => {
                // Take back what the call was given. An in-flight tool's next
                // broker call then fails `Revoked` instead of succeeding on a
                // grant nobody wants any more.
                self.revoker.revoke(call.call);
                Outcome::Cancelled { reason: cancel.reason() }
            }
            dispatched = self.registry.dispatch(&tool_input.r#ref, tool_input.input.clone(), call_ctx) => {
                dispatched?
            }
        };

        self.audit.append(AuditEvent::tool_call(
            call.call,
            tool_input.r#ref.to_string(),
            &tool_input.input,
            if outcome.is_ok() {
                CallOutcome::Ok
            } else {
                CallOutcome::Failed
            },
        ));

        let cancelled = matches!(outcome, Outcome::Cancelled { .. });
        let outcome = self.after(&ctx, &matching, outcome);
        self.append_result(lease, call.call, tool_input.r#ref, outcome)
            .await?;

        if cancelled && cancel.is_cancelled() {
            return Ok(Some(TurnOutcome::Cancelled {
                turn: pass.turn,
                reason: cancel.reason(),
                usage: *budget.spent(),
            }));
        }
        Ok(None)
    }

    /// `tool.after`, folded into one outcome.
    fn after(&self, ctx: &InterceptCtx<'_>, m: &MatchCtx, outcome: Outcome) -> Outcome {
        let (outcome, chain) = self.interceptors.run::<ToolAfter>(ctx, m, outcome);
        match chain {
            ChainOutcome::Continue => outcome,
            ChainOutcome::Denied { reason } => denied(reason),
            ChainOutcome::Handled { result } => result,
        }
    }

    /// Assemble the context, compacting while it does not fit.
    ///
    /// `Ok(Err(kind))` is "it still does not fit and we have stopped trying",
    /// which the caller turns into a [`TurnOutcome::StoppedByBudget`].
    async fn build_context(
        &self,
        lease: &BranchLease,
        input: &TurnInput,
        pass: PassId,
        budget: &mut TurnBudget,
        cancel: &TurnCancel,
    ) -> Result<Result<ContextDraft, BudgetKind>, KernelError> {
        let capabilities = *self.provider.capabilities();
        let window = TokenBudget {
            max: capabilities.max_context,
            reserve: self.config.context_reserve_tokens,
        };
        let counter = self.provider.counter();
        let adapter = CounterAdapter(counter.clone());

        // TODO(plan-12): memory also owes the tree a `TurnKind::Recalled` row,
        // so a replay shows what the model saw rather than a query to re-run.
        // The row is plan 12's to write; the recall itself happens here because
        // the assembly order is this phase's.
        let recalled = self.memory.recall(&input.input, window).await;
        let recalled = recalled_message(self.memory.name(), &recalled)
            .map(|m| vec![m])
            .unwrap_or_default();

        for attempt in 0..=self.config.compaction_attempts {
            let materialised = self
                .store
                .materialise(lease.branch(), window, &adapter)
                .await?;
            let mut history = materialised.messages;
            let newest = history.pop().unwrap_or_else(|| input_message(&input.input));

            let tools = if capabilities.tools {
                self.registry.visible(&input.scope)
            } else {
                Vec::new()
            };
            let mut system = vec![Section::new("base", self.config.system_prompt.clone())];
            if let Some(agent) = &self.config.agent_prompt {
                system.push(Section::new("agent", agent.clone()));
            }
            system.push(Section::new("tools", tools_section(&tools)));
            if !self.config.skills.is_empty() {
                system.push(Section::new("skills", self.config.skills.join("\n\n")));
            }
            // Everything above this is stable from pass to pass. The volatile
            // suffix — recalled, history, the newest message — begins after it.
            let cache_breakpoint = system.len();

            let draft = ContextDraft {
                system,
                cache_breakpoint,
                recalled: recalled.clone(),
                history,
                input: newest,
                tools,
            };

            // context.build interceptors may rewrite and may not deny; the
            // registration check in `InterceptorSet::register` is what makes
            // that true rather than hoped for.
            let (draft, _chain) = self.interceptors.run::<ContextBuild>(
                &self.ctx(input, pass, budget),
                &MatchCtx::for_agent(input.scope.agent.clone()),
                draft,
            );

            if self.fits(&draft, counter.as_ref(), window) {
                return Ok(Ok(draft));
            }
            if attempt == self.config.compaction_attempts {
                break;
            }
            if !self
                .compact_once(lease, input, pass, budget, &draft, window, attempt + 1, cancel)
                .await?
            {
                break;
            }
        }
        Ok(Err(BudgetKind::Tokens))
    }

    /// Whether a draft fits the window it has to go into.
    fn fits(
        &self,
        draft: &ContextDraft,
        counter: &dyn orrery_provider::TokenCounter,
        window: TokenBudget,
    ) -> bool {
        let tokens = counter
            .count_messages(&draft.messages())
            .saturating_add(counter.count_text(&draft.system_text()));
        tokens <= window.available()
    }

    /// One compaction attempt. `false` means it cannot be done at all.
    #[allow(clippy::too_many_arguments)]
    async fn compact_once(
        &self,
        lease: &BranchLease,
        input: &TurnInput,
        pass: PassId,
        budget: &mut TurnBudget,
        draft: &ContextDraft,
        window: TokenBudget,
        attempt: u32,
        cancel: &TurnCancel,
    ) -> Result<bool, KernelError> {
        // The branch's last written sequence number: everything up to and
        // including it is what a summary would stand in for.
        let upto = Seq(lease.next_seq().0.saturating_sub(1));
        if upto.0 == 0 {
            return Ok(false);
        }
        let plan = CompactPlan {
            branch: lease.branch(),
            upto,
            messages: draft.messages(),
            target: window,
            attempt,
        };
        let (plan, chain) = self.interceptors.run::<ContextCompact>(
            &self.ctx(input, pass, budget),
            &MatchCtx::for_agent(input.scope.agent.clone()),
            plan,
        );
        if chain.is_final() {
            return Ok(false);
        }

        match self.compactor.compact(&plan, cancel.token()).await {
            Ok(Compacted { summary, usage }) => {
                // Charged to the turn that triggered it, like any other model
                // call (translation #8).
                budget.charge(&usage);
                self.store
                    .compact(
                        lease,
                        plan.upto,
                        NewTurn::new(TurnKind::Summary {
                            covers: (Seq(1), plan.upto),
                            text: summary,
                            usage,
                        }),
                    )
                    .await?;
                Ok(true)
            }
            Err(e) => {
                tracing::info!(
                    target: "orrery.kernel.context",
                    attempt,
                    error = %e,
                    "compaction did not happen; the turn stops on tokens"
                );
                Ok(false)
            }
        }
    }

    /// A draft, as the provider takes it.
    fn request_from(&self, draft: &ContextDraft) -> ModelRequest {
        let system = draft.system_text();
        ModelRequest {
            model: self.config.model.clone(),
            system: (!system.is_empty()).then(|| Arc::from(system.as_str())),
            messages: Arc::from(draft.messages()),
            tools: Arc::from(draft.tools.clone()),
            max_output_tokens: self.config.max_output_tokens,
            temperature: self.config.temperature,
            stop: self.config.stop.clone(),
            cache_breakpoint: self
                .provider
                .capabilities()
                .cache
                .then(|| draft.message_breakpoint()),
        }
    }

    /// One pass's stream, retried while the provider says it is worth it.
    async fn stream_pass(
        &self,
        request: ModelRequest,
        pass: PassId,
        budget: &TurnBudget,
        cancel: &TurnCancel,
    ) -> Result<PassResult, PassFailure> {
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            if cancel.is_cancelled() {
                return Err(PassFailure::Cancelled);
            }
            match self
                .one_attempt(request.clone(), pass, attempts, cancel)
                .await
            {
                Ok(mut result) => {
                    result.attempts = attempts;
                    return Ok(result);
                }
                Err(PassFailure::Cancelled) => return Err(PassFailure::Cancelled),
                Err(PassFailure::Provider { error, .. }) => {
                    let retryable =
                        error.is_retryable() && self.config.retry.may_retry(attempts);
                    self.audit.append(AuditEvent::Content {
                        action: "provider.attempt".to_owned(),
                        content: orrery_audit::ContentRef::new(
                            pass.to_string(),
                            error.code().to_owned(),
                            &format!(
                                "attempt {attempts} failed: {error}{}",
                                if retryable { "; retrying" } else { "" }
                            ),
                        ),
                    });
                    if !retryable {
                        return Err(PassFailure::Provider { error, attempts });
                    }
                    // The wait is charged to the turn's wall clock: it is
                    // measured from one `Instant` and sleeping does not stop it.
                    let wait = self.config.retry.backoff(attempts, &error);
                    if let Some(left) = budget.remaining_ms() {
                        if wait.as_millis() as u64 >= left {
                            return Err(PassFailure::Provider { error, attempts });
                        }
                    }
                    tokio::select! {
                        () = tokio::time::sleep(wait) => {}
                        () = cancel.token().cancelled() => return Err(PassFailure::Cancelled),
                    }
                }
            }
        }
    }

    /// One attempt: open the stream, accumulate, stop when it does.
    async fn one_attempt(
        &self,
        request: ModelRequest,
        pass: PassId,
        attempt: u32,
        cancel: &TurnCancel,
    ) -> Result<PassResult, PassFailure> {
        let mut stream = self.provider.stream(request, cancel.token().clone());
        let mut accumulator = ToolCallAccumulator::new();
        let mut result = PassResult {
            pass,
            text: String::new(),
            thinking: String::new(),
            calls: Vec::new(),
            usage: Usage::default(),
            stop: None,
            attempts: attempt,
        };

        loop {
            let next = tokio::select! {
                biased;
                () = cancel.token().cancelled() => {
                    // Dropping the stream aborts the request, which is what
                    // makes cancellation stop costing money rather than stop
                    // being rendered.
                    drop(stream);
                    return Err(PassFailure::Cancelled);
                }
                item = stream.next() => item,
            };
            let Some(item) = next else { break };
            let event = match item {
                Ok(event) => event,
                Err(error) => return Err(PassFailure::Provider { error, attempts: attempt }),
            };
            if let Some(completed) = accumulator.feed(&event) {
                match completed {
                    Ok(call) => result.calls.push(call),
                    Err(e) => {
                        // A model that emitted invalid JSON is a model that can
                        // be told so on the next pass; it is not a broken
                        // harness.
                        tracing::warn!(
                            target: "orrery.kernel.turn",
                            %pass,
                            error = %e,
                            "a tool call's arguments did not parse"
                        );
                    }
                }
            }
            match event {
                ModelEvent::TextDelta { text } => result.text.push_str(&text),
                ModelEvent::ThinkingDelta { text } => result.thinking.push_str(&text),
                ModelEvent::Usage { usage } => result.usage += usage,
                ModelEvent::Done { stop } => {
                    result.stop = Some(stop);
                    break;
                }
                _ => {}
            }
        }
        Ok(result)
    }

    /// Keep whatever the model managed to say before it was stopped.
    async fn append_partial(
        &self,
        lease: &BranchLease,
        state: &TurnState,
    ) -> Result<(), KernelError> {
        if state.text.is_empty() {
            return Ok(());
        }
        self.store
            .append(
                lease,
                NewTurn::new(TurnKind::Assistant {
                    content: vec![ContentBlock::Text {
                        text: state.text.clone(),
                    }],
                    usage: Usage::default(),
                }),
            )
            .await?;
        Ok(())
    }

    /// Append one settled call.
    async fn append_result(
        &self,
        lease: &BranchLease,
        call: CallId,
        r#ref: ToolRef,
        outcome: Outcome,
    ) -> Result<(), KernelError> {
        self.store
            .append(
                lease,
                NewTurn::new(TurnKind::ToolResult {
                    call,
                    r#ref,
                    outcome,
                }),
            )
            .await?;
        Ok(())
    }

    fn ctx<'a>(
        &self,
        input: &'a TurnInput,
        pass: PassId,
        budget: &'a TurnBudget,
    ) -> InterceptCtx<'a> {
        InterceptCtx {
            session: input.session,
            turn: pass.turn,
            pass,
            agent: &input.scope,
            budget_spent: budget.spent(),
        }
    }

    fn stopped(&self, turn: TurnId, kind: BudgetKind, budget: &TurnBudget) -> TurnOutcome {
        tracing::info!(
            target: "orrery.kernel.budget",
            %turn,
            ?kind,
            turns = budget.turns(),
            "a ceiling stopped the turn"
        );
        TurnOutcome::StoppedByBudget {
            turn,
            kind,
            usage: *budget.spent(),
        }
    }
}

/// What the loop is carrying between passes.
struct TurnState {
    turn: TurnId,
    passes: u32,
    text: String,
    tool_calls: u32,
}

/// Why a pass did not produce a result.
enum PassFailure {
    Cancelled,
    Provider {
        error: ProviderError,
        attempts: u32,
    },
}

/// A chain outcome that stops a phase, as the outcome it settles as.
fn final_outcome(chain: ChainOutcome) -> Option<Outcome> {
    match chain {
        ChainOutcome::Continue => None,
        ChainOutcome::Denied { reason } => Some(denied(reason)),
        ChainOutcome::Handled { result } => Some(result),
    }
}

/// The rule id a refusal carries when no numbered rule is responsible for it.
fn denied(reason: String) -> Outcome {
    Outcome::Denied {
        rule: "00000000-0000-0000-0000-000000000000"
            .parse()
            .expect("the nil uuid is a uuid"),
        reason,
    }
}

/// A reference for a name that never resolved.
///
/// [`TurnKind::ToolResult`] names a [`ToolRef`], and a name the registry does
/// not know has none. Rather than drop the row — which would leave the model's
/// `tool_use` block unanswered and the transcript unbalanced — it is filed
/// under the reserved extension `unresolved`.
fn unresolved(name: &str) -> ToolRef {
    name.parse().unwrap_or_else(|_| ToolRef {
        ext: ExtId::new("unresolved").expect("`unresolved` is a valid ext id"),
        name: name.to_owned(),
    })
}

/// The store counts messages; the provider counts tokens. One adapter, because
/// the store must not know which provider is bound.
struct CounterAdapter(Arc<dyn orrery_provider::TokenCounter>);

impl orrery_session::TokenCounter for CounterAdapter {
    fn count(&self, messages: &[Message]) -> u64 {
        self.0.count_messages(messages)
    }
}

/// A plain text message, for whoever is assembling one by hand.
#[must_use]
pub fn assistant_text(text: impl Into<String>) -> Message {
    Message::text(MessageRole::Assistant, text)
}
