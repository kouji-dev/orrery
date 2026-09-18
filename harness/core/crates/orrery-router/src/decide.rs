//! The decision itself: sync, pure, and returning data.
//!
//! # Precedence
//!
//! An explicit user instruction, then a declared rule, then a model proposal
//! the router grants, downgrades or denies. That order is the whole of
//! [`Router::decide`], and it is why a [`Proposal`] carries a [`Source`]: the
//! same words mean something different depending on who said them.
//!
//! # Why this returns data
//!
//! `decide` is a `fn`, not an `async fn`, and it hands back a
//! [`RouteDecision`]. It cannot spawn, call or wait, so the orchestrator
//! depends on the router and the router depends on nothing that runs. That is
//! the cut that keeps the graph acyclic.

use orrery_audit::{Audit, AuditEvent};
use orrery_proto::{AgentScope, Budget, Predicate};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fanout::{self, FanOutProfile};
use crate::rules::{RuleSet, RuleVerdict, Rung};
use crate::signals::Signals;

/// A step in a declared body, by name. The orchestrator resolves it; the router
/// only passes it along.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StepRef(pub String);

impl StepRef {
    /// Name one.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl std::fmt::Display for StepRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the router decided. **Data.** Acting on it is the orchestrator's job.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum RouteDecision {
    /// Run another pass under the turn budget.
    NextPass,
    /// Loop, with a predicate and a hard cap the kernel enforces.
    BoundedLoop {
        /// What to run each time round.
        body: StepRef,
        /// When to stop.
        until: Predicate,
        /// The cap, which stops it whether or not `until` ever holds.
        max_iterations: u32,
    },
    /// One child, on its own branch, with a slice of the parent's budget.
    SubAgent {
        /// Which agent.
        agent: String,
        /// What it may spend.
        budget: Budget,
    },
    /// Several children over disjoint inputs.
    FanOut {
        /// Which agent.
        agent: String,
        /// One input set per child. Its length is N.
        inputs: Vec<Value>,
        /// What each child may spend.
        budget_each: Budget,
    },
    /// A declared sequence, typechecked at load.
    Workflow {
        /// Which one.
        name: String,
    },
    /// No.
    Deny {
        /// Why, in words the caller — and the model — can act on.
        reason: String,
    },
}

impl RouteDecision {
    /// The word the audit records, and the rung it corresponds to.
    #[must_use]
    pub const fn chose(&self) -> &'static str {
        match self {
            RouteDecision::NextPass => "one-pass",
            RouteDecision::BoundedLoop { .. } => "bounded-loop",
            RouteDecision::SubAgent { .. } => "sub-agent",
            RouteDecision::FanOut { .. } => "fan-out",
            RouteDecision::Workflow { .. } => "workflow",
            RouteDecision::Deny { .. } => "deny",
        }
    }

    /// Which rung it lands on, when it lands on one.
    #[must_use]
    pub const fn rung(&self) -> Option<Rung> {
        Some(match self {
            RouteDecision::NextPass => Rung::OnePass,
            RouteDecision::BoundedLoop { .. } => Rung::BoundedLoop,
            RouteDecision::SubAgent { .. } => Rung::SubAgent,
            RouteDecision::FanOut { .. } => Rung::FanOut,
            RouteDecision::Workflow { .. } => Rung::Workflow,
            RouteDecision::Deny { .. } => return None,
        })
    }
}

/// Who is asking.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// The model proposed it. Subject to the rules and to the ladder.
    #[default]
    Model,
    /// A person instructed it. Beats a declared rule, and skips the ladder,
    /// because "explicitly instructed" is exactly what the ladder's exception
    /// is for.
    User,
}

/// What somebody wants to do next.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    /// Who wants it.
    pub source: Source,
    /// Which rung is being asked for.
    pub rung: Rung,
    /// Which rung the caller is on now. The ladder is climbed relative to this,
    /// and it is the caller's to know: the router is pure and remembers nothing
    /// between calls.
    pub from: Rung,
    /// The agent a sub-agent or fan-out would run.
    pub agent: Option<String>,
    /// The workflow a workflow rung would run.
    pub workflow: Option<String>,
    /// One input set per proposed child.
    pub inputs: Vec<Value>,
    /// The body of a proposed loop.
    pub body: Option<StepRef>,
    /// The predicate of a proposed loop.
    pub until: Option<Predicate>,
    /// The cap of a proposed loop.
    pub max_iterations: Option<u32>,
}

impl Proposal {
    /// A model's proposal to climb to `rung` from `OnePass`.
    #[must_use]
    pub fn model(rung: Rung) -> Self {
        Self {
            source: Source::Model,
            rung,
            from: Rung::OnePass,
            agent: None,
            workflow: None,
            inputs: Vec::new(),
            body: None,
            until: None,
            max_iterations: None,
        }
    }

    /// A person's instruction to climb to `rung`.
    #[must_use]
    pub fn user(rung: Rung) -> Self {
        Self {
            source: Source::User,
            ..Self::model(rung)
        }
    }

    /// Say which rung this is being climbed from.
    #[must_use]
    pub fn from(mut self, from: Rung) -> Self {
        self.from = from;
        self
    }

    /// Name the agent.
    #[must_use]
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }

    /// Name the workflow.
    #[must_use]
    pub fn with_workflow(mut self, name: impl Into<String>) -> Self {
        self.workflow = Some(name.into());
        self
    }

    /// Give the children their input sets.
    #[must_use]
    pub fn with_inputs(mut self, inputs: Vec<Value>) -> Self {
        self.inputs = inputs;
        self
    }

    /// Give a loop its body, predicate and cap.
    #[must_use]
    pub fn looping(mut self, body: &str, until: Predicate, max_iterations: u32) -> Self {
        self.body = Some(StepRef::new(body));
        self.until = Some(until);
        self.max_iterations = Some(max_iterations);
        self
    }
}

/// The hard cap on a loop nobody declared one for.
///
/// Not a free-running `while`: a proposal that forgets to say when to stop is
/// still bounded, because the kernel enforces a cap whatever the prompt says.
pub const DEFAULT_MAX_ITERATIONS: u32 = 4;

/// A decision, with everything needed to explain it afterwards.
#[derive(Clone, Debug, PartialEq)]
pub struct Decided {
    /// What was decided.
    pub decision: RouteDecision,
    /// The rung that was asked for, before any downgrade.
    pub asked: Option<Rung>,
    /// The rule that produced it, when a rule did.
    pub rule: Option<String>,
    /// The signal values behind it.
    pub signals: std::collections::BTreeMap<String, f64>,
}

impl Decided {
    /// The audit line: what was chosen, and the numbers that chose it.
    #[must_use]
    pub fn audit_event(&self) -> AuditEvent {
        AuditEvent::RoutingDecision {
            chose: self.decision.chose().to_owned(),
            signals: self.signals.clone(),
        }
    }
}

/// What a profile declares about routing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RouterProfile {
    /// The rules in force.
    pub rules: RuleSet,
    /// What fanning out costs and how wide it may go.
    pub fanout: FanOutProfile,
}

/// The router.
///
/// Holds declared configuration and an audit sink. It holds **no** handle to
/// anything that runs.
#[derive(Clone)]
pub struct Router {
    profile: RouterProfile,
    audit: Option<Audit>,
}

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Router")
            .field("rules", &self.profile.rules.rules.len())
            .field("fanout", &self.profile.fanout)
            .finish_non_exhaustive()
    }
}

impl Router {
    /// A router over a profile.
    #[must_use]
    pub fn new(profile: RouterProfile) -> Self {
        Self {
            profile,
            audit: None,
        }
    }

    /// A router with no rules and a fan-out cap of one.
    #[must_use]
    pub fn inert() -> Self {
        Self::new(RouterProfile::default())
    }

    /// Record every decision here.
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = Some(audit);
        self
    }

    /// The profile in force.
    #[must_use]
    pub const fn profile(&self) -> &RouterProfile {
        &self.profile
    }

    /// **Sync. Pure. Returns DATA** — this is what breaks the
    /// kernel↔orchestrator cycle.
    ///
    /// Auditing is the one side effect, and it is append-only: the same signals
    /// in produce the same decision out however many times it is called.
    #[must_use]
    pub fn decide(
        &self,
        signals: &Signals,
        scope: &AgentScope,
        proposal: Option<Proposal>,
    ) -> RouteDecision {
        self.decide_explained(signals, scope, proposal).decision
    }

    /// [`decide`](Router::decide), plus the rule and the numbers behind it.
    #[must_use]
    pub fn decide_explained(
        &self,
        signals: &Signals,
        scope: &AgentScope,
        proposal: Option<Proposal>,
    ) -> Decided {
        let decided = self.judge(signals, scope, proposal);
        if let Some(audit) = &self.audit {
            audit.append(decided.audit_event());
        }
        decided
    }

    fn judge(&self, signals: &Signals, _scope: &AgentScope, proposal: Option<Proposal>) -> Decided {
        let values = signals.values();
        let Some(proposal) = proposal else {
            // Nobody asked for anything. The default rung is the cheap one.
            return Decided {
                decision: RouteDecision::NextPass,
                asked: None,
                rule: None,
                signals: values,
            };
        };
        let asked = Some(proposal.rung);

        // 1 · an explicit user instruction. It beats a declared rule and it is
        //     the ladder's documented exception, so neither is consulted.
        if proposal.source == Source::User {
            return Decided {
                decision: self.materialise(proposal.rung, &proposal, signals),
                asked,
                rule: None,
                signals: values,
            };
        }

        // 2 · a declared rule.
        if let RuleVerdict::Denied { rule, reason } =
            self.profile.rules.verdict(signals, proposal.rung)
        {
            return Decided {
                decision: RouteDecision::Deny { reason },
                asked,
                rule: Some(rule),
                signals: values,
            };
        }
        let rule = match self.profile.rules.verdict(signals, proposal.rung) {
            RuleVerdict::Allowed { rule } => Some(rule),
            _ => None,
        };

        // 3 · the model's proposal, granted or downgraded. Rungs are climbed
        //     one at a time: a jump from one pass to a workflow becomes a
        //     bounded loop, and the model may ask again from there.
        let mut rung = proposal.rung;
        if rung.level() > proposal.from.level().saturating_add(1) {
            rung = proposal.from.next();
        }

        Decided {
            decision: self.materialise(rung, &proposal, signals),
            asked,
            rule,
            signals: values,
        }
    }

    /// Turn a granted rung into the decision that carries it out.
    fn materialise(&self, rung: Rung, proposal: &Proposal, signals: &Signals) -> RouteDecision {
        match rung {
            Rung::OnePass => RouteDecision::NextPass,
            Rung::BoundedLoop => RouteDecision::BoundedLoop {
                body: proposal
                    .body
                    .clone()
                    .unwrap_or_else(|| StepRef::new("current")),
                // A loop with no declared predicate still terminates: the cap
                // is the kernel's, not the prompt's.
                until: proposal
                    .until
                    .clone()
                    .unwrap_or_else(|| Predicate::All(Vec::new())),
                max_iterations: proposal
                    .max_iterations
                    .unwrap_or(DEFAULT_MAX_ITERATIONS)
                    .max(1),
            },
            Rung::SubAgent => match &proposal.agent {
                Some(agent) => RouteDecision::SubAgent {
                    agent: agent.clone(),
                    budget: slice(signals.budget_limit, signals, 1),
                },
                None => RouteDecision::Deny {
                    reason: "a sub-agent rung names the agent to run; this proposal names none"
                        .to_owned(),
                },
            },
            Rung::FanOut => {
                let Some(agent) = proposal.agent.clone() else {
                    return RouteDecision::Deny {
                        reason: "a fan-out rung names the agent to run; this proposal names none"
                            .to_owned(),
                    };
                };
                let plan = fanout::plan(
                    &proposal.inputs,
                    &self.profile.fanout,
                    signals.remaining_tokens(),
                );
                if plan.n == 0 {
                    return RouteDecision::Deny {
                        reason: format!(
                            "fan-out computes to zero children ({:?}); there is nothing to spend",
                            plan.bound
                        ),
                    };
                }
                if plan.n == 1 {
                    // One child is not a fan-out. Downgrading rather than
                    // denying keeps the work moving on the rung below.
                    return RouteDecision::SubAgent {
                        agent,
                        budget: slice(signals.budget_limit, signals, 1),
                    };
                }
                let inputs: Vec<Value> = proposal
                    .inputs
                    .iter()
                    .take(plan.n as usize)
                    .cloned()
                    .collect();
                RouteDecision::FanOut {
                    agent,
                    budget_each: slice(signals.budget_limit, signals, plan.n),
                    inputs,
                }
            }
            Rung::Workflow => match &proposal.workflow {
                Some(name) => RouteDecision::Workflow { name: name.clone() },
                None => RouteDecision::Deny {
                    reason: "a workflow rung names the workflow to run; this proposal names none"
                        .to_owned(),
                },
            },
        }
    }
}

/// A child's slice of what is left, split `n` ways.
///
/// Arithmetic on the parent's remaining budget, so it can never exceed it, and
/// deterministic, so `decide` stays pure.
fn slice(limit: Budget, signals: &Signals, n: u32) -> Budget {
    let n = u64::from(n.max(1));
    let remaining_tokens = signals.remaining_tokens();
    Budget {
        max_turns: limit.max_turns,
        max_tokens: remaining_tokens / n,
        wall_clock_ms: limit.wall_clock_ms,
        max_micro_usd: limit.max_micro_usd.map(|usd| {
            let spent = signals.budget_spent.micro_usd.unwrap_or(0);
            usd.saturating_sub(spent) / n
        }),
    }
}
