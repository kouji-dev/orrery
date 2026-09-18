//! How loading an extension went.
//!
//! An extension that fails to load is a *reportable state*, not a panic and not
//! a silent absence: the session still starts, the user is told what is missing
//! and what it cost them, and `query extensions` can answer for it afterwards.

use serde::{Deserialize, Serialize};

use crate::ids::ExtId;

/// What an extension added to the session.
///
/// `kind` is a plain enum here. The derive macro that keeps it in step with the
/// extension's manifest — so that a manifest promising a tool and a crate
/// contributing a renderer is a build error rather than a surprise — is the
/// extension host's (plan 06).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Contribution {
    /// What kind of thing it is.
    pub kind: ContributionKind,
    /// Its name, unqualified: the extension is already known.
    pub name: String,
}

/// The kinds of thing an extension can contribute.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ContributionKind {
    /// A tool the model can call.
    Tool,
    /// A model provider.
    Provider,
    /// A renderer for a custom surface kind.
    Renderer,
    /// A slash command.
    Command,
    /// A skill.
    Skill,
    /// A memory store.
    Memory,
    /// A grader for the eval runner.
    Grader,
    /// A routing policy.
    Router,
    /// A session store.
    SessionStore,
    /// A mode.
    Mode,
    /// A named view over the session.
    View,
}

/// How far loading got before it stopped.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LoadStage {
    /// Reading and parsing the manifest.
    Manifest,
    /// Resolving its dependencies and its version requirements.
    Resolve,
    /// Linking the code: dynamic library, wasm module, child process.
    Link,
    /// Running its activation.
    Activate,
    /// Registering what it contributes.
    Contribute,
}

/// Why an extension was not loaded at all.
///
/// A **closed** set. "It did not load" is a thing a user has to be able to act
/// on, and free text cannot be grouped, counted or matched against a rule.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    /// Turned off by configuration.
    Disabled,
    /// Not built for this platform or this harness version.
    Unsupported,
    /// Something it depends on is not there.
    DependencyMissing,
    /// Policy refused it.
    PolicyDenied,
    /// Another copy is already loaded.
    AlreadyLoaded,
}

/// The result of trying to load one extension.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum LoadOutcome {
    /// It loaded and did everything it promised.
    Ok {
        /// Which extension.
        ext: ExtId,
        /// What it added.
        #[serde(default)]
        contributions: Vec<Contribution>,
        /// How long it took.
        ms: u64,
    },
    /// It loaded, and something it promised is missing.
    ///
    /// The interesting case, and the reason this is an enum rather than a
    /// `Result`: half an extension is usable, and saying which half is what
    /// stops a user hunting for a tool that quietly never registered.
    Degraded {
        /// Which extension.
        ext: ExtId,
        /// What it did add.
        #[serde(default)]
        contributions: Vec<Contribution>,
        /// How long it took.
        ms: u64,
        /// What is missing, in words a person can act on.
        #[serde(default)]
        problems: Vec<String>,
    },
    /// It was not tried.
    Skipped {
        /// Which extension.
        ext: ExtId,
        /// Why not.
        reason: SkipReason,
    },
    /// It was tried and it did not work.
    Failed {
        /// Which extension.
        ext: ExtId,
        /// How far it got.
        stage: LoadStage,
        /// What went wrong.
        message: String,
    },
}

impl LoadOutcome {
    /// Which extension this is about.
    #[must_use]
    pub fn ext(&self) -> &ExtId {
        match self {
            LoadOutcome::Ok { ext, .. }
            | LoadOutcome::Degraded { ext, .. }
            | LoadOutcome::Skipped { ext, .. }
            | LoadOutcome::Failed { ext, .. } => ext,
        }
    }

    /// What it contributed, which is nothing when it did not load.
    #[must_use]
    pub fn contributions(&self) -> &[Contribution] {
        match self {
            LoadOutcome::Ok { contributions, .. } | LoadOutcome::Degraded { contributions, .. } => {
                contributions
            }
            LoadOutcome::Skipped { .. } | LoadOutcome::Failed { .. } => &[],
        }
    }
}
