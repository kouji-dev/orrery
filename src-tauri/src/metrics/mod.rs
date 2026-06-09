use std::collections::HashMap;

use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use uuid::Uuid;

pub mod commands;

/// One subtree's roll-up: an agent's process tree, or the app's own tree.
/// `id` is the agent uuid string (or "app"); `label` is the agent name (or "Orrery").
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcMetric {
    pub id: String,
    pub label: String,
    pub cpu: f32,
    pub mem_bytes: u64,
}

/// The whole snapshot pushed to the UI every tick. Totals are the SUM of the
/// per-subtree rows — i.e. the cpu% and memory used by orrery + its agents ONLY
/// (NOT machine-wide). `total_cpu` is machine-relative (a share of ALL logical
/// cores, like Task Manager), so it never exceeds 100.
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
    /// Logical core count — sysinfo reports per-process cpu% as one-core-relative
    /// (100% = one full core), so we divide the subtree sum by this to get a
    /// machine-relative % that matches Task Manager.
    cpu_count: usize,
}

impl Default for MetricsSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsSampler {
    pub fn new() -> Self {
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        Self {
            sys: System::new(),
            cpu_count,
        }
    }

    /// Refresh per-process cpu + memory. sysinfo derives cpu% from the delta
    /// between two refreshes, so the first call after construction yields 0% cpu —
    /// callers do one warm-up refresh before the first real sample.
    pub fn refresh(&mut self) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
    }

    /// Sample the app subtree (pid `app_pid`, labelled "Orrery") plus one subtree
    /// per agent. Each row aggregates the pid AND all its descendants — so the
    /// "Orrery" row rolls up the Rust core PLUS its WebView2 child processes (the UI
    /// renderer/GPU procs), which hold the bulk of the app's memory. Per-row cpu% is
    /// normalized by the logical core count (machine-relative, like Task Manager).
    /// Totals are the SUM of the rows — the cpu/memory used by orrery + its agents.
    pub fn sample(&self, app_pid: u32, agents: &[(Uuid, u32)]) -> SystemMetrics {
        let map = self.process_map();
        let cores = self.cpu_count.max(1) as f32;

        let mut procs = Vec::with_capacity(agents.len() + 1);
        procs.push(subtree_metric(
            "app".into(),
            "Orrery".into(),
            app_pid,
            &map,
            cores,
        ));
        for (id, pid) in agents {
            procs.push(subtree_metric(
                id.to_string(),
                id.to_string(),
                *pid,
                &map,
                cores,
            ));
        }

        SystemMetrics {
            total_cpu: procs.iter().map(|p| p.cpu).sum(),
            total_mem_bytes: procs.iter().map(|p| p.mem_bytes).sum(),
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
/// `cores` divides the summed (one-core-relative) cpu into a machine-relative %.
fn subtree_metric(
    id: String,
    label: String,
    root: u32,
    map: &HashMap<u32, ProcSample>,
    cores: f32,
) -> ProcMetric {
    let (cpu, mem_bytes) = aggregate_subtree(root, map);
    ProcMetric {
        id,
        label,
        cpu: cpu / cores,
        mem_bytes,
    }
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
        m.insert(
            1,
            ProcSample {
                parent: None,
                cpu: 1.0,
                mem_bytes: 100,
            },
        );
        m.insert(
            2,
            ProcSample {
                parent: Some(1),
                cpu: 2.0,
                mem_bytes: 200,
            },
        );
        m.insert(
            3,
            ProcSample {
                parent: Some(1),
                cpu: 3.0,
                mem_bytes: 300,
            },
        );
        m.insert(
            4,
            ProcSample {
                parent: Some(2),
                cpu: 4.0,
                mem_bytes: 400,
            },
        );
        m.insert(
            10,
            ProcSample {
                parent: Some(1),
                cpu: 10.0,
                mem_bytes: 1000,
            },
        );
        m.insert(
            11,
            ProcSample {
                parent: Some(10),
                cpu: 11.0,
                mem_bytes: 1100,
            },
        );
        m.insert(
            99,
            ProcSample {
                parent: None,
                cpu: 99.0,
                mem_bytes: 9900,
            },
        );
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
        m.insert(
            1,
            ProcSample {
                parent: Some(2),
                cpu: 1.0,
                mem_bytes: 10,
            },
        );
        m.insert(
            2,
            ProcSample {
                parent: Some(1),
                cpu: 2.0,
                mem_bytes: 20,
            },
        );
        let (cpu, mem) = aggregate_subtree(1, &m);
        assert_eq!(cpu, 3.0);
        assert_eq!(mem, 30);
    }

    // Build the per-subtree rows from a synthetic map via the same roll-up the
    // sampler uses. (Machine totals — used/total RAM, global cpu — come straight
    // from `sysinfo` and aren't derived from this map, so they're not asserted here.)
    #[test]
    fn builds_per_subtree_rows() {
        let m = fixture();
        // cores = 1.0 → cpu stays the raw subtree sum (the /cores normalization is a
        // no-op here, so the roll-up math is asserted directly).
        let app = subtree_metric("app".into(), "Orrery".into(), 1, &m, 1.0);
        let agent_id = Uuid::new_v4();

        // point the agent row at node 10's branch directly.
        let agent = subtree_metric(agent_id.to_string(), "nova".into(), 10, &m, 1.0);

        assert_eq!(app.id, "app");
        assert_eq!(app.label, "Orrery");
        assert_eq!(agent.cpu, 10.0 + 11.0);
        assert_eq!(agent.mem_bytes, 1000 + 1100);
    }

    // cpu% is normalized by the logical core count → a subtree summing to 20%
    // one-core-relative reports 1% on a 20-core machine (machine-relative, like
    // Task Manager).
    #[test]
    fn cpu_is_normalized_by_core_count() {
        let m = fixture();
        // node 10 + child 11 = 10.0 + 11.0 = 21.0 raw; /20 cores = 1.05.
        let row = subtree_metric("a".into(), "nova".into(), 10, &m, 20.0);
        assert!((row.cpu - (21.0 / 20.0)).abs() < f32::EPSILON);
        // memory is NOT divided by cores.
        assert_eq!(row.mem_bytes, 1000 + 1100);
    }
}
