//! One error type, and it always names a file and a line when it has one.

use std::path::{Path, PathBuf};

/// Something went wrong reading, merging or validating configuration.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// A file could not be read.
    #[error("{file}: {source}")]
    Io {
        /// Which file.
        file: PathBuf,
        /// What the OS said.
        source: std::io::Error,
    },

    /// A file did not parse as TOML.
    #[error("{file}:{line}: {message}")]
    Syntax {
        /// Which file.
        file: PathBuf,
        /// Which line, 1-based; zero when the parser gave no span.
        line: u32,
        /// What is wrong.
        message: String,
    },

    /// The configuration parsed but says something that cannot be honoured.
    #[error("{file}:{line}: {message}")]
    Invalid {
        /// Which file.
        file: PathBuf,
        /// Which line, 1-based.
        line: u32,
        /// What is wrong, in words a person can act on.
        message: String,
    },

    /// `--profile <name>` named something no layer defines.
    ///
    /// Deliberately **not** an [`ConfigError::Invalid`]: for several rounds it
    /// was, reported against `config.toml:0` — a bare filename that is not a
    /// path anybody can open and a line number that is not a line. There is no
    /// file to name here, because the profile is missing from *every* layer, so
    /// this names none and says what to write instead.
    #[error("no profile named `{name}` is defined in any layer. {advice}")]
    NoSuchProfile {
        /// What was asked for.
        name: String,
        /// What to do about it: the profiles there are, or how to define one.
        advice: String,
    },

    /// A rule did not parse, or a layer would not compile.
    #[error(transparent)]
    Policy(#[from] orrery_policy::PolicyError),

    /// The trust store would live somewhere a project could write to.
    #[error("the trust store must live outside every project-writable path, and `{path}` does not")]
    TrustStoreInsideProject {
        /// Where it was going to be written.
        path: PathBuf,
    },
}

impl ConfigError {
    /// An [`ConfigError::Invalid`] at a file and a line.
    #[must_use]
    pub fn invalid(file: impl AsRef<Path>, line: u32, message: impl Into<String>) -> Self {
        ConfigError::Invalid {
            file: file.as_ref().to_path_buf(),
            line,
            message: message.into(),
        }
    }

    /// The file this is about, when it is about one.
    #[must_use]
    pub fn file(&self) -> Option<&Path> {
        match self {
            ConfigError::Io { file, .. }
            | ConfigError::Syntax { file, .. }
            | ConfigError::Invalid { file, .. }
            | ConfigError::TrustStoreInsideProject { path: file } => Some(file),
            ConfigError::Policy(_) | ConfigError::NoSuchProfile { .. } => None,
        }
    }

    /// The line this is about, when it is about one.
    #[must_use]
    pub fn line(&self) -> Option<u32> {
        match self {
            ConfigError::Syntax { line, .. } | ConfigError::Invalid { line, .. } => Some(*line),
            _ => None,
        }
    }
}
