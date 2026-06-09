//! Per-command exec timing (the Rust half of the perf spec): a global registry
//! of recent (ts, duration) samples per command, pushed to the webview every 2s
//! on `perf://stats`. O(1) per call; in-memory only; never uploaded.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Per-command sample ring cap (mirrors the frontend PerfStore ring).
pub(crate) const RING: usize = 128;
/// Rolling rate window (mirrors the frontend PERF_WINDOW_MS).
const WINDOW_MS: u128 = 10_000;

#[derive(Default)]
struct CmdAgg {
    /// (epoch ms, micros) — newest last, capped at RING.
    samples: Vec<(u128, u32)>,
}

/// One command's exec aggregate, shaped for the frontend `ExecAgg`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdExec {
    pub cmd: String,
    pub calls10s: u32,
    pub avg_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

static STATS: OnceLock<Mutex<HashMap<&'static str, CmdAgg>>> = OnceLock::new();
fn stats() -> &'static Mutex<HashMap<&'static str, CmdAgg>> {
    STATS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// Record one exec sample. O(1).
pub fn record(cmd: &'static str, elapsed: Duration) {
    let mut m = stats().lock().unwrap();
    let agg = m.entry(cmd).or_default();
    agg.samples
        .push((now_ms(), elapsed.as_micros().min(u32::MAX as u128) as u32));
    if agg.samples.len() > RING {
        agg.samples.remove(0);
    }
}

/// Run `f`, recording its wall time under `cmd` — the command wrappers use this.
pub fn timed<T>(cmd: &'static str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    record(cmd, t.elapsed());
    out
}

/// Current per-command aggregates: whole ring for latency (so a row keeps its
/// profile while idle), 10s window for the call rate.
pub fn snapshot() -> Vec<CmdExec> {
    let m = stats().lock().unwrap();
    let now = now_ms();
    m.iter()
        .filter(|(_, agg)| !agg.samples.is_empty())
        .map(|(cmd, agg)| {
            let mut ms: Vec<f64> = agg
                .samples
                .iter()
                .map(|(_, us)| *us as f64 / 1000.0)
                .collect();
            ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let calls10s = agg
                .samples
                .iter()
                .filter(|(ts, _)| now.saturating_sub(*ts) < WINDOW_MS)
                .count();
            let p95_idx = ((ms.len() as f64 * 0.95).ceil() as usize).clamp(1, ms.len()) - 1;
            CmdExec {
                cmd: (*cmd).to_string(),
                calls10s: calls10s as u32,
                avg_ms: ms.iter().sum::<f64>() / ms.len() as f64,
                p95_ms: ms[p95_idx],
                max_ms: *ms.last().unwrap(),
            }
        })
        .collect()
}

/// Drop all samples (tests).
#[cfg(test)]
pub(crate) fn reset() {
    stats().lock().unwrap().clear();
}

/// Emit `perf://stats` every 2s while the app runs (skipped when empty).
pub fn spawn_push_loop<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        let snap = snapshot();
        if !snap.is_empty() {
            use tauri::Emitter;
            let _ = app.emit("perf://stats", &snap);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // NOTE: these tests share the global registry with each other (cargo runs
    // them in one process) — they use distinct cmd keys so reset() races stay
    // harmless.

    #[test]
    fn timed_records_and_snapshot_aggregates() {
        timed("t_cmd", || std::thread::sleep(Duration::from_millis(2)));
        timed("t_cmd", || std::thread::sleep(Duration::from_millis(4)));
        let snap = snapshot();
        let row = snap
            .iter()
            .find(|r| r.cmd == "t_cmd")
            .expect("t_cmd present");
        assert_eq!(row.calls10s, 2);
        assert!(row.avg_ms >= 2.0, "avg {} >= 2ms", row.avg_ms);
        assert!(row.max_ms >= row.avg_ms);
        assert!(row.p95_ms >= row.avg_ms);
    }

    #[test]
    fn ring_caps_and_concurrent_pushes_dont_panic() {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..100 {
                        record("t_conc", Duration::from_micros(50));
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let snap = snapshot();
        let row = snap.iter().find(|r| r.cmd == "t_conc").unwrap();
        assert!(row.calls10s as usize <= RING, "window count capped by ring");
    }

    #[test]
    fn reset_empties_the_registry() {
        record("t_reset", Duration::from_micros(10));
        assert!(snapshot().iter().any(|r| r.cmd == "t_reset"));
        reset();
        assert!(snapshot().iter().all(|r| r.cmd != "t_reset"));
    }
}
