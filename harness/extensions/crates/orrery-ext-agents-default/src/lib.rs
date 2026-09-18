//! The shipped role agents: planner, executor, verifier, compactor and summariser.
//!
//! Implementation plan: `harness/docs/plans/11-router-roles-orchestrator.md`
//!
//! # Why this is an extension and not a built-in
//!
//! The kernel **names** the roles; it does not bind them. If the five defaults
//! were compiled into the loop there would be one path for the roles everybody
//! uses and a different one for everybody else's — and "plan with a large model
//! and compact with a cheap one" would stop being configuration.
//!
//! So the floor loads the way a third-party bundle does: an `orrery.toml`
//! parsed by the same parser, a load through the same harness, an entry in the
//! ledger, and a deny rule that switches it off. Replacing the planner is then
//! a binding in a config layer, and `roles::exactly_one_wins` decides between
//! this bundle's planner and somebody else's by ordinary layer precedence.
//!
//! # No model is named here
//!
//! An agent declares a [`ModelClass`], not a model id. What `cheap` means is a
//! profile's business, and it changes every few months; hard-coding a name in
//! the shipped floor would make it wrong on a schedule — and would put a
//! provider decision in a crate that has no business making one.
//!
//! # Every agent declares a budget
//!
//! `budget` is mandatory on an agent because an agent that cannot terminate is
//! a cost incident. These five are the shipped defaults; a profile narrows
//! them, and narrowing is the only direction available.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use async_trait::async_trait;
use orrery_ext_api::{CallCtx, HostError, NativeExtension, ToolDef};
use orrery_proto::{Aspect, Budget, Capability, Consent, ExtId, GrantSpec, Outcome, Role};
use serde_json::Value;

/// This extension's `orrery.toml`, compiled in.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// What the extension is called, everywhere.
pub const ID: &str = "agents-default";

/// How expensive a model this agent wants.
///
/// A class, not a name. The profile binds a class to a model id, which is the
/// same indirection the provider layer uses and the reason "compact with a
/// cheap one" is one line of configuration.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ModelClass {
    /// The session's model. What an agent asks for when it has no opinion.
    Default,
    /// The largest bound model: worth it for planning.
    Large,
    /// The cheapest bound model: compaction and summarising are mechanical.
    Cheap,
}

impl ModelClass {
    /// The word a profile binds.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ModelClass::Default => "default",
            ModelClass::Large => "large",
            ModelClass::Cheap => "cheap",
        }
    }
}

/// Which permission set the agent's step runs under.
///
/// Mirrors `orrery-router`'s `Mode` without depending on it: an extension
/// depends on core only through **published** crates, and the router is not one
/// (`cargo xtask deps-check`, rule 2). The word is the contract, and it is the
/// word a `mode(...)` rule is written with.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ModeWord {
    /// Read-only.
    Plan,
    /// The full granted set.
    Execute,
    /// Reads and reports; never writes.
    Review,
}

impl ModeWord {
    /// The word itself.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ModeWord::Plan => "plan",
            ModeWord::Execute => "execute",
            ModeWord::Review => "review",
        }
    }
}

/// One shipped role agent.
#[derive(Clone, Debug, PartialEq)]
pub struct RoleAgent {
    /// Its name, which is what a `[roles]` line binds.
    pub name: &'static str,
    /// The role it answers.
    pub role: Role,
    /// The mode its step runs in.
    pub mode: ModeWord,
    /// How expensive a model it wants.
    pub model: ModelClass,
    /// Its prompt.
    pub prompt: &'static str,
    /// What it asks for. **Narrowing only**: intersected with the step's grant,
    /// never substituted for it.
    pub grant: GrantSpec,
    /// What it may spend. Mandatory.
    pub budget: Budget,
}

fn budget(max_turns: u32, max_tokens: u64) -> Budget {
    Budget {
        max_turns,
        max_tokens,
        wall_clock_ms: 300_000,
        max_micro_usd: None,
    }
}

fn asking(aspects: &[Aspect]) -> GrantSpec {
    GrantSpec {
        capabilities: Some(aspects.iter().copied().map(Capability::all).collect()),
        // Ask once. A shipped default cannot grant itself silence.
        consent: Some(Consent::Once),
    }
}

/// The five shipped agents, in the order the manifest promises them.
#[must_use]
pub fn agents() -> Vec<RoleAgent> {
    vec![
        RoleAgent {
            name: "planner",
            role: Role::Planner,
            mode: ModeWord::Plan,
            model: ModelClass::Large,
            prompt: "Work out what to change and say so. Do not change anything.",
            // Read-only, and that is the mode's ceiling as well as this
            // declaration's: two independent reasons a planner cannot write.
            grant: asking(&[Aspect::Read, Aspect::MemRead, Aspect::Tool]),
            budget: budget(8, 120_000),
        },
        RoleAgent {
            name: "executor",
            role: Role::Executor,
            mode: ModeWord::Execute,
            model: ModelClass::Default,
            prompt: "Carry out the plan. Change one thing at a time.",
            // Inherits: the granted set is the step's, and an executor's job is
            // to use it rather than to narrow it.
            grant: GrantSpec::default(),
            budget: budget(20, 400_000),
        },
        RoleAgent {
            name: "verifier",
            role: Role::Verifier,
            mode: ModeWord::Review,
            model: ModelClass::Default,
            prompt: "Run the check and report what it said. Do not fix anything.",
            // It spawns — a verifier that cannot run the tests is not a
            // verifier — and it never writes.
            grant: asking(&[Aspect::Read, Aspect::Spawn, Aspect::Tool]),
            budget: budget(4, 80_000),
        },
        RoleAgent {
            name: "compactor",
            role: Role::Compactor,
            mode: ModeWord::Review,
            model: ModelClass::Cheap,
            prompt: "Shrink this transcript. Keep every decision and every open question.",
            grant: asking(&[Aspect::Read]),
            budget: budget(1, 40_000),
        },
        RoleAgent {
            name: "summariser",
            role: Role::Summariser,
            mode: ModeWord::Review,
            model: ModelClass::Cheap,
            prompt: "Say what happened, in one paragraph.",
            grant: asking(&[Aspect::Read]),
            budget: budget(1, 20_000),
        },
    ]
}

/// The one this extension ships for a role, if it ships one.
///
/// `router` and `grader` are deliberately absent: leaving `router` unbound is
/// what keeps routing declarative and reproducible, and a grader belongs to
/// the eval suite that defines what a good answer is.
#[must_use]
pub fn for_role(role: Role) -> Option<RoleAgent> {
    agents().into_iter().find(|a| a.role == role)
}

/// The names the manifest promises, in order.
#[must_use]
pub fn promised() -> Vec<String> {
    agents().iter().map(|a| a.name.to_owned()).collect()
}

/// The shipped role agents, as an extension.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultAgents;

impl DefaultAgents {
    /// What it contributes.
    #[must_use]
    pub fn agents(&self) -> Vec<RoleAgent> {
        agents()
    }
}

#[async_trait]
impl NativeExtension for DefaultAgents {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn manifest_path(&self) -> &str {
        "harness/extensions/crates/orrery-ext-agents-default/orrery.toml"
    }

    /// None. This extension contributes **agents**, and an agent is not a tool:
    /// it is a configuration of the loop, never something the model calls.
    fn tools(&self) -> Vec<ToolDef> {
        Vec::new()
    }

    async fn call(&self, tool: &str, _input: Value, _ctx: &CallCtx) -> Result<Outcome, HostError> {
        Err(HostError::NoSuchTool {
            ext: ExtId::new(ID).expect("a literal ext id"),
            tool: tool.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{DefaultAgents, MANIFEST, agents, for_role, promised};
    use orrery_ext_api::ExtensionManifest;
    use orrery_proto::Role;

    /// What the manifest promises is what the code contributes.
    #[test]
    fn the_manifest_and_the_code_agree() {
        let manifest = ExtensionManifest::from_toml_str(MANIFEST, "orrery.toml").expect("parses");
        assert_eq!(manifest.provides.agents, promised());
        assert_eq!(DefaultAgents.agents().len(), 5);
    }

    /// Every agent declares a budget, because an agent that cannot terminate is
    /// a cost incident.
    #[test]
    fn every_agent_declares_a_budget() {
        for agent in agents() {
            assert!(agent.budget.max_turns > 0, "{} has no turn cap", agent.name);
            assert!(
                agent.budget.max_tokens > 0,
                "{} has no token cap",
                agent.name
            );
        }
    }

    /// `router` is unbound. That is the default, and it is what eval comparison
    /// needs.
    #[test]
    fn the_router_role_is_not_shipped_bound() {
        assert!(for_role(Role::Router).is_none());
        assert!(for_role(Role::Grader).is_none());
        assert!(for_role(Role::Planner).is_some());
    }
}
