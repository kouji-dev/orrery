//! Cancellation, and the honest account of what it cannot do.
//!
//! # Coarse, and we say so
//!
//! Epoch interruption traps at loop backedges and function entries. It
//! **cannot** cleanly unwind a guest that is blocked inside a host import, and
//! it gives the guest no chance to run a destructor. So `turn.cancel` against a
//! wasm tool is:
//!
//! > trap the guest and discard the `Store`
//!
//! and not "ask the guest to stop". Three consequences, none of them hidden:
//!
//! 1. **The guest gets no cleanup.** Anything it needed to finish must already
//!    have gone through the broker, which is transactional where it matters
//!    (atomic writes). A half-written file is the broker's problem to have
//!    prevented, not the guest's to fix on the way out.
//! 2. **A guest blocked in an import is freed by the broker, not by the
//!    epoch.** The broker kills the child, the import returns
//!    [`Failure::Cancelled`](crate::Failure::Cancelled), and the guest then
//!    traps at its next backedge. Two mechanisms, and **the broker's is the one
//!    that stops the work**; the epoch only stops the guest from carrying on.
//! 3. **Discarding the `Store` is complete cleanup.** One `Store` per instance
//!    is what makes that true: there is no shared state left behind to tidy.
//!
//! [`Coarseness`] exists so a caller can read that back as data rather than
//! discovering it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use wasmtime::Engine;

/// What cancelling a wasm tool actually does, as data.
///
/// Returned by [`crate::WasmHost::coarseness`] so that a client can say the
/// true thing in a tooltip instead of implying a graceful stop.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Coarseness {
    /// Whether the guest is given a chance to clean up. Always `false` here.
    pub guest_can_clean_up: bool,
    /// Whether the `Store` is discarded. Always `true` here.
    pub store_is_discarded: bool,
    /// Whether a guest blocked in an import is stopped by the epoch alone.
    /// Always `false`: the broker is what stops the work.
    pub epoch_alone_frees_a_blocked_guest: bool,
}

impl Coarseness {
    /// The truth about this host.
    pub const TRUTH: Self = Self {
        guest_can_clean_up: false,
        store_is_discarded: true,
        epoch_alone_frees_a_blocked_guest: false,
    };
}

/// Cancel one in-flight call.
///
/// Cloneable and cheap: the kernel keeps one per call and drops it when the
/// call settles.
#[derive(Clone, Debug)]
pub struct CancelHandle {
    engine: Engine,
    cancelled: Arc<AtomicBool>,
}

impl CancelHandle {
    pub(crate) fn new(engine: Engine) -> Self {
        Self {
            engine,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Trap the guest at its next backedge.
    ///
    /// Idempotent. It does **not** wait for the guest to stop, and it does not
    /// free a guest blocked inside an import — cancelling the broker's work is
    /// what does that, and the kernel does both.
    ///
    /// Mechanically: this sets a flag and advances the epoch once. The store's
    /// epoch-deadline callback fires at the guest's next backedge, reads the
    /// flag and refuses to extend the deadline. Setting a flag rather than
    /// racing the epoch counter past a live deadline is what makes this work
    /// for a long budget as well as a short one.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.engine.increment_epoch();
    }

    /// Whether [`Self::cancel`] has been called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// The thread that turns wall clock into epochs.
///
/// One per [`crate::WasmHost`]. It ticks every [`Ticker::TICK`]; a store's
/// deadline is therefore accurate to within one tick, which is why the
/// ceiling tests allow slack rather than asserting to the millisecond.
#[derive(Debug)]
pub struct Ticker {
    stop: Arc<AtomicBool>,
}

impl Ticker {
    /// How often the epoch advances. One tick is the resolution of every
    /// wall-clock ceiling in this crate.
    pub const TICK: Duration = Duration::from_millis(10);

    /// Start ticking `engine`'s epoch.
    ///
    /// The thread holds only a clone of the `Engine`, so it keeps the engine
    /// alive but nothing else; dropping the [`Ticker`] stops it.
    #[must_use]
    pub fn start(engine: &Engine) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let engine = engine.clone();
        let flag = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("orrery-wasm-epoch".to_owned())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Self::TICK);
                    engine.increment_epoch();
                }
            })
            .expect("the epoch ticker thread starts");
        Self { stop }
    }

    /// How many ticks a wall-clock budget is worth, rounded up and never zero.
    #[must_use]
    pub fn ticks_for(ms: u64) -> u64 {
        let tick = Self::TICK.as_millis() as u64;
        ms.div_ceil(tick).max(1)
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
