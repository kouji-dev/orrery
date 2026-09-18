//! Live unload: `Live → Draining → Dead`, and the session does not restart.
//!
//! ```text
//! Live ──unload()──> Draining ──grace elapsed──> Dead
//!                       │
//!                       └─ cancel token fired; in-flight calls settle as Cancelled
//! ```
//!
//! The registry keeps its entry for the unloaded extension on purpose. A
//! reference the model already has must settle as
//! [`Outcome::Unloaded`](orrery_proto::Outcome) — something it can re-plan
//! around — and not as `NoSuchTool`, which is an `Err` and would fail the turn.

use std::sync::Arc;
use std::time::Duration;

use orrery_ext_api::{HostError, InstanceState};
use orrery_proto::ExtId;
use orrery_tools::{ExtState, Registry};

use crate::table::{ExtensionTable, not_loaded};

/// How long an extension is given to finish what it has before it is killed.
pub const DEFAULT_GRACE: Duration = Duration::from_secs(5);

/// How often the drain checks whether the in-flight calls have settled.
const POLL: Duration = Duration::from_millis(5);

impl ExtensionTable {
    /// Take one extension down without restarting the session.
    ///
    /// Fires the instance's cancel token, waits up to `grace` for its in-flight
    /// calls to settle, then drops it whether they did or not. A well-behaved
    /// extension settles its calls `Cancelled`; a stubborn one is simply left
    /// behind, because the alternative is a session that cannot unload anything
    /// that has stopped listening.
    ///
    /// # Errors
    ///
    /// [`HostError::NotLoaded`] when there is nothing of that name to unload —
    /// the caller asked for something impossible and is told so. Everything the
    /// *extension* does wrong on the way out is logged, not returned: by the
    /// time unload is called, the decision has already been taken.
    pub async fn unload(&self, ext: &ExtId, grace: Duration) -> Result<(), HostError> {
        let Some(instance) = self.get(ext) else {
            return Err(not_loaded(ext));
        };

        // 1 · No new calls. Anything arriving now settles `Unloaded`.
        self.set_state(ext, InstanceState::Draining);

        // 2 · Every call this instance is carrying is told to stop. The token
        //     is the parent of every call token, so one fire reaches them all.
        instance.cancel_token().cancel();

        // 3 · Wait for them, but not forever.
        let deadline = tokio::time::Instant::now() + grace;
        while instance.in_flight() > 0 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(POLL).await;
        }
        let abandoned = instance.in_flight();
        if abandoned > 0 {
            tracing::warn!(
                target: "orrery.host.unload",
                %ext,
                abandoned,
                ?grace,
                "the grace window closed with calls still in flight"
            );
        }

        // 4 · The runtime's own teardown: kill the child, drop the store, close
        //     the module. Best effort — it is already going away.
        if let Err(e) = instance.host_ref().unload(ext).await {
            tracing::warn!(target: "orrery.host.unload", %ext, error = %e, "runtime teardown failed");
        }

        // 5 · Dead, and out of the table. A reference held anywhere else can no
        //     longer resolve, which is exactly what `Outcome::Unloaded` is.
        instance.mark_dead();
        self.forget(ext);
        tracing::info!(target: "orrery.host.unload", %ext, "unloaded");
        Ok(())
    }

    /// Move an extension's state, if the machine allows it.
    pub fn set_state(&self, ext: &ExtId, state: InstanceState) -> bool {
        self.get(ext).is_some_and(|i| i.set_state(state))
    }

    /// Tell a tool registry that an extension is going, so its tools stop being
    /// offered to the model while its entries stay resolvable.
    ///
    /// Two steps, in this order, because between them is the window where a
    /// reference the model already holds is still dispatched — and settles
    /// `Unloaded` rather than failing the turn.
    pub fn withdraw_from(&self, registry: &mut Registry, ext: &ExtId) {
        registry.set_state(ext, ExtState::Dead);
    }

    /// Unload every loaded extension. Shutdown, in load order reversed.
    pub async fn unload_all(self: &Arc<Self>, grace: Duration) {
        let mut loaded: Vec<ExtId> = self.all().iter().map(|i| i.ext().clone()).collect();
        loaded.reverse();
        for ext in loaded {
            let _ = self.unload(&ext, grace).await;
        }
    }
}
