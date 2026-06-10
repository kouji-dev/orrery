use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use uuid::Uuid;

pub mod commands;

/// One-shot freshness window: the push loop refreshes every 5s while agents
/// run, so a snapshot at most one period old is as good as a fresh sweep.
pub const SNAPSHOT_FRESH: Duration = Duration::from_secs(5);

/// Scoped refreshes between full discovery sweeps. Scoped refreshes only touch
/// pids already known to belong to our subtrees, so brand-new children (agent
/// tools spawn workers constantly) are invisible until the next full sweep —
/// discovery lag = DISCOVERY_EVERY × cadence (5 s active → ~50 s; 20 s idle → ~200 s).
const DISCOVERY_EVERY: u32 = 10;

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
    /// Machine RAM in bytes — the denominator for the UI's memory gauge.
    pub sys_mem_bytes: u64,
    /// Logical core count — the denominator already applied to the cpu rows.
    pub cores: u32,
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
    /// Scoped refreshes since the last full sweep (drives `DISCOVERY_EVERY`).
    scoped_streak: u32,
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
            scoped_streak: 0,
        }
    }

    /// Full sweep: refresh cpu + memory for EVERY process on the machine. sysinfo
    /// derives cpu% from the delta between two refreshes, so the first call after
    /// construction yields 0% cpu — callers do one warm-up refresh before the
    /// first real sample. Costs hundreds of ms on a busy box, so steady-state
    /// ticks use `refresh_scoped` and only fall back here for discovery.
    pub fn refresh(&mut self) {
        self.scoped_streak = 0;
        // total machine RAM is static — refreshing it on the (rare) full sweep
        // keeps `total_memory()` valid for every scoped tick in between
        self.sys.refresh_memory();
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
    }

    /// Refresh only the subtrees we report on: each root (app pid + agent pids)
    /// plus every descendant already known from the last sweep. Falls back to a
    /// full sweep when a root is missing from the map (fresh agent — its children
    /// are unknown) or every `DISCOVERY_EVERY` calls (so children spawned since
    /// the last sweep get picked up). Dead pids in the scoped list are dropped by
    /// sysinfo (`remove_dead = true`), so the set self-cleans between sweeps.
    pub fn refresh_scoped(&mut self, roots: &[u32]) {
        let map = self.process_map();
        if needs_full_sweep(roots, &map, self.scoped_streak) {
            self.refresh();
            return;
        }
        self.scoped_streak += 1;
        let pids: Vec<Pid> = subtree_pids(roots, &map)
            .into_iter()
            .map(Pid::from_u32)
            .collect();
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
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
            sys_mem_bytes: self.sys.total_memory(),
            cores: self.cpu_count as u32,
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

/// Pure: collect `roots` plus every known descendant (the pids whose parent
/// chain reaches a root). Roots are included even when absent from the map so a
/// scoped refresh still adds a just-started process. Mirrors
/// `aggregate_subtree`'s walk; kept separate so the scoped-refresh pid list is
/// testable from synthetic data.
fn subtree_pids(roots: &[u32], map: &HashMap<u32, ProcSample>) -> Vec<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, s) in map {
        if let Some(parent) = s.parent {
            children.entry(parent).or_default().push(*pid);
        }
    }

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<u32> = roots.to_vec();
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        out.push(pid);
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids.iter().copied());
        }
    }
    out
}

/// Replace each agent row's label (defaulted to the uuid string) with the
/// agent's display name. The "app" row keeps its fixed label.
pub fn apply_labels(metrics: &mut SystemMetrics, labels: &HashMap<Uuid, String>) {
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
}

/// Warm sampler + last labelled snapshot, shared (managed state + a clone in
/// the push loop) between the loop and the one-shot `system_metrics` command.
/// The loop keeps the sampler warm and the snapshot fresh; the command serves
/// the cached snapshot when recent and otherwise does a single scoped refresh —
/// no cold sampler init, no double sweep, no warm-up sleep on the command path.
#[derive(Clone)]
pub struct SharedSampler {
    inner: Arc<Mutex<SharedState>>,
}

struct SharedState {
    sampler: MetricsSampler,
    snapshot: Option<(SystemMetrics, Instant)>,
}

impl Default for SharedSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedSampler {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SharedState {
                sampler: MetricsSampler::new(),
                snapshot: None,
            })),
        }
    }

    /// One full sweep so the next sample has a cpu delta to diff against
    /// (sysinfo's first refresh reads 0% cpu). Run once before the loop starts.
    pub fn warm_up(&self) {
        self.inner.lock().unwrap().sampler.refresh();
    }

    /// The cached snapshot, if one exists and is at most `max_age` old.
    pub fn cached(&self, max_age: Duration) -> Option<SystemMetrics> {
        let s = self.inner.lock().unwrap();
        s.snapshot
            .as_ref()
            .and_then(|(m, at)| (at.elapsed() <= max_age).then(|| m.clone()))
    }

    /// One scoped refresh on the warm sampler, then sample + label the app and
    /// agent subtrees. Caches the result for `cached` readers.
    pub fn refresh_and_sample(
        &self,
        app_pid: u32,
        pids: &[(Uuid, u32)],
        labels: &HashMap<Uuid, String>,
    ) -> SystemMetrics {
        let mut s = self.inner.lock().unwrap();
        let mut roots: Vec<u32> = Vec::with_capacity(pids.len() + 1);
        roots.push(app_pid);
        roots.extend(pids.iter().map(|(_, pid)| *pid));
        s.sampler.refresh_scoped(&roots);
        let mut m = s.sampler.sample(app_pid, pids);
        apply_labels(&mut m, labels);
        s.snapshot = Some((m.clone(), Instant::now()));
        m
    }

    #[cfg(test)]
    fn put_snapshot(&self, m: SystemMetrics) {
        self.inner.lock().unwrap().snapshot = Some((m, Instant::now()));
    }
}

/// Returns `true` when a full discovery sweep is required: either a root pid is
/// absent from the current process map (fresh agent whose children are unknown),
/// or the scoped streak has reached `DISCOVERY_EVERY` (pick up children spawned
/// since the last sweep). Pure — no side-effects, fully unit-testable.
fn needs_full_sweep(roots: &[u32], map: &HashMap<u32, ProcSample>, streak: u32) -> bool {
    roots.iter().any(|r| !map.contains_key(r)) || streak >= DISCOVERY_EVERY
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

    // The scoped-refresh pid list is the roots + every known descendant —
    // unrelated processes (99) are excluded; that's the whole point of scoping.
    #[test]
    fn subtree_pids_collects_roots_and_descendants_only() {
        let m = fixture();
        // node 2's branch (2 + 4) plus root 10's branch (10 + 11).
        let mut pids = subtree_pids(&[2, 10], &m);
        pids.sort_unstable();
        assert_eq!(pids, vec![2, 4, 10, 11]);
    }

    // A root absent from the map is still listed (a just-started process must be
    // picked up by the scoped refresh) and overlapping roots don't duplicate.
    #[test]
    fn subtree_pids_keeps_unknown_roots_and_dedups() {
        let m = fixture();
        let mut pids = subtree_pids(&[10, 11, 555], &m);
        pids.sort_unstable();
        assert_eq!(pids, vec![10, 11, 555]);
    }

    #[test]
    fn apply_labels_renames_agent_rows_only() {
        let id = Uuid::new_v4();
        let mut m = SystemMetrics {
            total_cpu: 0.0,
            total_mem_bytes: 0,
            sys_mem_bytes: 0,
            cores: 1,
            procs: vec![
                ProcMetric {
                    id: "app".into(),
                    label: "Orrery".into(),
                    cpu: 0.0,
                    mem_bytes: 0,
                },
                ProcMetric {
                    id: id.to_string(),
                    label: id.to_string(),
                    cpu: 0.0,
                    mem_bytes: 0,
                },
            ],
        };
        let labels: HashMap<Uuid, String> = [(id, "nova".to_string())].into();
        apply_labels(&mut m, &labels);
        assert_eq!(m.procs[0].label, "Orrery");
        assert_eq!(m.procs[1].label, "nova");
    }

    // --- needs_full_sweep predicate ------------------------------------------

    // An unknown root (not in the process map) always forces a full sweep,
    // regardless of streak position.
    #[test]
    fn sweep_predicate_unknown_root_forces_full_sweep() {
        let m = fixture();
        // pid 999 is not in fixture — unknown root → must sweep.
        assert!(needs_full_sweep(&[999], &m, 0));
        // Also true mid-streak.
        assert!(needs_full_sweep(&[999], &m, 5));
    }

    // When all roots are known and the streak is below the threshold, a scoped
    // refresh is sufficient (predicate returns false).
    #[test]
    fn sweep_predicate_known_roots_below_streak_is_scoped() {
        let m = fixture();
        // streak 0..9 with all-known roots → scoped is fine.
        for streak in 0..DISCOVERY_EVERY {
            assert!(
                !needs_full_sweep(&[1, 10], &m, streak),
                "expected scoped at streak={streak}"
            );
        }
    }

    // When the streak reaches DISCOVERY_EVERY the predicate flips to full-sweep
    // so that newly-spawned children get discovered.
    #[test]
    fn sweep_predicate_streak_at_threshold_forces_full_sweep() {
        let m = fixture();
        assert!(needs_full_sweep(&[1, 10], &m, DISCOVERY_EVERY));
        // And beyond (shouldn't normally happen, but guard anyway).
        assert!(needs_full_sweep(&[1, 10], &m, DISCOVERY_EVERY + 1));
    }

    // After a full sweep the MetricsSampler resets scoped_streak to 0, so the
    // next DISCOVERY_EVERY − 1 calls are scoped again.
    #[test]
    fn sweep_streak_resets_after_full_sweep() {
        let mut sampler = MetricsSampler::new();
        // Drive scoped_streak to DISCOVERY_EVERY − 1 manually via the field
        // (we only test the reset, not real process data).
        sampler.scoped_streak = DISCOVERY_EVERY - 1;
        // refresh() is the full-sweep path; it must reset the streak.
        sampler.refresh();
        assert_eq!(sampler.scoped_streak, 0);
    }

    // The one-shot command path: a fresh snapshot is served from cache; an aged
    // window (ZERO) misses, and an empty cache misses.
    #[test]
    fn shared_sampler_serves_only_fresh_snapshots() {
        let shared = SharedSampler::new();
        assert!(shared.cached(SNAPSHOT_FRESH).is_none());

        let snap = SystemMetrics {
            total_cpu: 1.5,
            total_mem_bytes: 42,
            sys_mem_bytes: 0,
            cores: 1,
            procs: vec![],
        };
        shared.put_snapshot(snap.clone());
        assert_eq!(shared.cached(SNAPSHOT_FRESH), Some(snap));
        // an elapsed() of any real duration exceeds a ZERO freshness window.
        assert!(shared.cached(Duration::ZERO).is_none());
    }
}
