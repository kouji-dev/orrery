//! One instance's mutable half: its state, its cancel token and how many calls
//! are in flight.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use orrery_ext_api::InstanceState;
use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

/// The state cell, with the transition rules actually enforced.
#[derive(Debug, Default)]
pub struct StateCell(RwLock<InstanceState>);

impl StateCell {
    /// A cell in a given state.
    #[must_use]
    pub fn new(state: InstanceState) -> Self {
        Self(RwLock::new(state))
    }

    /// What state it is in.
    #[must_use]
    pub fn get(&self) -> InstanceState {
        *self.0.read()
    }

    /// Move to `next`, if the machine allows it.
    ///
    /// Returns whether it moved. A refused transition is logged rather than
    /// panicking: a host that tries to revive a dead instance has a bug, and a
    /// bug in the host must not take the session with it.
    pub fn set(&self, next: InstanceState) -> bool {
        let mut slot = self.0.write();
        if *slot == next {
            return true;
        }
        if slot.can_transition_to(next) {
            *slot = next;
            true
        } else {
            tracing::warn!(
                target: "orrery.host.state",
                from = %*slot,
                to = %next,
                "refused an impossible instance transition"
            );
            false
        }
    }
}

/// How many calls an instance is currently carrying.
///
/// The drain in `unload` waits on this, which is what makes "in-flight calls
/// settle" a property rather than a hope.
#[derive(Debug, Default)]
pub struct InFlight(AtomicUsize);

impl InFlight {
    /// How many right now.
    #[must_use]
    pub fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }

    /// Enter one call. The guard leaves it, including on a panic.
    #[must_use]
    pub fn enter(self: &Arc<Self>) -> CallGuard {
        self.0.fetch_add(1, Ordering::SeqCst);
        CallGuard(self.clone())
    }
}

/// Holds an instance's in-flight count up for as long as a call lives.
#[derive(Debug)]
pub struct CallGuard(Arc<InFlight>);

impl Drop for CallGuard {
    fn drop(&mut self) {
        self.0.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// An instance's cancel token: one per instance, a child per call.
///
/// `turn.cancel` must reach one in-flight tool call and not the connection, so
/// a call gets a child token; `unload` fires the parent, which reaches every
/// child at once.
#[must_use]
pub fn call_token(instance: &CancellationToken) -> CancellationToken {
    instance.child_token()
}

#[cfg(test)]
mod tests {
    use super::{InFlight, StateCell};
    use orrery_ext_api::InstanceState;
    use std::sync::Arc;

    #[test]
    fn a_dead_instance_does_not_come_back() {
        let cell = StateCell::new(InstanceState::Live);
        assert!(cell.set(InstanceState::Draining));
        assert!(cell.set(InstanceState::Dead));
        assert!(!cell.set(InstanceState::Live));
        assert_eq!(cell.get(), InstanceState::Dead);
    }

    #[test]
    fn the_guard_leaves_even_on_a_panic() {
        let flight = Arc::new(InFlight::default());
        let taken = flight.clone();
        let _ = std::panic::catch_unwind(move || {
            let _guard = taken.enter();
            panic!("the tool blew up");
        });
        assert_eq!(flight.count(), 0);
    }
}
