//! Shared fixtures. Nothing here reaches a network, a model or a paid API.

#![allow(dead_code)]

use orrery_memory::{Actor, LifecycleCtx, LifecyclePoint, MemEntry, Recall};
use orrery_proto::{AgentScope, BranchId, Consent, Grant, SessionId, Subject, TokenBudget};

/// A lifecycle context, as a handler at `turn.end` would hold one. The only
/// thing in the tree that can produce a `LifecycleWitness`.
#[must_use]
pub fn lifecycle(session: SessionId, branch: BranchId) -> LifecycleCtx {
    LifecycleCtx::at(LifecyclePoint::TurnEnd, session, branch)
}

/// An unqualified grant: the rules narrow, the scope does not.
#[must_use]
pub fn open_scope(agent: &str, branch: BranchId) -> AgentScope {
    AgentScope {
        agent: agent.to_owned(),
        branch,
        tools: Vec::new(),
        grant: Grant {
            capabilities: Vec::new(),
            consent: Consent::Always,
        },
    }
}

/// The main agent: one branch, no ancestors, allowed to write as wide as its
/// rules permit.
#[must_use]
pub fn root_actor(session: SessionId, branch: BranchId) -> Actor {
    Actor::root(Subject::Agent, open_scope("agent", branch), session, branch)
}

/// A sub-agent on its own branch, under `parents`.
#[must_use]
pub fn child_actor(
    name: &str,
    session: SessionId,
    parents: &[BranchId],
    branch: BranchId,
) -> Actor {
    Actor::sub_agent(
        Subject::SubAgent(name.to_owned()),
        open_scope(name, branch),
        session,
        parents.iter().copied().chain(std::iter::once(branch)),
    )
}

#[must_use]
pub fn entry(key: &str, text: &str) -> MemEntry {
    MemEntry::new(key, text)
}

#[must_use]
pub const fn budget(max: u64) -> TokenBudget {
    TokenBudget { max, reserve: 0 }
}

/// Every text a recall produced, in order, across providers.
#[must_use]
pub fn texts(recalls: &[Recall]) -> Vec<String> {
    recalls
        .iter()
        .flat_map(|r| r.entries.iter().map(|e| e.text.clone()))
        .collect()
}
