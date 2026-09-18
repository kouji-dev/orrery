//! What can go wrong with a skill, and what deliberately cannot.
//!
//! A **denial is not in here**. A script whose effect falls outside its grant
//! comes back as [`EffectOutcome::Denied`](crate::scripts::EffectOutcome), the
//! same way a refused tool call comes back as `Outcome::Denied` rather than an
//! error. What is in here is the harness failing: a file that is not a skill, a
//! script that was asked for without a grant, a broker that could not start a
//! process.

use std::path::{Path, PathBuf};

/// A skill that could not be loaded or run.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    /// The file has no YAML front matter at all.
    #[error("`{path}` is not a SKILL.md: it has no `---` front matter")]
    MissingFrontMatter {
        /// Which file.
        path: PathBuf,
    },
    /// The front matter parsed but carries no `name`.
    #[error("`{path}` has front matter but no `name`")]
    MissingName {
        /// Which file.
        path: PathBuf,
    },
    /// The front matter is not YAML, or is not a mapping.
    #[error("`{path}` has front matter that is not a YAML mapping: {message}")]
    BadFrontMatter {
        /// Which file.
        path: PathBuf,
        /// What the YAML parser said.
        message: String,
    },
    /// The file could not be read.
    #[error("`{path}` could not be read: {message}")]
    Io {
        /// Which file.
        path: PathBuf,
        /// What the filesystem said.
        message: String,
    },
    /// A script was asked for on a skill that declares no grant.
    ///
    /// The whole of task 3: elsewhere a bundled script inherits the user's
    /// shell. Here, no grant means no scripts at all.
    #[error(
        "skill `{skill}` declares no grant, so its scripts cannot run — \
         declare one in configuration under `[skills.{skill}]`"
    )]
    NoGrant {
        /// Which skill.
        skill: String,
    },
    /// The named script is not in the skill's `scripts/` directory.
    #[error("skill `{skill}` has no script `{script}`")]
    NoSuchScript {
        /// Which skill.
        skill: String,
        /// What was asked for.
        script: String,
    },
    /// Policy refused to let the script start at all.
    ///
    /// Distinct from an effect being denied: the process never existed, so
    /// there is no run to report effects against.
    #[error("skill `{skill}` may not run `{script}`: {reason}")]
    SpawnDenied {
        /// Which skill.
        skill: String,
        /// What was asked for.
        script: String,
        /// Why, in words a person can act on.
        reason: String,
    },
    /// The broker could not carry the call.
    #[error("skill `{skill}`: {message}")]
    Broker {
        /// Which skill.
        skill: String,
        /// What the broker said.
        message: String,
    },
}

impl SkillError {
    /// An I/O failure against a path.
    #[must_use]
    pub fn io(path: impl AsRef<Path>, e: &std::io::Error) -> Self {
        SkillError::Io {
            path: path.as_ref().to_path_buf(),
            message: e.to_string(),
        }
    }
}
