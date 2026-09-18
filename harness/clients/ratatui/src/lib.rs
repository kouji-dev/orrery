//! The default Orrery TUI: a ratatui renderer linked into the `orrery` binary.
//!
//! # The hybrid screen
//!
//! Settled turns are printed **once** into the terminal's own scrollback, where
//! selection and the scrollwheel are the terminal's job and nothing here ever
//! repaints them. Only the turn in flight, the consent bar, the composer and the
//! footer are redrawn, at a frame budget. Codex went full-screen, then swapped
//! its history widget for an append-only log to get selection back and lost
//! streaming doing it; [`scrollback`] and [`live`] are how this client takes
//! both (§6.4).
//!
//! # This client is not privileged
//!
//! It attaches over the in-process transport exactly the way an external client
//! attaches over a pipe, and it reads [`orrery_client::SurfaceStore`], never
//! kernel state. All decoding, `seq` tracking and store mutation live in
//! `orrery-client`: **this crate draws**. Nothing here parses an AG-UI event,
//! and anything that needs to belongs in the SDK.
//!
//! # Every core surface renders
//!
//! [`widgets`] has one widget per [`orrery_proto::SurfaceKind`] and a test that
//! fails when a variant appears without one. A `custom` surface always draws its
//! fallback here (§6.3); the hook point for a custom TUI renderer is documented
//! in [`widgets::custom`] and deliberately not built.
//!
//! Implementation plan: `harness/docs/plans/09b-client-ratatui.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod app;
pub mod composer;
pub mod event;
pub mod footer;
pub mod live;
pub mod scrollback;
pub mod terminal;
pub mod testing;
pub mod theme;
pub mod widgets;

pub use app::{App, RestoreGuard};
pub use composer::Composer;
pub use event::{FrameSource, Outgoing, VecSource};
pub use footer::Footer;
pub use live::LiveRegion;
pub use scrollback::{Recording, Scrollback};
pub use theme::Theme;
