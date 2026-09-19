//! A case: a workspace, a prompt, a grader and a ceiling.
//!
//! A suite is just a TOML document listing them, which is what lets a private
//! suite over a team's own monorepo install from the registry like any other
//! extension and need no special treatment.

use std::collections::BTreeMap;
use std::path::PathBuf;

use orrery_proto::Budget;
use serde::{Deserialize, Serialize};

use crate::adapter::{AdapterEntry, AdapterSpec};
use crate::error::EvalError;

/// Where a case's workspace comes from.
///
/// A case never runs in the repository it was launched from: it runs in a copy
/// that [`crate::isolate`] made for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WorkspaceSpec {
    /// A git repository at a commit. Isolated with a worktree.
    Repo {
        /// The repository to fork a worktree from.
        repo: PathBuf,
        /// The commit to check out. `None` takes whatever `HEAD` is, which is
        /// reproducible only for as long as `HEAD` does not move — so a suite
        /// that means to be reproducible pins it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commit: Option<String>,
    },
    /// A directory of files, copied in. No git required.
    Fixture {
        /// The directory to copy.
        archive: PathBuf,
    },
    /// Nothing. The case starts in an empty directory.
    Empty,
}

impl WorkspaceSpec {
    /// Whether this spec needs git to isolate.
    #[must_use]
    pub const fn needs_git(&self) -> bool {
        matches!(self, WorkspaceSpec::Repo { .. })
    }
}

/// Which grader scores this case, and how it is configured.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraderSpec {
    /// The grader's id, as the extension publishes it: `command`, `assertion`,
    /// `model`.
    pub grader: String,
    /// Whatever that grader reads. Opaque here on purpose — the runner must not
    /// know what an assertion looks like.
    #[serde(default)]
    pub config: serde_json::Value,
}

impl GraderSpec {
    /// Name a grader with no configuration.
    #[must_use]
    pub fn new(grader: impl Into<String>) -> Self {
        Self {
            grader: grader.into(),
            config: serde_json::Value::Null,
        }
    }

    /// Name a grader with configuration.
    #[must_use]
    pub fn with_config(mut self, config: serde_json::Value) -> Self {
        self.config = config;
        self
    }
}

/// One thing to try.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvalCase {
    /// The id it is reported and replayed by.
    pub id: String,
    /// Where it runs.
    pub workspace: WorkspaceSpec,
    /// What the agent is asked to do.
    pub prompt: String,
    /// How it is scored.
    pub grade: GraderSpec,
    /// What it may spend. Zero in any field means unmetered, as everywhere
    /// else in the harness.
    #[serde(default)]
    pub budget: Budget,
}

impl EvalCase {
    /// A case with an empty workspace and no ceiling.
    #[must_use]
    pub fn new(id: impl Into<String>, prompt: impl Into<String>, grade: GraderSpec) -> Self {
        Self {
            id: id.into(),
            workspace: WorkspaceSpec::Empty,
            prompt: prompt.into(),
            grade,
            budget: Budget::default(),
        }
    }

    /// Run it somewhere in particular.
    #[must_use]
    pub fn in_workspace(mut self, workspace: WorkspaceSpec) -> Self {
        self.workspace = workspace;
        self
    }

    /// Put a ceiling on it.
    #[must_use]
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }
}

/// A named list of cases, and the competing harnesses to stand beside them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Suite {
    /// What the suite is called. `orrery eval run <name>`.
    pub name: String,
    /// Its cases, in the order they were written.
    #[serde(default, rename = "case")]
    pub cases: Vec<EvalCase>,
    /// The external agent CLIs this suite runs its cases against as well,
    /// one `[adapter.<id>]` block each.
    ///
    /// A `BTreeMap` rather than a list, so that the section is keyed by the id
    /// the way TOML writes it and so that two runs of the same suite expand
    /// their competitors in the same order.
    #[serde(default, rename = "adapter")]
    pub adapters: BTreeMap<String, AdapterEntry>,
}

impl Suite {
    /// An empty suite.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cases: Vec::new(),
            adapters: BTreeMap::new(),
        }
    }

    /// Declare a competing harness.
    #[must_use]
    pub fn with_adapter(mut self, id: impl Into<String>, entry: AdapterEntry) -> Self {
        self.adapters.insert(id.into(), entry);
        self
    }

    /// The ids of the competitors, in the order the matrix will add them.
    #[must_use]
    pub fn adapter_ids(&self) -> Vec<&str> {
        self.adapters.keys().map(String::as_str).collect()
    }

    /// Every competitor's full spec.
    ///
    /// # Errors
    ///
    /// [`EvalError::Adapter`] when a block names an unknown id and carries no
    /// `command`.
    pub fn adapter_specs(&self) -> Result<Vec<AdapterSpec>, EvalError> {
        self.adapters
            .iter()
            .map(|(id, entry)| entry.resolve(id))
            .collect()
    }

    /// Add a case.
    #[must_use]
    pub fn with_case(mut self, case: EvalCase) -> Self {
        self.cases.push(case);
        self
    }

    /// Parse a suite document.
    ///
    /// # Errors
    ///
    /// [`EvalError::Parse`] naming what TOML disliked.
    pub fn from_toml(text: &str) -> Result<Self, EvalError> {
        toml::from_str(text).map_err(|e| EvalError::Parse {
            what: "suite",
            detail: e.to_string(),
        })
    }

    /// Find one case by id.
    #[must_use]
    pub fn case(&self, id: &str) -> Option<&EvalCase> {
        self.cases.iter().find(|c| c.id == id)
    }
}
