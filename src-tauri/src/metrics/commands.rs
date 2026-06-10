use std::collections::HashMap;

use tauri::State;
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::runtime::RuntimeService;

use super::{SharedSampler, SystemMetrics, SNAPSHOT_FRESH};

/// Resolve agent uuids -> display names so subtree rows carry the agent name
/// (falling back to the short uuid when a name lookup fails).
pub fn agent_labels(agents: &AgentService) -> HashMap<Uuid, String> {
    agents
        .list()
        .map(|list| list.into_iter().map(|a| (a.id, a.name)).collect())
        .unwrap_or_default()
}

/// Optional initial value so the UI can paint before the first push. Served
/// from the push loop's snapshot when fresh (at most one loop period old);
/// otherwise
/// a single scoped refresh on the SAME warm sampler — never a cold init, double
/// sweep, or sleep (the old path cost ~1s per call). State reads stay outside
/// the blocking closure (cheap, non-Send).
#[tauri::command]
pub async fn system_metrics(
    shared: State<'_, SharedSampler>,
    runtime: State<'_, RuntimeService>,
    agents: State<'_, AgentService>,
) -> Result<SystemMetrics, String> {
    let pids = runtime.pids();
    let labels = agent_labels(&agents);
    let shared = shared.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Even the cache check goes through the blocking pool: the shared lock
        // can be held by the push loop for a full discovery sweep, and that
        // wait must not park an async-runtime thread.
        if let Some(m) = shared.cached(SNAPSHOT_FRESH) {
            return m;
        }
        crate::perf::timed("system_metrics", move || {
            shared.refresh_and_sample(std::process::id(), &pids, &labels)
        })
    })
    .await
    .map_err(|e| format!("join: {e}"))
}
