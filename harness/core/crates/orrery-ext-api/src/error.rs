//! What can go wrong that is *not* a refusal.
//!
//! As in `orrery-tools`: there is no `Denied` variant here and there never will
//! be. A denial is [`Outcome::Denied`](orrery_proto::Outcome), a value, returned
//! `Ok`. This type is for the cases where the harness itself could not carry the
//! call — the child is gone, the guest answered nonsense, the extension is not
//! loaded any more.

use orrery_proto::{ExtId, LoadStage};

/// The harness could not carry the call.
#[non_exhaustive]
#[derive(Clone, Debug, thiserror::Error)]
pub enum HostError {
    /// No extension of that name is loaded.
    ///
    /// Note that a *stale reference to an extension that was unloaded* is not
    /// this: that is `Ok(Outcome::Unloaded)`, because the model can re-plan
    /// around it. This is a reference to something that never existed.
    #[error("no extension `{ext}` is loaded")]
    NotLoaded {
        /// Which extension.
        ext: ExtId,
    },
    /// The extension is loaded and does not have that tool.
    #[error("extension `{ext}` has no tool `{tool}`")]
    NoSuchTool {
        /// Which extension.
        ext: ExtId,
        /// Which tool it does not have.
        tool: String,
    },
    /// The guest could not be reached, or answered something unusable.
    #[error("extension `{ext}`: {message}")]
    Transport {
        /// Which extension.
        ext: ExtId,
        /// What went wrong.
        message: String,
    },
    /// Loading failed, and this is how far it got.
    #[error("extension `{ext}` failed at {stage:?}: {message}")]
    Load {
        /// Which extension.
        ext: ExtId,
        /// How far it got.
        stage: LoadStage,
        /// What went wrong.
        message: String,
    },
    /// The extension's own code panicked or returned something impossible.
    ///
    /// A panicking `PermissionHandler` fails **closed** and a `SessionStore`
    /// failure ends the session; every other panic degrades the one extension.
    /// The decision is the host's, not this type's.
    #[error("extension `{ext}` misbehaved: {message}")]
    Misbehaved {
        /// Which extension.
        ext: ExtId,
        /// What it did.
        message: String,
    },
}

impl HostError {
    /// Which extension the problem is with.
    #[must_use]
    pub fn ext(&self) -> &ExtId {
        match self {
            HostError::NotLoaded { ext }
            | HostError::NoSuchTool { ext, .. }
            | HostError::Transport { ext, .. }
            | HostError::Load { ext, .. }
            | HostError::Misbehaved { ext, .. } => ext,
        }
    }
}
