//! The engine, and every setting on it that is load-bearing.
//!
//! | Setting | Why |
//! |---|---|
//! | `wasm_component_model(true)` | We bind components, not core modules. |
//! | async imports (`imports: { default: async }`) | The broker is async. A blocking import would block the whole runtime. |
//! | `epoch_interruption(true)` + [`Ticker`] | Wall-clock ceiling. Traps at loop backedges. |
//! | `consume_fuel` **only under the eval runner** | Determinism for reproducible runs; it costs throughput, so it is off normally. |
//! | `Store::limiter` from the budget | Memory ceiling, enforced by wasmtime rather than hoped for. |
//! | One `Store` per instance | Isolation, and it makes "discard on trap" complete cleanup. |
//!
//! # Fuel needs a second engine (plan 14, open question 2 — answered)
//!
//! `Config` is per-`Engine`, and `consume_fuel` is a `Config` setting. The eval
//! runner therefore **cannot** turn fuel on per run against a shared engine; it
//! needs its own. [`WasmHost::for_eval`] builds that second engine, and
//! [`EngineOpts::fuel`] is the only difference between them. Two engines is the
//! answer, and it is cheap: an `Engine` is a compiler and a code cache, and the
//! eval runner wants a separate cache anyway.

use std::sync::Arc;

use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

use crate::HostError;
use crate::broker::HostBroker;
use crate::cancel::{CancelHandle, Coarseness, Ticker};
use crate::limits::Ceilings;
use crate::store::{HostState, Outcome};

/// How one engine is configured.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct EngineOpts {
    /// Turn on fuel metering. **Eval runner only.**
    ///
    /// It buys determinism — the same run consumes the same fuel — and costs
    /// throughput on every instruction, which is why it is not on by default.
    pub fuel: bool,
}

/// The wasm extension host.
///
/// Holds the engine, the linker with the broker imports already on it, and the
/// epoch ticker. One per process is normal; a second one exists only for eval.
pub struct WasmHost {
    engine: Engine,
    linker: Linker<HostState>,
    opts: EngineOpts,
    // Dropping this stops the epoch thread, so the host must outlive every call
    // it started. That is why it is owned here and not handed out.
    _ticker: Ticker,
}

impl std::fmt::Debug for WasmHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmHost")
            .field("opts", &self.opts)
            .finish_non_exhaustive()
    }
}

impl WasmHost {
    /// The normal host: component model, async, epochs, no fuel.
    ///
    /// # Errors
    ///
    /// [`HostError::Engine`] when the engine cannot be built or the imports
    /// cannot be linked.
    pub fn new() -> Result<Self, HostError> {
        Self::with(EngineOpts::default())
    }

    /// The eval runner's host: the same, plus fuel.
    ///
    /// # Errors
    ///
    /// As [`Self::new`].
    pub fn for_eval() -> Result<Self, HostError> {
        Self::with(EngineOpts { fuel: true })
    }

    /// A host with explicit options.
    ///
    /// # Errors
    ///
    /// [`HostError::Engine`].
    pub fn with(opts: EngineOpts) -> Result<Self, HostError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        config.consume_fuel(opts.fuel);

        let engine = Engine::new(&config).map_err(HostError::Engine)?;
        let mut linker: Linker<HostState> = Linker::new(&engine);

        // WASI first, then ours. Both are needed: a component built for
        // wasm32-wasip2 imports `wasi:cli` and `wasi:io` whatever it does, and
        // refusing to link them would mean refusing to run any real guest. The
        // sandbox is not "no WASI"; it is "WASI with nothing in it" — see
        // `store::no_preopens`.
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(HostError::Engine)?;
        crate::imports::add_to_linker(&mut linker).map_err(HostError::Engine)?;

        let ticker = Ticker::start(&engine);
        Ok(Self {
            engine,
            linker,
            opts,
            _ticker: ticker,
        })
    }

    /// The engine, for a caller that needs to pre-compile a component.
    #[must_use]
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// How this host's options were set.
    #[must_use]
    pub fn opts(&self) -> EngineOpts {
        self.opts
    }

    /// What cancelling a call on this host actually does. See [`crate::cancel`].
    #[must_use]
    pub const fn coarseness(&self) -> Coarseness {
        Coarseness::TRUTH
    }

    /// Compile a component from bytes.
    ///
    /// # Errors
    ///
    /// [`HostError::NotOurComponent`] when the bytes are not a component, or
    /// not one built against our world.
    pub fn compile(&self, bytes: &[u8]) -> Result<Component, HostError> {
        Component::new(&self.engine, bytes)
            .map_err(|source| HostError::NotOurComponent { source })
    }

    /// A cancel handle for a call that has not started yet.
    ///
    /// Handed to the kernel **before** the call, because a cancel that arrives
    /// while the guest is still being instantiated must still land.
    #[must_use]
    pub fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(self.engine.clone())
    }

    /// Call one tool in one component, in a `Store` of its own.
    ///
    /// The `Store` is created here and dropped when this returns, whatever
    /// happens — a trap, a cancel or a clean return. That is what makes
    /// "discard on trap" complete cleanup rather than a hope.
    ///
    /// # Errors
    ///
    /// [`HostError`] when the component cannot be instantiated. A guest that
    /// trapped, failed or was cancelled is an [`Outcome`], not an error: the
    /// session survives all three.
    pub async fn call(
        &self,
        component: &Component,
        ceilings: Ceilings,
        broker: Arc<dyn HostBroker>,
        cancel: CancelHandle,
        tool: &str,
        input: &str,
    ) -> Result<Outcome, HostError> {
        let mut store = Store::new(
            &self.engine,
            HostState::new(ceilings, broker, cancel.clone()),
        );
        store.limiter(|state| &mut state.limits);

        // The wall-clock ceiling AND cancellation, in one mechanism.
        //
        // The deadline is one tick, and a callback decides each time whether to
        // extend it. That is what lets a cancel land against a long budget:
        // racing the epoch counter past a 10-minute deadline would need 60 000
        // increments, while a flag the callback reads needs one.
        //
        // The callback runs at loop backedges and function entries, which is
        // also precisely why it cannot free a guest blocked inside a host
        // import. See `crate::cancel`.
        store.set_epoch_deadline(1);
        store.epoch_deadline_callback(|ctx| {
            let state = ctx.data();
            if state.cancel.is_cancelled() {
                return Err(wasmtime::Error::msg(CANCELLED));
            }
            if state.started.elapsed() > state.wall_clock {
                return Err(wasmtime::Error::msg(
                    "the extension ran past its wall-clock ceiling and was stopped",
                ));
            }
            Ok(wasmtime::UpdateDeadline::Continue(1))
        });

        if self.opts.fuel {
            // Only reachable under the eval runner. A large but finite budget:
            // the point is determinism, not a second ceiling.
            store
                .set_fuel(u64::from(u32::MAX))
                .map_err(HostError::Engine)?;
        }

        let instance = self
            .linker
            .instantiate_async(&mut store, component)
            .await
            .map_err(HostError::Instantiate)?;
        let world = crate::bindings::OrreryExtension::new(&mut store, &instance)
            .map_err(|source| HostError::NotOurComponent { source })?;

        let called = world
            .orrery_extension_tools()
            .call_call(&mut store, tool, input)
            .await;

        // Drop the store before anything else looks at the result: the guest's
        // memory, its tables and anything it held are gone by the time the
        // caller sees an `Outcome`.
        drop(store);

        // A cancelled call settles `Cancelled` even when the guest handled the
        // `error::cancelled` it got and returned tidily: the turn was stopped,
        // and a surface produced after that is not the turn's answer.
        if cancel.is_cancelled() {
            return Ok(Outcome::Cancelled);
        }

        match called {
            Ok(Ok(arena)) => {
                let arena = crate::imports::from_wit(arena);
                // The arena came out of guest memory, so nothing about it is
                // assumed: a malformed one is a trap, not a panic.
                match orrery_wit::arena::rebuild(&arena) {
                    Ok(surface) => Ok(Outcome::Ok(surface)),
                    Err(e) => Ok(Outcome::Trapped(format!(
                        "the guest returned a malformed surface: {e}"
                    ))),
                }
            }
            Ok(Err(message)) => Ok(Outcome::Failed(message)),
            Err(e) if cancel.is_cancelled() || is_cancelled(&e) => {
                tracing::debug!(error = %e, "wasm guest trapped after cancellation");
                Ok(Outcome::Cancelled)
            }
            Err(e) => Ok(Outcome::Trapped(describe(&e))),
        }
    }
}

/// What the deadline callback says when a call was cancelled. Matched on the
/// way back out, so a cancellation is never reported as a ceiling.
const CANCELLED: &str = "orrery: the turn was cancelled";

/// A trap, said in a way a person can act on.
///
/// wasmtime wraps the real cause in a wasm backtrace, so this walks the chain
/// rather than reading the top: the top is always "error while executing".
fn describe(e: &wasmtime::Error) -> String {
    if let Some(trap) = trap_of(e) {
        return match trap {
            wasmtime::Trap::Interrupt => {
                "the extension ran past its wall-clock ceiling and was stopped".to_owned()
            }
            wasmtime::Trap::OutOfFuel => {
                "the extension ran past its fuel budget and was stopped".to_owned()
            }
            wasmtime::Trap::UnreachableCodeReached | wasmtime::Trap::MemoryOutOfBounds => {
                // What an out-of-memory guest looks like: the allocator got a
                // failure from `memory.grow`, Rust aborted, and abort is
                // `unreachable`.
                "the extension stopped abruptly: it ran past its memory ceiling,                  or it panicked"
                    .to_owned()
            }
            other => format!("the extension trapped: {other}"),
        };
    }
    // Our own deadline callback's message, or anything else.
    root(e)
}

/// The trap somewhere in this error's chain, if there is one.
fn trap_of(e: &wasmtime::Error) -> Option<wasmtime::Trap> {
    e.chain()
        .find_map(|cause| cause.downcast_ref::<wasmtime::Trap>().copied())
}

/// The deepest cause, which is where our own messages end up.
fn root(e: &wasmtime::Error) -> String {
    e.chain()
        .last()
        .map_or_else(|| e.to_string(), ToString::to_string)
}

/// Whether this error is the deadline callback saying the turn was cancelled.
fn is_cancelled(e: &wasmtime::Error) -> bool {
    e.chain().any(|cause| cause.to_string().contains(CANCELLED))
}
