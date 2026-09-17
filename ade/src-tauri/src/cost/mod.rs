//! Total Claude usage cost, read from the `ccusage` CLI. We don't persist
//! anything — ccusage reads the on-disk transcripts fresh each run, so the total
//! survives app restarts and agent deletion. Global all-time total (all Claude
//! Code usage on the machine, not just orrery agents).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

pub mod commands;

/// Kill switch for the cost feature (twin of `COST_FEATURES_ENABLED` in
/// `src/app/cost/cost-flags.ts`). When off, the ccusage push loop is never
/// spawned — `system://cost` never fires, the one-shot `system_cost` peeks an
/// always-cold cache and returns `None`, and the UI readout stays hidden.
pub const COST_FEATURES_ENABLED: bool = false;

/// Serve a cached total this long before shelling out again. Cost moves slowly
/// and one ccusage run costs seconds (cmd -> npx -> node + full transcript
/// scan), so the push loop's 5-minute cadence is also the freshness bound.
pub const COST_FRESH: Duration = Duration::from_secs(300);

/// Cached ccusage snapshot shared (managed state + a clone in the push loop)
/// between the loop and the one-shot `system_cost` command. The loop is the
/// only runner: the one-shot [`peek`](CostCache::peek)s and never blocks or
/// spawns (A1.8). `run` still serializes any concurrent `get` callers so a
/// second caller waits for the in-flight run's result instead of spawning a
/// second node process.
#[derive(Clone)]
pub struct CostCache {
    inner: Arc<CostCacheInner>,
}

struct CostCacheInner {
    /// Injected so tests can count runs without npx/node on the machine.
    runner: Box<dyn Fn() -> CostSnapshot + Send + Sync>,
    last: Mutex<Option<(CostSnapshot, Instant)>>,
    /// Held for the whole ccusage run: a concurrent caller blocks here, then
    /// finds the finished run's result in `last` on the re-check.
    run: Mutex<()>,
}

impl Default for CostCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CostCache {
    pub fn new() -> Self {
        Self::with_runner(Box::new(snapshot))
    }

    pub fn with_runner(runner: Box<dyn Fn() -> CostSnapshot + Send + Sync>) -> Self {
        Self {
            inner: Arc::new(CostCacheInner {
                runner,
                last: Mutex::new(None),
                run: Mutex::new(()),
            }),
        }
    }

    /// The cached snapshot if younger than `max_age`, else one (deduped) run.
    /// Blocking — callers sit on the blocking pool / a dedicated thread.
    pub fn get(&self, max_age: Duration) -> CostSnapshot {
        if let Some(s) = self.cached(max_age) {
            return s;
        }
        let _run = self.inner.run.lock().unwrap();
        // Re-check under the run lock: if we blocked behind an in-flight run,
        // its just-stored result is what we would have computed — serve it.
        if let Some(s) = self.cached(max_age) {
            return s;
        }
        let snap = (self.inner.runner)();
        *self.inner.last.lock().unwrap() = Some((snap, Instant::now()));
        snap
    }

    /// The cached snapshot if younger than `max_age`, WITHOUT ever running
    /// ccusage — `None` on a cold or stale cache. This is the startup one-shot's
    /// path (A1.8): the push loop's first run is already in flight when the
    /// webview asks, so the one-shot must not park a thread behind the run lock
    /// (or worse, trigger a second multi-second node process) just to duplicate
    /// the `system://cost` emit the loop is about to make.
    pub fn peek(&self, max_age: Duration) -> Option<CostSnapshot> {
        self.cached(max_age)
    }

    fn cached(&self, max_age: Duration) -> Option<CostSnapshot> {
        self.inner
            .last
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|(s, at)| (at.elapsed() <= max_age).then_some(*s))
    }
}

/// A cost snapshot pushed to the UI. `available` is false when ccusage could not
/// run (no Node / not installed / bad output) — the UI then hides the readout.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CostSnapshot {
    pub total_cost: f64,
    pub currency: &'static str,
    pub available: bool,
}

impl CostSnapshot {
    pub fn unavailable() -> Self {
        Self {
            total_cost: 0.0,
            currency: "USD",
            available: false,
        }
    }
    pub fn usd(total: f64) -> Self {
        Self {
            total_cost: total,
            currency: "USD",
            available: true,
        }
    }
}

/// Pull `.totals.totalCost` out of a `ccusage daily --json` blob. `None` when the
/// JSON is malformed or the field is missing/non-numeric.
pub fn parse_total(json: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("totals")?.get("totalCost")?.as_f64()
}

/// Run `ccusage daily --json` and return the global total cost, or `None` if it
/// can't run / parse. Invokes via `npx`; on Windows `npx` is a `.cmd`, so it goes
/// through `cmd /C`.
pub fn read_total() -> Option<f64> {
    let output = run_ccusage()?;
    if !output.status.success() {
        log::warn!("ccusage exited non-zero");
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_total(&stdout)
}

#[cfg(windows)]
fn run_ccusage() -> Option<std::process::Output> {
    use std::os::windows::process::CommandExt;
    let mut cmd = crate::core::proc::cmd("cmd");
    cmd.args(["/C", "npx", "ccusage", "daily", "--json"]);
    // Cost is pure housekeeping — the node transcript scan must lose every CPU
    // contest against the UI and agents. Flags REPLACE (see core::proc), so
    // NO_WINDOW must be restated.
    cmd.creation_flags(
        crate::core::proc::CREATE_NO_WINDOW | crate::core::proc::BELOW_NORMAL_PRIORITY_CLASS,
    );
    cmd.output().ok()
}

#[cfg(not(windows))]
fn run_ccusage() -> Option<std::process::Output> {
    crate::core::proc::cmd("npx")
        .args(["ccusage", "daily", "--json"])
        .output()
        .ok()
}

/// One snapshot: the global total, or `unavailable()` when ccusage can't run.
pub fn snapshot() -> CostSnapshot {
    match read_total() {
        Some(t) => CostSnapshot::usd(t),
        None => CostSnapshot::unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_total_reads_totals_total_cost() {
        let blob = r#"{"daily":[{"date":"2026-06-07","totalCost":1.5}],"totals":{"inputTokens":10,"outputTokens":20,"totalCost":12.34}}"#;
        assert_eq!(parse_total(blob), Some(12.34));
    }

    #[test]
    fn parse_total_none_on_garbage_or_missing() {
        assert_eq!(parse_total("not json"), None);
        assert_eq!(parse_total(r#"{"daily":[]}"#), None); // no totals
        assert_eq!(parse_total(r#"{"totals":{}}"#), None); // no totalCost
    }

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A CostCache whose runner counts invocations instead of shelling out.
    fn counting_cache(delay: Duration) -> (CostCache, Arc<AtomicUsize>) {
        let runs = Arc::new(AtomicUsize::new(0));
        let r = runs.clone();
        let cache = CostCache::with_runner(Box::new(move || {
            r.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(delay);
            CostSnapshot::usd(7.0)
        }));
        (cache, runs)
    }

    #[test]
    fn cache_serves_fresh_snapshot_without_rerunning() {
        let (cache, runs) = counting_cache(Duration::ZERO);
        assert_eq!(cache.get(COST_FRESH), CostSnapshot::usd(7.0));
        assert_eq!(cache.get(COST_FRESH), CostSnapshot::usd(7.0));
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cache_reruns_when_stale() {
        let (cache, runs) = counting_cache(Duration::ZERO);
        // a ZERO freshness window makes every cached entry stale immediately.
        cache.get(Duration::ZERO);
        cache.get(Duration::ZERO);
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    // A1.8: the startup one-shot peeks — it must NEVER trigger a run (the push
    // loop owns running ccusage). Cold cache → None and zero runs; once the
    // loop has stored a snapshot, peek serves it; stale → None again.
    #[test]
    fn peek_never_runs_and_serves_only_a_fresh_cache() {
        let (cache, runs) = counting_cache(Duration::ZERO);
        assert_eq!(cache.peek(COST_FRESH), None, "cold cache peeks empty");
        assert_eq!(runs.load(Ordering::SeqCst), 0, "peek spawned nothing");
        cache.get(COST_FRESH); // the push loop's run
        assert_eq!(cache.peek(COST_FRESH), Some(CostSnapshot::usd(7.0)));
        assert_eq!(cache.peek(Duration::ZERO), None, "stale cache peeks empty");
        assert_eq!(runs.load(Ordering::SeqCst), 1, "still only the loop's run");
    }

    // The startup race: push loop + one-shot command both ask at once. The
    // second caller must block behind the in-flight run and reuse its result —
    // exactly one node process, both callers get the snapshot.
    #[test]
    fn concurrent_callers_share_one_run() {
        let (cache, runs) = counting_cache(Duration::from_millis(100));
        let a = {
            let c = cache.clone();
            std::thread::spawn(move || c.get(COST_FRESH))
        };
        let b = {
            let c = cache.clone();
            std::thread::spawn(move || c.get(COST_FRESH))
        };
        assert_eq!(a.join().unwrap(), CostSnapshot::usd(7.0));
        assert_eq!(b.join().unwrap(), CostSnapshot::usd(7.0));
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }
}
