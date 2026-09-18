//! Who is acting, where a name came from, and what a sub-agent may see.

use serde::{Deserialize, Serialize};

use crate::grant::Grant;
use crate::ids::{BranchId, ExtId};

/// What a model is being asked to be, in this pass.
///
/// A role is not a permission — it selects a prompt, a model and a tool set.
/// What is *allowed* is the [`Grant`].
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// Decides what to do.
    Planner,
    /// Does it.
    Executor,
    /// Checks that it was done.
    Verifier,
    /// Shrinks a transcript that no longer fits.
    Compactor,
    /// Writes the summary a compaction leaves behind.
    Summariser,
    /// Picks the model or the sub-agent.
    Router,
    /// Scores a run for the eval runner.
    Grader,
}

/// Where a piece of configuration came from.
///
/// # The ordering is one-directional, on purpose
///
/// `Managed` is **lowest** and `Project` **highest**, so `max()` reads as
/// "closest layer wins", which is how *names* resolve: a project-level agent
/// shadows an org-level one of the same name.
///
/// Permission does **not** work that way. A `managed` deny is final and no
/// closer layer can lift it, so a policy decision folds by *severity*, not by
/// this ordering. Two directions, deliberately; do not reach for `max()` when
/// the question is "may I".
#[non_exhaustive]
#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Layer {
    /// Shipped by whoever administers the install. Furthest away, and the only
    /// layer whose denials are final.
    Managed,
    /// The organisation.
    Org,
    /// This user, across every workspace.
    User,
    /// This workspace.
    Workspace,
    /// This project, inside the workspace. Closest.
    Project,
}

/// What a sub-agent is: a name, a branch of the turn tree, the tools it can
/// see, and what it may do with them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentScope {
    /// The agent's name.
    pub agent: String,
    /// The branch it runs on.
    pub branch: BranchId,
    /// The visible tool set. This is the set, not a suggestion: a tool that is
    /// not named here is not offered to the model at all.
    #[serde(default)]
    pub tools: Vec<String>,
    /// What it may do.
    pub grant: Grant,
}

/// Who a grant belongs to.
///
/// Serialises to and from the string forms `"agent"`, `"ext:<id>"` and
/// `"agent:<name>"`.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Subject {
    /// The main agent.
    Agent,
    /// An extension.
    Ext(ExtId),
    /// A named sub-agent.
    SubAgent(String),
}

impl std::fmt::Display for Subject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Subject::Agent => f.write_str("agent"),
            Subject::Ext(ext) => write!(f, "ext:{ext}"),
            Subject::SubAgent(name) => write!(f, "agent:{name}"),
        }
    }
}

/// A string that is not a [`Subject`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{value}` is not a subject: expected `agent`, `ext:<id>` or `agent:<name>`")]
pub struct SubjectError {
    /// The offending string.
    pub value: String,
}

impl std::str::FromStr for Subject {
    type Err = SubjectError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || SubjectError {
            value: s.to_owned(),
        };
        match s.split_once(':') {
            None if s == "agent" => Ok(Subject::Agent),
            None => Err(bad()),
            Some(("ext", id)) => ExtId::new(id).map(Subject::Ext).map_err(|_| bad()),
            Some(("agent", name)) if !name.is_empty() => Ok(Subject::SubAgent(name.to_owned())),
            Some(_) => Err(bad()),
        }
    }
}

impl Serialize for Subject {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Subject {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = String::deserialize(d)?;
        raw.parse().map_err(D::Error::custom)
    }
}

impl schemars::JsonSchema for Subject {
    fn schema_name() -> String {
        "Subject".to_owned()
    }

    fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        schemars::schema::SchemaObject {
            instance_type: Some(schemars::schema::InstanceType::String.into()),
            string: Some(Box::new(schemars::schema::StringValidation {
                pattern: Some("^(agent|ext:[a-z0-9.-]+|agent:.+)$".to_owned()),
                ..Default::default()
            })),
            ..Default::default()
        }
        .into()
    }
}
