//! The kernel-side differ, wired to the calls that produce surfaces.
//!
//! # What was wrong
//!
//! `orrery-surface` — validate, hash, diff, the per-turn store and the seal at
//! `turn.settled` — was absent from `cargo tree -p orrery-cli` altogether. An
//! extension's `ctx.ui.*` output was described, returned in the tool's
//! `Outcome`, and **discarded** by the extension table's session-wide
//! `SurfaceSink::discarding()`. The only surfaces a client ever saw were the
//! ones `orrery-cli`'s own `Publisher` minted by hand, which is a client
//! inventing surfaces the kernel should have produced.
//!
//! The structural reason was named in plan 09 and is fixed in
//! `orrery-ext-api`: `SurfaceEmit::emit` carries neither a turn nor a surface
//! id, so one session-wide sink has nothing to key a diff on. A
//! [`SurfaceSource`] is asked per call, and a [`CallId`] is exactly the key the
//! differ needed.
//!
//! # One call, one surface — but **not** the call's own id
//!
//! Plan 09 proposed keying it on `SurfaceId::from_uuid(*call.as_uuid())`, the
//! derivation `orrery-cli` already uses for a tool call's streaming arguments.
//! That does not survive contact with the wire: `orrery-agui` encodes a
//! `Replace` on an open call's surface as `TOOL_CALL_ARGS`, so the file a
//! `read` described arrived at the client claiming to be the *arguments* the
//! model had written. Two different things on one id is one thing too many.
//!
//! So the sink mints an id of its own, once, in [`SurfaceSource::for_call`] —
//! which is called once per call, so it is still one surface per call and a
//! second `ctx.ui.*` from that call is a **diff** against the first, which is
//! the distinction the whole crate exists to make.

use std::sync::Arc;

use orrery_ext_api::{SurfaceEmit, SurfaceSink, SurfaceSource};
use orrery_proto::{CallId, Surface, SurfaceId, SurfacePatch, TurnId};
use orrery_surface::SurfaceStore;
use parking_lot::Mutex;

/// Where the patches a differ produces go.
///
/// Implemented by the composition root — `orrery-cli`'s `Publisher` turns each
/// one into an `Event::Delta` on the hub, which is how a surface the *kernel*
/// produced reaches an attached client.
pub trait SurfacePatches: Send + Sync + std::fmt::Debug {
    /// One patch, for one surface.
    fn patch(&self, surface: SurfaceId, patch: SurfacePatch);
}

/// A sink that keeps nothing. The honest default for a harness nobody is
/// watching: the differ still runs, and its output goes nowhere.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoPatches;

impl SurfacePatches for NoPatches {
    fn patch(&self, _surface: SurfaceId, _patch: SurfacePatch) {}
}

struct Inner {
    store: Mutex<SurfaceStore>,
    sink: Arc<dyn SurfacePatches>,
    /// The turn surfaces are keyed on. A store keyed by turn is what makes
    /// sealing possible, and the harness does not otherwise know which turn is
    /// running — so the composition root says, at `turn.started`.
    turn: Mutex<TurnId>,
}

/// The kernel's surface store, as the extension table's surface source.
#[derive(Clone)]
pub struct KernelSurfaces {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for KernelSurfaces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KernelSurfaces")
            .field("turn", &*self.inner.turn.lock())
            .finish_non_exhaustive()
    }
}

impl KernelSurfaces {
    /// A differ whose patches go somewhere.
    #[must_use]
    pub fn new(sink: Arc<dyn SurfacePatches>) -> Self {
        Self {
            inner: Arc::new(Inner {
                store: Mutex::new(SurfaceStore::new()),
                sink,
                turn: Mutex::new(TurnId::new()),
            }),
        }
    }

    /// A differ that runs and reports to nobody.
    #[must_use]
    pub fn discarding() -> Self {
        Self::new(Arc::new(NoPatches))
    }

    /// A turn has started: surfaces from now on belong to it.
    pub fn begin_turn(&self, turn: TurnId) {
        *self.inner.turn.lock() = turn;
    }

    /// A turn has settled. Everything it holds is history, and a late patch is
    /// **reported to the extension** rather than sent — a client that has closed
    /// a turn has nowhere to put one.
    pub fn seal_turn(&self, turn: TurnId) {
        self.inner.store.lock().seal(turn);
    }

    /// One turn's surfaces, as the store last saw them. For a caller that wants
    /// to render the whole turn rather than follow the patches.
    #[must_use]
    pub fn of_turn(&self, turn: TurnId) -> Vec<(SurfaceId, Surface)> {
        self.inner
            .store
            .lock()
            .surfaces(&turn)
            .into_iter()
            .map(|(id, s)| (id, s.clone()))
            .collect()
    }
}

impl SurfaceSource for KernelSurfaces {
    fn for_call(&self, call: CallId) -> SurfaceSink {
        let _ = call;
        SurfaceSink::to(Arc::new(CallSurfaces {
            inner: self.inner.clone(),
            id: SurfaceId::new(),
        }))
    }
}

/// One call's view of the store.
struct CallSurfaces {
    inner: Arc<Inner>,
    id: SurfaceId,
}

impl SurfaceEmit for CallSurfaces {
    fn emit(&self, surface: &Surface) {
        let turn = *self.inner.turn.lock();
        // An extension may name its own surface id — `SurfaceBuilders::with_id`
        // exists so it can re-emit under one — and when it has, that is the key.
        // Otherwise the call is.
        let id = surface.id.unwrap_or(self.id);
        let patches = self.inner.store.lock().emit(turn, id, surface.clone());
        match patches {
            Ok(patches) => {
                for patch in patches {
                    self.inner.sink.patch(id, patch);
                }
            }
            // Sealed, malformed or too deep. **No frame is produced**: the
            // refusal belongs to the extension that emitted it, and a warning
            // in the log is the only place this layer can put it.
            Err(e) => tracing::warn!(
                target: "orrery.harness.surfaces",
                surface = %id,
                error = %e,
                "a surface was refused rather than sent"
            ),
        }
    }
}
