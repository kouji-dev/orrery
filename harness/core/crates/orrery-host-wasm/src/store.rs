//! One `Store` per instance, and the state that hangs off it.

use std::sync::Arc;

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::broker::HostBroker;
use crate::cancel::CancelHandle;
use crate::limits::Ceilings;

/// Everything one instance's host functions can reach.
///
/// Note what is **not** here: a `CapabilityToken`. The broker behind
/// [`HostState::broker`] holds whatever it needs to authorise a call; the guest
/// never sees it, and there is no field it could read it out of.
pub struct HostState {
    /// The WASI context. Built with **no preopens** — see [`no_preopens`].
    pub(crate) wasi: WasiCtx,
    /// Resources the guest holds: streams, pollables. Not files: there are none.
    pub(crate) table: ResourceTable,
    /// Memory ceiling, enforced by wasmtime.
    pub(crate) limits: wasmtime::StoreLimits,
    /// The one way out.
    pub(crate) broker: Arc<dyn HostBroker>,
    /// Cancellation for this call.
    pub(crate) cancel: CancelHandle,
    /// When the call started, so the deadline callback can tell how long the
    /// guest has had.
    pub(crate) started: std::time::Instant,
    /// How long it may have in total.
    pub(crate) wall_clock: std::time::Duration,
}

impl HostState {
    /// State for one call.
    pub(crate) fn new(
        ceilings: Ceilings,
        broker: Arc<dyn HostBroker>,
        cancel: CancelHandle,
    ) -> Self {
        Self {
            wasi: no_preopens(),
            table: ResourceTable::new(),
            limits: ceilings.limiter(),
            broker,
            cancel,
            started: std::time::Instant::now(),
            wall_clock: std::time::Duration::from_millis(ceilings.wall_clock_ms),
        }
    }

    /// The broker this call talks to.
    pub fn broker(&self) -> &Arc<dyn HostBroker> {
        &self.broker
    }

    /// Whether this call has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

/// The WASI context every guest gets: **nothing**.
///
/// This function is the boundary, and it is deliberately one line long with a
/// long comment, because the whole sandbox story is "what is not called here":
///
/// - No `preopened_dir`, so `wasi:filesystem/preopens.get-directories` returns
///   an empty list and there is no descriptor to resolve a path against. A
///   guest calling `std::fs::File::open` fails, whatever the path.
/// - No `inherit_stdio`, `inherit_env`, `inherit_args`, `inherit_network`,
///   `allow_ip_name_lookup` or `allow_tcp`. Ambient authority is not withheld
///   selectively; it is never granted.
///
/// Adding any of those is the single change that would put a hole straight past
/// the policy engine, which is why `tests/sandbox.rs` asserts the result from
/// **inside a guest** rather than trusting this comment.
#[must_use]
pub fn no_preopens() -> WasiCtx {
    WasiCtxBuilder::new().build()
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// How one guest call ended.
///
/// A denial is **not** here: a denial is a value the guest received and carried
/// on from. What reaches this type is the call's own fate.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The guest returned a surface.
    Ok(orrery_proto::surface::Surface),
    /// The guest returned its own error string, which is what the model sees.
    Failed(String),
    /// The guest trapped: a ceiling, a panic, or a malformed arena.
    Trapped(String),
    /// The guest was cancelled. The `Store` has been discarded.
    Cancelled,
}

impl Outcome {
    /// The surface, if there is one.
    #[must_use]
    pub fn surface(&self) -> Option<&orrery_proto::surface::Surface> {
        match self {
            Outcome::Ok(s) => Some(s),
            _ => None,
        }
    }

    /// Whether the session survives this. Always true — that is the point of
    /// one `Store` per instance.
    #[must_use]
    pub const fn session_survives(&self) -> bool {
        true
    }
}
