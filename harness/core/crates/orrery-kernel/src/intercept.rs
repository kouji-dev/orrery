//! The chain: sync, typed, and holding nothing it could do I/O with.
//!
//! # Translation #1, as a type rather than a rule
//!
//! [`Interceptor::run`] is a **synchronous** `fn` and [`InterceptCtx`] carries
//! five values, none of them a handle. There is no broker, no store, no
//! cancellation token and no runtime, so an interceptor cannot read a file,
//! call a service or block the loop — not by convention, but because there is
//! nothing to call. A `Handled` verdict therefore has to come from data the
//! interceptor already holds, which is what makes the cache case honest.
//!
//! # What the chain enforces
//!
//! - **`Deny` narrows only.** The chain runs *before* the policy check and
//!   re-checks nothing after, so an interceptor can refuse a call policy would
//!   have allowed and can never allow one policy refused.
//! - **Order is deterministic.** Registration order within a phase, and the
//!   registration order is the layer order because the loader registers
//!   closest-first.
//! - **Every `Deny` and `Handled` is in the audit.**

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use orrery_audit::{Audit, AuditEvent, Digest};
use orrery_proto::{
    AgentScope, Contribution, ContributionKind, Outcome, SessionId, TurnId, Usage, Verdict,
};

use crate::phase::Phase;
use crate::turn::PassId;

/// What an interceptor is allowed to look at when deciding whether it applies.
///
/// Separate from [`InterceptCtx`] because `matches` is asked once per call and
/// has no business seeing what has been spent.
#[derive(Clone, Debug, Default)]
pub struct MatchCtx {
    /// The tool, at a `tool.*` phase.
    pub tool: Option<String>,
    /// The agent whose turn this is.
    pub agent: String,
}

impl MatchCtx {
    /// A match context for an agent, naming no tool.
    #[must_use]
    pub fn for_agent(agent: impl Into<String>) -> Self {
        Self {
            tool: None,
            agent: agent.into(),
        }
    }

    /// The same, naming a tool.
    #[must_use]
    pub fn for_tool(agent: impl Into<String>, tool: impl Into<String>) -> Self {
        Self {
            tool: Some(tool.into()),
            agent: agent.into(),
        }
    }
}

/// Deliberately anaemic. If it is not here, an interceptor cannot reach it.
#[derive(Clone, Copy, Debug)]
pub struct InterceptCtx<'a> {
    /// Which session.
    pub session: SessionId,
    /// Which turn.
    pub turn: TurnId,
    /// Which pass through the loop.
    pub pass: PassId,
    /// Who is acting, and what they may see.
    pub agent: &'a AgentScope,
    /// What the turn has spent so far.
    pub budget_spent: &'a Usage,
}

/// One interceptor at one phase.
///
/// **Sync.** See the module docs.
pub trait Interceptor<P: Phase>: Send + Sync {
    /// Whether this interceptor applies to this call at all.
    fn matches(&self, _m: &MatchCtx) -> bool {
        true
    }

    /// Whether this interceptor ever returns [`Verdict::Deny`].
    ///
    /// Declared rather than inferred, because a phase that cannot honour a
    /// denial has to refuse the *registration* — discovering it mid-turn would
    /// mean either ignoring a verdict or failing a turn over a configuration
    /// mistake. An interceptor that only rewrites says so and can be registered
    /// anywhere.
    fn can_deny(&self) -> bool {
        true
    }

    /// Look at the payload, and say what should happen.
    fn run(&self, ctx: &InterceptCtx<'_>, payload: &P::Payload) -> Verdict<P::Payload>;
}

/// How a whole chain ended.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum ChainOutcome {
    /// Nobody stopped it. The payload comes back, rewritten or not.
    Continue,
    /// Somebody refused. Becomes an [`Outcome::Denied`], never an error.
    Denied {
        /// Why, in words a person can act on.
        reason: String,
    },
    /// Somebody answered from what they already held; dispatch never runs.
    Handled {
        /// What to report as having happened.
        result: Outcome,
    },
}

impl ChainOutcome {
    /// Whether the chain stopped the phase.
    #[must_use]
    pub fn is_final(&self) -> bool {
        !matches!(self, ChainOutcome::Continue)
    }
}

/// A registration the set refused.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegisterError {
    /// An interceptor that can deny, registered at a phase where a denial means
    /// nothing.
    #[error(
        "`{phase}` is rewrite-only: an interceptor there cannot return `Deny`, \
         and this one says it can. Override `can_deny` to return false, or \
         register it at `tool.before`, which is where a refusal belongs."
    )]
    DenyNotAllowed {
        /// The phase that refused it.
        phase: &'static str,
    },
}

/// The erased half: one vector per phase, keyed by `P::NAME`.
trait AnyInterceptor: Send + Sync {
    fn matches(&self, m: &MatchCtx) -> bool;
    /// Takes the payload by value and hands it back, because a rewrite replaces
    /// it. The box is `P::Payload` and nothing else — the key it was filed
    /// under is `P::NAME`, so the downcast cannot be wrong.
    fn run_erased(
        &self,
        ctx: &InterceptCtx<'_>,
        payload: Box<dyn Any + Send>,
    ) -> (Box<dyn Any + Send>, ChainOutcome);
}

struct Typed<P: Phase, I: Interceptor<P>> {
    inner: I,
    _phase: std::marker::PhantomData<fn() -> P>,
}

impl<P: Phase, I: Interceptor<P>> AnyInterceptor for Typed<P, I> {
    fn matches(&self, m: &MatchCtx) -> bool {
        self.inner.matches(m)
    }

    fn run_erased(
        &self,
        ctx: &InterceptCtx<'_>,
        payload: Box<dyn Any + Send>,
    ) -> (Box<dyn Any + Send>, ChainOutcome) {
        let payload: Box<P::Payload> = payload
            .downcast()
            .expect("a phase's vector only ever holds that phase's payload");
        match self.inner.run(ctx, &payload) {
            Verdict::Continue => (payload, ChainOutcome::Continue),
            Verdict::Rewrite(next) => (Box::new(next), ChainOutcome::Continue),
            Verdict::Deny { reason } => (payload, ChainOutcome::Denied { reason }),
            Verdict::Handled { result } => (payload, ChainOutcome::Handled { result }),
            // `Verdict` is `#[non_exhaustive]`: a variant this crate has not
            // been taught about is not a licence to skip the rest of the chain.
            _ => (payload, ChainOutcome::Continue),
        }
    }
}

/// Every interceptor the session has, filed by phase.
pub struct InterceptorSet {
    by_phase: HashMap<&'static str, Vec<Arc<dyn AnyInterceptor>>>,
    /// Registration order across phases, for the ledger.
    order: Vec<&'static str>,
    audit: Audit,
}

impl std::fmt::Debug for InterceptorSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut counts: Vec<(&str, usize)> = self
            .by_phase
            .iter()
            .map(|(k, v)| (*k, v.len()))
            .collect::<Vec<_>>();
        counts.sort();
        f.debug_struct("InterceptorSet")
            .field("phases", &counts)
            .finish()
    }
}

impl Default for InterceptorSet {
    fn default() -> Self {
        Self::new()
    }
}

impl InterceptorSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_phase: HashMap::new(),
            order: Vec::new(),
            audit: orrery_audit::null(),
        }
    }

    /// Record every `Deny` and `Handled` in an audit stream.
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = audit;
        self
    }

    /// Add an interceptor at a phase.
    ///
    /// # Errors
    ///
    /// [`RegisterError::DenyNotAllowed`] when the interceptor can deny and the
    /// phase cannot honour a denial.
    pub fn register<P: Phase>(
        &mut self,
        interceptor: impl Interceptor<P> + 'static,
    ) -> Result<(), RegisterError> {
        if !P::ALLOWS_DENY && interceptor.can_deny() {
            return Err(RegisterError::DenyNotAllowed { phase: P::NAME });
        }
        self.by_phase.entry(P::NAME).or_default().push(Arc::new(Typed {
            inner: interceptor,
            _phase: std::marker::PhantomData,
        }));
        self.order.push(P::NAME);
        Ok(())
    }

    /// How many interceptors are registered at a phase.
    #[must_use]
    pub fn len_at<P: Phase>(&self) -> usize {
        self.by_phase.get(P::NAME).map_or(0, Vec::len)
    }

    /// What this set contributes, as the load ledger reports it.
    ///
    /// One entry per registration, named for the phase, so
    /// `query extensions` can answer "what is rewriting my tool calls".
    #[must_use]
    pub fn contributions(&self) -> Vec<Contribution> {
        self.order
            .iter()
            .map(|phase| Contribution {
                kind: ContributionKind::Interceptor,
                name: (*phase).to_owned(),
            })
            .collect()
    }

    /// Run the chain for one phase.
    ///
    /// Interceptors run in registration order and stop at the first final
    /// verdict. The payload that comes back is whatever the last rewrite left.
    #[must_use]
    pub fn run<P: Phase>(
        &self,
        ctx: &InterceptCtx<'_>,
        m: &MatchCtx,
        payload: P::Payload,
    ) -> (P::Payload, ChainOutcome) {
        let Some(chain) = self.by_phase.get(P::NAME) else {
            return (payload, ChainOutcome::Continue);
        };
        let mut erased: Box<dyn Any + Send> = Box::new(payload);
        for interceptor in chain {
            if !interceptor.matches(m) {
                continue;
            }
            let (next, outcome) = interceptor.run_erased(ctx, erased);
            erased = next;
            if outcome.is_final() {
                self.record(P::NAME, m, &outcome);
                return (unbox::<P>(erased), outcome);
            }
        }
        (unbox::<P>(erased), ChainOutcome::Continue)
    }

    /// Every refusal and every short circuit lands in the stream, with the
    /// phase that produced it.
    fn record(&self, phase: &'static str, m: &MatchCtx, outcome: &ChainOutcome) {
        let (verdict, reason) = match outcome {
            ChainOutcome::Denied { reason } => ("deny", Some(reason.clone())),
            ChainOutcome::Handled { result } => (
                "handled",
                Some(format!(
                    "answered from the interceptor's own data ({})",
                    Digest::of(result).as_str()
                )),
            ),
            ChainOutcome::Continue => return,
        };
        tracing::info!(
            target: "orrery.kernel.intercept",
            phase,
            tool = m.tool.as_deref().unwrap_or("-"),
            verdict,
            "an interceptor stopped a phase"
        );
        self.audit.append(AuditEvent::CapabilityDecision {
            subject: orrery_proto::Subject::SubAgent(m.agent.clone()),
            request: match &m.tool {
                Some(tool) => format!("{phase}({tool})"),
                None => format!("{phase}()"),
            },
            verdict: match verdict {
                "deny" => orrery_audit::Verdict::Deny,
                _ => orrery_audit::Verdict::Allow,
            },
            rule: None,
            rule_text: None,
            layer: None,
            reason,
        });
    }
}

fn unbox<P: Phase>(erased: Box<dyn Any + Send>) -> P::Payload {
    *erased
        .downcast::<P::Payload>()
        .expect("a phase's vector only ever holds that phase's payload")
}
