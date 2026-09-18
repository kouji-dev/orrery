//! The kernel-side surface differ, its validation, the per-turn store and the seal at turn.settled.
//!
//! Implementation plan: `harness/docs/plans/09-surfaces.md`
//!
//! # The pipeline
//!
//! ```text
//! extension ctx.ui.*  →  validate + policy  →  diff vs last surface  →  SurfacePatch  →  renderer
//! ```
//!
//! An extension re-emits its **whole** surface every time. The kernel is what
//! works out the difference, so extension code stays trivial and the wire stays
//! small. Nothing here draws: a [`Surface`](orrery_proto::Surface) is a
//! description, and which pixels it becomes is the client's business.
//!
//! # What lives where
//!
//! - [`validate`] — the rules that can be checked without knowing the client.
//! - [`hash`] — per-node blake3, so an unchanged subtree is skipped in O(1).
//! - [`diff`] — the differ, the append fast path and the cost guard.
//! - [`store`] — per-turn storage, and the seal at `turn.settled`.
//! - [`sink`] — the builders an extension describes surfaces with.
//! - [`view`] — binding loop events to surfaces, and the profile that moves
//!   them. Re-exported from `orrery-ext-api`: contributing a view is an
//!   extension's job, so the vocabulary ships in the published crate and the
//!   kernel borrows it rather than the other way round.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod diff;
pub mod hash;
pub mod sink;
pub mod store;
pub mod validate;

pub use orrery_ext_api::view;

pub use diff::{COST_GUARD, DiffCost, apply, diff};
pub use hash::{HashCounter, HashTree, NodeHash, hash_tree};
pub use sink::SurfaceBuilders;
pub use store::SurfaceStore;
pub use validate::{MAX_DEPTH, Warning, validate};
pub use view::{
    EventKind, LoopEvent, Placed, Placement, Predicate, ProfileError, ViewBinding, ViewRegistry,
    floor, floor_kinds,
};

use orrery_proto::{SurfaceId, TurnId};

/// What can go wrong with a surface, on this side of the wire.
///
/// [`orrery_proto::SurfaceError`] covers what is wrong with a surface's
/// *shape*; this covers what is wrong with emitting it — which needs a turn and
/// a store to be wrong about, and so cannot live in the types crate.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SurfaceError {
    /// The surface itself is not well-formed.
    #[error(transparent)]
    Malformed(#[from] orrery_proto::SurfaceError),

    /// Nested deeper than any renderer will draw.
    #[error(
        "a surface nested {depth} deep is past the {max} the renderers agree on; \
         no client would draw it, so it is refused here rather than there"
    )]
    TooDeep {
        /// How deep it went.
        depth: usize,
        /// How deep it may go.
        max: usize,
    },

    /// The turn is over. Reported **to the extension**; no frame is produced.
    #[error(
        "turn {turn} has settled, so surface {surface} is sealed: a client that \
         has closed the turn cannot apply a patch to it"
    )]
    Sealed {
        /// Which turn.
        turn: TurnId,
        /// Which surface the extension tried to patch.
        surface: SurfaceId,
    },
}

/// What a patch could not be applied to.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplyError {
    /// The path names a field that is not there.
    #[error("no field at `/{}`", path.join("/"))]
    NoSuchField {
        /// The path that missed.
        path: Vec<String>,
    },
    /// The result of the write is not a surface any more.
    #[error("applying the patch left something that is not a surface: {message}")]
    NotASurface {
        /// What serde said.
        message: String,
    },
    /// `append` against a variant that has no text to append to.
    #[error("`append` needs a text or markdown surface, and this one is `{kind}`")]
    NotAppendable {
        /// The variant's tag.
        kind: String,
    },
    /// `remove` addresses a whole surface, which only a store can take away.
    #[error("`remove` takes a surface out of a store; there is nothing to apply it to here")]
    Removed,
}
