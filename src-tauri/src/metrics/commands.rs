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
/// The A7.7 recursive process tree, PULL-based: the frontend polls this only
/// while the perf panel is open (the A1.7 gating pattern), so the sampler has
/// zero background cost. Discovery does not trust the parent-pid walk alone —
/// the Job Object every descendant inherits is queried as a backstop and any
/// missed member surfaces as a "detached" child of the app root.
#[tauri::command]
pub async fn process_tree(
    shared: State<'_, SharedSampler>,
    runtime: State<'_, RuntimeService>,
    agents: State<'_, AgentService>,
    job: State<'_, crate::runtime::jobobj::JobGuard>,
    discover: Option<bool>,
) -> Result<super::tree::ProcessTreeSnapshot, String> {
    let labels = agent_labels(&agents);
    let agent_roots: Vec<(String, String, u32)> = runtime
        .pids()
        .into_iter()
        .map(|(id, pid)| {
            let label = labels
                .get(&id)
                .cloned()
                .unwrap_or_else(|| id.to_string()[..8].to_string());
            (id.to_string(), label, pid)
        })
        .collect();
    let job_pids = job.pids();
    let shared = shared.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("process_tree", move || {
            let app_pid = std::process::id();
            let mut roots: Vec<u32> = Vec::with_capacity(agent_roots.len() + 1);
            roots.push(app_pid);
            roots.extend(agent_roots.iter().map(|(_, _, pid)| *pid));
            let map = shared.refresh_and_procs(&roots, discover.unwrap_or(false));
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            super::tree::build_tree(
                app_pid,
                &agent_roots,
                &map,
                job_pids.as_deref(),
                &super::tree::private_bytes_or_rss,
                now_ms,
            )
        })
    })
    .await
    .map_err(|e| format!("join: {e}"))
}

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
