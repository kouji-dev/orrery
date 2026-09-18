//! Roles, bindings, declarative rules, and the sync signals-to-decision function. Returns data, acts on nothing.
//!
//! # The model proposes, the router disposes
//!
//! Escalation from one pass to a bounded loop to a sub-agent to five sub-agents
//! to a workflow is a **request**, checked against declared rules and a budget
//! and audited with the signal values behind it. Not a vibe, and not something
//! a prompt can talk its way past.
//!
//! # The four things this crate makes true
//!
//! - **[`Router::decide`] is sync and returns data.** That is the cut that
//!   keeps the dependency graph acyclic: the kernel runs a turn, the
//!   orchestrator runs steps, and the router only produces a [`RouteDecision`]
//!   for the orchestrator to act on. This crate depends on neither of them, and
//!   holds no handle it could act through.
//! - **Rules read cheap signals only.** [`Signals`] is `Copy`, so nothing that
//!   needs fetching or generating can be added to it. See [`signals`].
//! - **Fan-out is computed, not chosen.** `N = min(disjoint units, cap,
//!   remaining ÷ declared child cost)` — [`fanout`].
//! - **Modes are permissions.** A mode switch is an ordinary capability request
//!   the policy engine answers, not a UI state — [`mode`].
//!
//! Implementation plan: `harness/docs/plans/11-router-roles-orchestrator.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod decide;
pub mod fanout;
pub mod mode;
pub mod roles;
pub mod rules;
pub mod signals;

pub use decide::{
    DEFAULT_MAX_ITERATIONS, Decided, Proposal, RouteDecision, Router, RouterProfile, Source,
    StepRef,
};
pub use fanout::{Bound, FanOutPlan, FanOutProfile, are_disjoint, n_of};
pub use roles::{
    Bindings, RoleAgentDef, RoleError, RoleOffer, bind_scope, role_of, role_word, shadowed_line,
};
pub use rules::{Condition, Effect, Op, RoutingRule, RuleError, RuleSet, RuleVerdict, Rung};
pub use signals::{ABSENT, GateOutcome, Mode, Signals};
