//! The one error this crate can hand back.

use std::path::{Path, PathBuf};

/// Opening a sink went wrong. Appending never does: see [`crate::FileSink::errors`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuditError {
    /// The audit file could not be opened for append.
    #[error("audit sink `{path}` could not be opened: {source}")]
    Sink {
        /// Which file.
        path: PathBuf,
        /// What the operating system said.
        #[source]
        source: std::io::Error,
    },
}

impl AuditError {
    pub(crate) fn sink(path: &Path, source: std::io::Error) -> Self {
        AuditError::Sink {
            path: path.to_path_buf(),
            source,
        }
    }
}
