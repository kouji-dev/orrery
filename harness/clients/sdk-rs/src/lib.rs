//! The Rust Orrery client SDK: an AG-UI session and a SurfaceStore. Pure data, no drawing.
//!
//! Two pieces, and a renderer needs both:
//!
//! - [`AguiSession`] — one connection, the same six calls whether the kernel is
//!   in this process, behind a pipe or behind HTTP.
//! - [`SurfaceStore`] — the projection. Text deltas become a markdown surface
//!   and tool calls become a tool stack **here**, so no renderer reimplements
//!   that and no two renderers disagree about it.
//!
//! Both are implemented twice, here and in `@orrery/client`, against one set of
//! [`conformance`] fixtures. That is the only thing keeping five clients honest.
//!
//! # A client never writes state
//!
//! AG-UI's shared state is bidirectional by design; ours is not. `StateSnapshot`
//! and `StateDelta` flow outward only, and an edit a person makes arrives back
//! as [`AguiSession::intent`], which the kernel validates. Anyone porting a
//! component that expects to write state directly needs to know that.
//!
//! Implementation plan: `harness/docs/plans/08-protocol-transport.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod conformance;
pub mod endpoint;
pub mod error;
pub mod session;
pub mod store;

pub use endpoint::Endpoint;
pub use error::ClientError;
pub use session::AguiSession;
pub use store::{
    DETACHED_TURN, Gap, PromptView, Resolution, StoreChange, StoreState, SurfaceStore, SurfaceView,
    TurnError, TurnView,
};
