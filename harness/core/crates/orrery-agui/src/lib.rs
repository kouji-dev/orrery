//! The AG-UI encoder: kernel frames out to AG-UI events. Encode only, never decode.
//!
//! # Why an encoder and not a dependency
//!
//! There is no first-party Rust SDK for AG-UI. Upstream carries a community
//! `sdks/community/rust` with no code owner and stalled PRs, plus three
//! unaffiliated crates. We only ever **produce** AG-UI — a client crate would be
//! dead weight — so the event enum is vendored here, the protocol version is
//! pinned in [`drift`], and `cargo xtask agui-drift` diffs our variant names
//! against upstream's published schema in CI.
//!
//! # State flows outward only
//!
//! AG-UI's shared state is bidirectional by design. Ours is not: `StateSnapshot`
//! and `StateDelta` leave the kernel and nothing decodes them back into kernel
//! state. A client edit arrives as an `intent` on the control RPC, which the
//! kernel validates. That is a restriction of AG-UI, not a violation of it, and
//! it is why this crate has no `decode` module.
//!
//! Implementation plan: `harness/docs/plans/08-protocol-transport.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod drift;
pub mod encode;
pub mod event;
pub mod map;

pub use drift::{AGUI_PROTOCOL_VERSION, AGUI_SCHEMA_URL};
pub use encode::Encoder;
pub use event::{AguiEvent, Frame, PatchOp};
pub use map::{
    CONSENT_REQUEST, CONSENT_RESOLVED, SURFACES_ROOT, TURN_CANCELLED, field_pointer,
    surface_pointer,
};
