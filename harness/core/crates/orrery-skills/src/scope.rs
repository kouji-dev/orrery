//! A skill as the rest of the harness sees it, and which agents may load it.
//!
//! # Both additions are configuration, not front matter
//!
//! Open question 2 of the plan, decided: **config-side scoping only.** Neither
//! the agent scope nor the script grant is written in `SKILL.md`. A sidecar
//! `skill.toml` or a reserved front-matter key would both mean that a skill
//! governed here is a different file from the skill published there, and the
//! adoption promise is that it is the same file. So [`SkillSettings`] is read
//! from the config layers and joined to the parsed document by name.
//!
//! The consequence is honest and worth stating: a skill acquires its grant from
//! the operator, never from itself. A skill cannot ask for privileges, which is
//! the right direction for a document that is easy to publish and easy to trust
//! by mistake.

use std::path::{Path, PathBuf};

use orrery_proto::{AgentScope, GrantSpec, Layer};
use serde::{Deserialize, Serialize};

use crate::parse::SkillDoc;

/// Where a skill came from.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillSource {
    /// Shipped with the harness, or with the managed/org layer above it.
    Builtin,
    /// This user, across every workspace.
    User,
    /// This workspace or project.
    Workspace,
    /// Contributed by an extension.
    Extension,
    /// Installed from the signed registry.
    Registry,
}

impl SkillSource {
    /// Which source a config layer contributes.
    ///
    /// `Project` and `Workspace` are both `Workspace` here: the distinction is
    /// a config-layer one and the skill is the same kind of thing either way.
    #[must_use]
    pub const fn of_layer(layer: Layer) -> Self {
        match layer {
            Layer::Managed | Layer::Org => SkillSource::Builtin,
            Layer::User => SkillSource::User,
            Layer::Workspace | Layer::Project => SkillSource::Workspace,
            // `Layer` is `#[non_exhaustive]`: a layer this crate has not been
            // taught about is the furthest away, not the closest.
            _ => SkillSource::Builtin,
        }
    }
}

/// What configuration says about one skill.
///
/// Both fields default to the closed answer: no agent scope means *every* agent
/// (a skill is documentation, and hiding it by default helps nobody), and no
/// grant means **no scripts at all**.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSettings {
    /// Which agents may load it. Empty means all of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
    /// What its `scripts/` may do. Absent means they may not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<GrantSpec>,
}

impl SkillSettings {
    /// Limit this skill to a set of agents.
    #[must_use]
    pub fn for_agents(mut self, agents: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.scope = agents.into_iter().map(Into::into).collect();
        self
    }

    /// Declare what the skill's scripts may do.
    #[must_use]
    pub fn granting(mut self, grant: GrantSpec) -> Self {
        self.grant = Some(grant);
        self
    }
}

/// One skill, ready to be offered to an agent.
///
/// The plan sketches `scope: Vec<AgentScope>`. An [`AgentScope`] is a whole
/// running sub-agent — a branch id, a tool set, a grant — and a list of them
/// hung off every skill would be a copy of state that lives elsewhere and goes
/// stale. The field here is the list of agent **names** the skill is offered to,
/// which is what [`SkillRef::visible_to`] actually needs.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillRef {
    /// The `name` from the front matter.
    pub name: String,
    /// The `description` from the front matter.
    pub description: String,
    /// Where it came from.
    pub source: SkillSource,
    /// The config layer that contributed it.
    pub layer: Layer,
    /// The `SKILL.md` itself.
    pub path: PathBuf,
    /// Which agents may load it. Empty means all of them.
    pub scope: Vec<String>,
    /// What its `scripts/` run under. `None` means they do not run.
    pub grant: Option<GrantSpec>,
}

impl SkillRef {
    /// Join a parsed document to what configuration says about it.
    #[must_use]
    pub fn new(doc: &SkillDoc, layer: Layer, settings: &SkillSettings) -> Self {
        Self {
            name: doc.name.clone(),
            description: doc.description.clone(),
            source: SkillSource::of_layer(layer),
            layer,
            path: doc.path.clone(),
            scope: settings.scope.clone(),
            grant: settings.grant.clone(),
        }
    }

    /// Whether this agent may load it.
    ///
    /// An empty scope is every agent. A named scope is exact: a skill offered
    /// to `reviewer` is **absent from** `executor`'s context, not merely
    /// discouraged there.
    #[must_use]
    pub fn visible_to(&self, agent: &str) -> bool {
        self.scope.is_empty() || self.scope.iter().any(|a| a == agent)
    }

    /// Whether this scope's agent may load it.
    #[must_use]
    pub fn in_scope(&self, scope: &AgentScope) -> bool {
        self.visible_to(&scope.agent)
    }

    /// The directory the skill lives in — where its `scripts/` are.
    #[must_use]
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    /// The path of one of its bundled scripts.
    #[must_use]
    pub fn script_path(&self, script: &str) -> PathBuf {
        self.dir().join("scripts").join(script)
    }
}
