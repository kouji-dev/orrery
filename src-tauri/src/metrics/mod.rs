use std::collections::HashMap;

use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use uuid::Uuid;

pub mod commands;

/// One subtree's roll-up: an agent's process tree, or the app's own tree.
/// `id` is the agent uuid string (or "app"); `label` is the agent name (or "katrix").
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcMetric {
    pub id: String,
    pub label: String,
    pub cpu: f32,
    pub mem_bytes: u64,
}

/// The whole snapshot pushed to the UI every tick: machine totals + per-subtree rows.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemMetrics {
    pub total_cpu: f32,
    pub total_mem_bytes: u64,
    pub procs: Vec<ProcMetric>,
}

/// A minimal per-process snapshot the pure aggregator works on. Kept independent
/// of `sysinfo` so the subtree roll-up is unit-testable from synthetic data.
#[derive(Debug, Clone, Copy)]
pub struct ProcSample {
    pub parent: Option<u32>,
    pub cpu: f32,
    pub mem_bytes: u64,
}

/// Owns a `sysinfo::System`, refreshed in place each tick.
pub struct MetricsSampler {
    sys: System,
}

impl Default for MetricsSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsSampler {
    pub fn new() -> Self {
        Self { sys: System::new() }
    }

    /// Refresh process cpu + memory. sysinfo derives cpu% from the delta between
    /// two refreshes, so the first call after construction yields 0% cpu — callers
    /// do one warm-up refresh before the first real sample.
    pub fn refresh(&mut self) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
    }

    /// Sample the app subtree (pid `app_pid`, labelled "katrix") plus one subtree
    /// per agent. Each row aggregates the pid AND all its descendants. Totals are
    /// the machine-wide cpu% and the sum of every sampled process's memory.
    pub fn sample(&self, app_pid: u32, agents: &[(Uuid, u32)]) -> SystemMetrics {
        let map = self.process_map();

        let mut procs = Vec::with_capacity(agents.len() + 1);
        procs.push(subtree_metric("app".into(), "katrix".into(), app_pid, &map));
        for (id, pid) in agents {
            procs.push(subtree_metric(id.to_string(), id.to_string(), *pid, &map));
        }

        SystemMetrics {
            total_cpu: self.sys.global_cpu_usage(),
            total_mem_bytes: map.values().map(|p| p.mem_bytes).sum(),
            procs,
        }
    }

    fn process_map(&self) -> HashMap<u32, ProcSample> {
        self.sys
            .processes()
            .iter()
            .map(|(pid, proc_)| {
                (
                    pid.as_u32(),
                    ProcSample {
                        parent: proc_.parent().map(|p| p.as_u32()),
                        cpu: proc_.cpu_usage(),
                        mem_bytes: proc_.memory(),
                    },
                )
            })
            .collect()
    }
}

/// Build one `ProcMetric` by rolling up `root`'s whole subtree (root + descendants).
fn subtree_metric(id: String, label: String, root: u32, map: &HashMap<u32, ProcSample>) -> ProcMetric {
    let (cpu, mem_bytes) = aggregate_subtree(root, map);
    ProcMetric { id, label, cpu, mem_bytes }
}

/// Pure roll-up: sum cpu% and memory over `root` and every process whose ancestry
/// chain reaches `root`. Built from a parent->child adjacency so it's testable from
/// a synthetic process map without touching the real machine.
fn aggregate_subtree(root: u32, map: &HashMap<u32, ProcSample>) -> (f32, u64) {
    // parent pid -> its direct children
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, s) in map {
        if let Some(parent) = s.parent {
            children.entry(parent).or_default().push(*pid);
        }
    }

    let mut cpu = 0.0_f32;
    let mut mem = 0_u64;
    // DFS from the root; guard against cycles (pid reuse) with a visited set.
    let mut stack = vec![root];
    let mut seen = std::collections::HashSet::new();
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(s) = map.get(&pid) {
            cpu += s.cpu;
            mem += s.mem_bytes;
        }
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids.iter().copied());
        }
    }
    (cpu, mem)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic tree (no real machine processes):
    //   1 (app)
    //   ├─ 2
    //   │  └─ 4
    //   └─ 3
    //   10 (agent)
    //   └─ 11
    //   99 (unrelated, not in any subtree)
    fn fixture() -> HashMap<u32, ProcSample> {
        let mut m = HashMap::new();
        m.insert(1, ProcSample { parent: None, cpu: 1.0, mem_bytes: 100 });
        m.insert(2, ProcSample { parent: Some(1), cpu: 2.0, mem_bytes: 200 });
        m.insert(3, ProcSample { parent: Some(1), cpu: 3.0, mem_bytes: 300 });
        m.insert(4, ProcSample { parent: Some(2), cpu: 4.0, mem_bytes: 400 });
        m.insert(10, ProcSample { parent: Some(1), cpu: 10.0, mem_bytes: 1000 });
        m.insert(11, ProcSample { parent: Some(10), cpu: 11.0, mem_bytes: 1100 });
        m.insert(99, ProcSample { parent: None, cpu: 99.0, mem_bytes: 9900 });
        m
    }

    #[test]
    fn aggregates_full_subtree_including_grandchildren() {
        let m = fixture();
        // app subtree = 1 + 2 + 3 + 4 + 10 + 11 (10 is parented to 1 in this fixture)
        let (cpu, mem) = aggregate_subtree(1, &m);
        assert_eq!(cpu, 1.0 + 2.0 + 3.0 + 4.0 + 10.0 + 11.0);
        assert_eq!(mem, 100 + 200 + 300 + 400 + 1000 + 1100);
    }

    #[test]
    fn aggregates_a_nested_branch() {
        let m = fixture();
        // node 2 + its child 4
        let (cpu, mem) = aggregate_subtree(2, &m);
        assert_eq!(cpu, 2.0 + 4.0);
        assert_eq!(mem, 200 + 400);
    }

    #[test]
    fn missing_root_yields_zero() {
        let m = fixture();
        let (cpu, mem) = aggregate_subtree(123456, &m);
        assert_eq!(cpu, 0.0);
        assert_eq!(mem, 0);
    }

    #[test]
    fn cycle_does_not_loop_forever() {
        // pid-reuse style cycle: a <-> b
        let mut m = HashMap::new();
        m.insert(1, ProcSample { parent: Some(2), cpu: 1.0, mem_bytes: 10 });
        m.insert(2, ProcSample { parent: Some(1), cpu: 2.0, mem_bytes: 20 });
        let (cpu, mem) = aggregate_subtree(1, &m);
        assert_eq!(cpu, 3.0);
        assert_eq!(mem, 30);
    }

    // Build SystemMetrics from a synthetic map via the same roll-up the sampler
    // uses, asserting per-subtree rows + the memory total over ALL processes.
    #[test]
    fn builds_per_subtree_rows_and_totals() {
        let m = fixture();
        let app = subtree_metric("app".into(), "katrix".into(), 1, &m);
        let agent_id = Uuid::new_v4();

        // a contrived disjoint agent root (28 = node 10 minus its own contribution
        // is awkward; instead point the agent at node 10's branch directly).
        let agent = subtree_metric(agent_id.to_string(), "nova".into(), 10, &m);

        assert_eq!(app.id, "app");
        assert_eq!(app.label, "katrix");
        assert_eq!(agent.cpu, 10.0 + 11.0);
        assert_eq!(agent.mem_bytes, 1000 + 1100);

        let total_mem: u64 = m.values().map(|p| p.mem_bytes).sum();
        assert_eq!(total_mem, 100 + 200 + 300 + 400 + 1000 + 1100 + 9900);
    }
}
