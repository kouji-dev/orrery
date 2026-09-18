//! The wasmtime extension host: one `Store` per instance, resource limits,
//! epoch deadlines and **zero WASI preopens**.
//!
//! # The one runtime with a real sandbox
//!
//! The other extension runtimes withhold capabilities but share the process.
//! This one does not: a wasm guest is isolated by construction, and the only
//! way out is an imported host function. That is what makes the threat model
//! honest, so every line of the configuration in [`engine`] is load-bearing and
//! the tests in `tests/sandbox.rs` are the proof rather than the comment.
//!
//! # Structural rules, enforced by the ABI
//!
//! 1. **The guest never holds a capability token.** It cannot: the type is not
//!    serialisable. Every import in [`HostBroker`] is expressed without one —
//!    the guest asks, the host decides, and the host looks the call's token up
//!    on its own side. See `imports::guest_never_sees_a_token`.
//! 2. **There are no WASI preopens.** The filesystem arrives only through
//!    `broker.read-file` / `broker.write-file`. A preopen would be a hole
//!    straight past the policy engine.
//! 3. **A denial is a value.** Every broker import returns `result<_, error>`,
//!    so "you may not do that" reaches the guest as something it can branch on.
//!    A trap would make policy indistinguishable from a bug.
//!
//! # Cancellation is coarse, and this crate says so
//!
//! See [`cancel`]. Short version: cancelling a wasm tool traps the guest and
//! discards the `Store`. The guest gets **no chance to clean up**. Anything it
//! needed to finish must have gone through the broker, which is transactional
//! where it matters.
//!
//! Implementation plan: `harness/docs/plans/14-wasm-wit.md`

#![deny(missing_docs)]
// This crate genuinely needs `unsafe` (see harness/docs/plans/00b-scaffold-workspace.md,
// open question 1): every `unsafe` block carries a `// SAFETY:` comment.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod broker;
pub mod cancel;
pub mod engine;
pub mod imports;
pub mod limits;
pub mod store;

pub use broker::{DenyAll, FetchOpts, FetchOut, Failure, FileOut, HostBroker, ProcOut, RunOpts};
pub use cancel::{CancelHandle, Coarseness, Ticker};
pub use engine::{EngineOpts, WasmHost};
pub use limits::Ceilings;
pub use store::{HostState, Outcome};

/// The generated host bindings for `harness/wit/orrery-extension.wit`.
///
/// Generated here rather than in `orrery-wit` so that the `wasmtime`
/// dependency stays in the one crate that runs wasm; the arena encoding is
/// usable from paths with no wasm in them at all.
pub mod bindings {
    // Generated code: the docs are the `.wit`, which is the source of truth.
    #![allow(missing_docs)]

    wasmtime::component::bindgen!({
        path: "../../../wit",
        world: "orrery-extension",
        imports: { default: async },
        exports: { default: async },
    });
}

/// Something the host could not do at all, as opposed to something the policy
/// refused. A refusal is a value; this is a failure.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// The engine could not be configured or built.
    #[error("the wasm engine could not be built: {0}")]
    Engine(#[source] wasmtime::Error),
    /// The component did not load: not a component, or the wrong world.
    #[error("this is not an `{package}` component: {source}", package = orrery_wit::PACKAGE)]
    NotOurComponent {
        /// What wasmtime said.
        #[source]
        source: wasmtime::Error,
    },
    /// Instantiation failed.
    #[error("the component could not be instantiated: {0}")]
    Instantiate(#[source] wasmtime::Error),
    /// The guest trapped, was cancelled, or ran past a ceiling.
    #[error("{0}")]
    Trapped(String),
    /// The guest returned an arena that is not a surface.
    #[error("the guest returned a malformed surface: {0}")]
    BadSurface(#[from] orrery_wit::arena::ArenaError),
}
