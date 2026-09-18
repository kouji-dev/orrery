//! Roles: the kernel names them, extensions and profiles bind them.
//!
//! The same indirection as the provider layer, turned on the loop itself.
//! "Plan with a large model and compact with a cheap one" is then two lines of
//! configuration rather than a branch in the loop.
//!
//! Four invariants keep this from becoming a second control flow:
//!
//! 1. A role binding is **intersected** with the step's grant and never widens
//!    it — [`bind_scope`].
//! 2. Exactly **one** binding wins per role per step, by layer precedence, and
//!    the audit records which one ran — [`Bindings::resolve`] and
//!    [`Bindings::audit_events`].
//! 3. Phases fire inside **every** step regardless of the agent. That one is
//!    the step machine's to keep, and it is asserted in `orrery-orchestrator`,
//!    where steps actually run.
//! 4. A role bound to an agent that does not exist fails at **`session.start`**
//!    — [`Bindings::resolve`] returns [`RoleError`], and it names the file and
//!    the line.
//!
//! # `router` is unbound by default
//!
//! Leaving it unbound keeps routing declarative and therefore reproducible,
//! which is what eval comparison needs. Binding it to an agent is how a model
//! router happens, and it is this same mechanism rather than a new one — which
//! is why nothing here treats `router` as special apart from
//! [`Bindings::is_declarative`].

use std::collections::{BTreeMap, HashMap};

use orrery_audit::AuditEvent;
use orrery_proto::{AgentScope, Grant, GrantSpec, Layer, Role};

/// One `[roles]` line, as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleOffer {
    /// Which role.
    pub role: Role,
    /// The agent it names.
    pub agent: String,
    /// Which layer wrote it.
    pub layer: Layer,
    /// Which file.
    pub file: String,
    /// Which line.
    pub line: u32,
}

impl RoleOffer {
    /// A binding written in one layer.
    #[must_use]
    pub fn new(
        role: Role,
        agent: impl Into<String>,
        layer: Layer,
        file: impl Into<String>,
        line: u32,
    ) -> Self {
        Self {
            role,
            agent: agent.into(),
            layer,
            file: file.into(),
            line,
        }
    }

    /// `file:line`, which is what an error message leads with.
    #[must_use]
    pub fn origin(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }
}

/// The word a role is written with.
///
/// [`Role`] is `#[non_exhaustive]`, so the wildcard is required. A role this
/// build does not know prints as `unknown` rather than failing to compile the
/// day `orrery-proto` grows one.
#[must_use]
pub fn role_word(role: Role) -> &'static str {
    match role {
        Role::Planner => "planner",
        Role::Executor => "executor",
        Role::Verifier => "verifier",
        Role::Compactor => "compactor",
        Role::Summariser => "summariser",
        Role::Router => "router",
        Role::Grader => "grader",
        _ => "unknown",
    }
}

/// Read a role back from the word it is written with.
#[must_use]
pub fn role_of(word: &str) -> Option<Role> {
    [
        Role::Planner,
        Role::Executor,
        Role::Verifier,
        Role::Compactor,
        Role::Summariser,
        Role::Router,
        Role::Grader,
    ]
    .into_iter()
    .find(|r| role_word(*r) == word)
}

/// A role bound to an agent nobody defined.
///
/// Raised at `session.start` — step 4 of the startup order — and **never**
/// mid-turn, when a run that has already cost money would fall over reaching
/// for it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "{file}:{line}: the `{role}` role is bound to `{agent}`, and no agent by that name is defined"
)]
pub struct RoleError {
    /// Which role.
    pub role: String,
    /// What it was bound to.
    pub agent: String,
    /// Which file.
    pub file: String,
    /// Which line.
    pub line: u32,
}

/// What an agent a role may be bound to declares.
///
/// Only the parts binding cares about: what it may do, and what it may see.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoleAgentDef {
    /// Its name, as a `[roles]` line spells it.
    pub name: String,
    /// What it asks for. A declaration narrows; it never substitutes.
    pub grant: GrantSpec,
    /// The tools it asks to see. Empty means "whatever the step sees".
    pub tools: Vec<String>,
}

impl RoleAgentDef {
    /// An agent that narrows nothing.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Ask for these capabilities.
    #[must_use]
    pub fn asking(mut self, grant: GrantSpec) -> Self {
        self.grant = grant;
        self
    }

    /// Ask to see these tools.
    #[must_use]
    pub fn seeing(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tools = tools.into_iter().map(Into::into).collect();
        self
    }
}

/// Every role binding in force, plus the ones that lost.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bindings {
    bound: HashMap<Role, RoleOffer>,
    shadowed: Vec<RoleOffer>,
}

impl Bindings {
    /// Nothing bound. Every role falls back to its shipped default, and
    /// `router` stays declarative.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Resolve every offer into one binding per role.
    ///
    /// The closest layer wins — [`Layer`] is ordered `Managed < … < Project`
    /// precisely so that `max()` reads as "closest wins" — and ties go to the
    /// offer written first, so resolution is deterministic. Everything that
    /// lost is kept in [`shadowed`](Bindings::shadowed) so the ledger can name
    /// it: a binding that silently did not take effect is the kind of thing
    /// people lose an afternoon to.
    ///
    /// # Errors
    ///
    /// [`RoleError`] when a role names an agent that is not in `known_agents`,
    /// naming the file and the line it was written on.
    pub fn resolve(
        offers: impl IntoIterator<Item = RoleOffer>,
        known_agents: &[String],
    ) -> Result<Bindings, RoleError> {
        let mut bound: HashMap<Role, RoleOffer> = HashMap::new();
        let mut shadowed: Vec<RoleOffer> = Vec::new();

        for offer in offers {
            // Invariant 4, and it fires before anything runs.
            let short = offer.agent.rsplit('.').next().unwrap_or(&offer.agent);
            if !known_agents.iter().any(|a| a == &offer.agent || a == short) {
                return Err(RoleError {
                    role: role_word(offer.role).to_owned(),
                    agent: offer.agent.clone(),
                    file: offer.file.clone(),
                    line: offer.line,
                });
            }
            match bound.get(&offer.role) {
                Some(winner) if winner.layer >= offer.layer => shadowed.push(offer),
                Some(_) => {
                    let loser = bound.insert(offer.role, offer).expect("just matched");
                    shadowed.push(loser);
                }
                None => {
                    bound.insert(offer.role, offer);
                }
            }
        }
        Ok(Bindings { bound, shadowed })
    }

    /// What is bound to this role, if anything.
    #[must_use]
    pub fn of(&self, role: Role) -> Option<&RoleOffer> {
        self.bound.get(&role)
    }

    /// Every offer that lost, in the order the losses happened.
    #[must_use]
    pub fn shadowed(&self) -> &[RoleOffer] {
        &self.shadowed
    }

    /// Whether this role is still answered declaratively rather than by an
    /// agent. True for every unbound role, and the default for `router`.
    ///
    /// **No code path assumes the router is always declarative** — open
    /// question 1. Everything that routes asks this and takes the other branch
    /// when the answer is false.
    #[must_use]
    pub fn is_declarative(&self, role: Role) -> bool {
        self.of(role).is_none()
    }

    /// One audit line per bound role: which agent ran it, and from which layer.
    ///
    /// A role binding *is* a routing decision — the router chose which agent
    /// answers the role — so it lands in the same stream, in a stable order.
    #[must_use]
    pub fn audit_events(&self) -> Vec<AuditEvent> {
        let mut ordered: Vec<&RoleOffer> = self.bound.values().collect();
        ordered.sort_by(|a, b| role_word(a.role).cmp(role_word(b.role)));
        ordered
            .into_iter()
            .map(|offer| AuditEvent::RoutingDecision {
                chose: format!("role:{}={}", role_word(offer.role), offer.agent),
                signals: BTreeMap::from([("layer".to_owned(), layer_rank(offer.layer))]),
            })
            .collect()
    }
}

/// How the ledger prints a shadowed binding.
#[must_use]
pub fn shadowed_line(winner: &RoleOffer, loser: &RoleOffer) -> String {
    format!(
        "the `{}` role is claimed twice: `{}` at {} wins by layer precedence; \
         `{}` at {} does not take effect",
        role_word(loser.role),
        winner.agent,
        winner.origin(),
        loser.agent,
        loser.origin(),
    )
}

/// Bind an agent into a step: **intersect, never widen** (invariant 1).
///
/// The grant is narrowed through
/// [`GrantSpec::apply_to`](orrery_proto::GrantSpec::apply_to), which is the one
/// operation grants have, and the visible tool set is intersected too — a
/// binding that could add a tool would be a binding that could widen a step by
/// the back door.
#[must_use]
pub fn bind_scope(step: &AgentScope, def: &RoleAgentDef) -> AgentScope {
    let grant: Grant = def.grant.apply_to(&step.grant);
    let tools = if def.tools.is_empty() {
        step.tools.clone()
    } else {
        def.tools
            .iter()
            .filter(|t| step.tools.iter().any(|s| s == *t))
            .cloned()
            .collect()
    };
    AgentScope {
        agent: def.name.clone(),
        branch: step.branch,
        tools,
        grant,
    }
}

fn layer_rank(layer: Layer) -> f64 {
    match layer {
        Layer::Managed => 0.0,
        Layer::Org => 1.0,
        Layer::User => 2.0,
        Layer::Workspace => 3.0,
        Layer::Project => 4.0,
        _ => -1.0,
    }
}
