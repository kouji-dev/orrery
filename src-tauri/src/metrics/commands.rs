use std::collections::HashMap;

use tauri::State;
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::runtime::RuntimeService;

use super::{MetricsSampler, SystemMetrics};

/// Resolve agent uuids -> display names so subtree rows carry the agent name
/// (falling back to the short uuid when a name lookup fails).
pub fn agent_labels(agents: &AgentService) -> HashMap<Uuid, String> {
    agents
        .list()
        .map(|list| list.into_iter().map(|a| (a.id, a.name)).collect())
        .unwrap_or_default()
}

/// Sample the app subtree + every running agent's subtree, labelling each row
/// with the agent's name. The sampler must already be warmed (one prior refresh)
/// for cpu% to be meaningful.
pub fn sample_with_labels(
    sampler: &MetricsSampler,
    app_pid: u32,
    runtime: &RuntimeService,
    agents: &AgentService,
) -> SystemMetrics {
    let pids = runtime.pids();
    let labels = agent_labels(agents);
    let mut metrics = sampler.sample(app_pid, &pids);
    for p in &mut metrics.procs {
        if p.id == "app" {
            continue;
        }
        if let Ok(uuid) = Uuid::parse_str(&p.id) {
            if let Some(name) = labels.get(&uuid) {
                p.label = name.clone();
            }
        }
    }
    metrics
}

/// Optional initial value so the UI can paint before the first push. Does a
/// warm-up + a real refresh (two refreshes spaced by sysinfo's minimum cpu
/// interval) so the one-shot reading still has a usable cpu%. The cold sampler
/// init + deliberate sleep cost ~2s — blocking pool; State reads stay outside
/// the closure (cheap, non-Send).
#[tauri::command]
pub async fn system_metrics(
    runtime: State<'_, RuntimeService>,
    agents: State<'_, AgentService>,
) -> Result<SystemMetrics, String> {
    let pids = runtime.pids();
    let labels = agent_labels(&agents);
    tauri::async_runtime::spawn_blocking(move || {
        let mut sampler = MetricsSampler::new();
        sampler.refresh(); // warm-up
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        sampler.refresh();
        let mut metrics = sampler.sample(std::process::id(), &pids);
        for p in &mut metrics.procs {
            if p.id == "app" {
                continue;
            }
            if let Ok(uuid) = Uuid::parse_str(&p.id) {
                if let Some(name) = labels.get(&uuid) {
                    p.label = name.clone();
                }
            }
        }
        metrics
    })
    .await
    .map_err(|e| format!("join: {e}"))
}
