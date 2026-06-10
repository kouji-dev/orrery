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
/// Per-second (calls, bytes) buckets kept per command — a bit more than the
/// window so rates stay exact. Why not count ring samples: the 128-sample
/// latency ring saturates above ~12.8 calls/s, which batched PTY emits exceed
/// by an order of magnitude; buckets keep counts exact at any rate.
const BUCKETS: usize = 16;

#[derive(Default)]
struct CmdAgg {
    /// (epoch ms, micros) — newest last, capped at RING. Latency profile only.
    samples: Vec<(u128, u32)>,
    /// (epoch second, calls, payload bytes) — newest last, capped at BUCKETS.
    buckets: Vec<(u128, u32, u64)>,
}

fn bump_bucket(agg: &mut CmdAgg, now: u128, calls: u32, bytes: u64) {
    let sec = now / 1000;
    if let Some(last) = agg.buckets.last_mut() {
        if last.0 == sec {
            last.1 += calls;
            last.2 += bytes;
            return;
        }
    }
    agg.buckets.push((sec, calls, bytes));
    if agg.buckets.len() > BUCKETS {
        agg.buckets.remove(0);
    }
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
    /// Payload bytes in the window — only for rows that record volume
    /// (e.g. batched PTY emits, where throughput matters more than latency).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes10s: Option<u64>,
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
    let now = now_ms();
    let mut m = stats().lock().unwrap();
    let agg = m.entry(cmd).or_default();
    agg.samples
        .push((now, elapsed.as_micros().min(u32::MAX as u128) as u32));
    if agg.samples.len() > RING {
        agg.samples.remove(0);
    }
    bump_bucket(agg, now, 1, 0);
}

/// Record payload bytes for one emission — the throughput dimension for
/// push/emit rows whose cost is volume, not round-trip latency. O(1).
pub fn record_volume(cmd: &'static str, bytes: u64) {
    let now = now_ms();
    let mut m = stats().lock().unwrap();
    let agg = m.entry(cmd).or_default();
    bump_bucket(agg, now, 0, bytes);
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
    let now_sec = now / 1000;
    m.iter()
        .filter(|(_, agg)| !agg.samples.is_empty())
        .map(|(cmd, agg)| {
            let mut ms: Vec<f64> = agg
                .samples
                .iter()
                .map(|(_, us)| *us as f64 / 1000.0)
                .collect();
            ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let (calls10s, bytes10s) = agg
                .buckets
                .iter()
                .filter(|(sec, _, _)| now_sec.saturating_sub(*sec) < WINDOW_MS / 1000)
                .fold((0u32, 0u64), |(c, b), (_, cc, bb)| (c + cc, b + bb));
            let p95_idx = ((ms.len() as f64 * 0.95).ceil() as usize).clamp(1, ms.len()) - 1;
            CmdExec {
                cmd: (*cmd).to_string(),
                calls10s,
                avg_ms: ms.iter().sum::<f64>() / ms.len() as f64,
                p95_ms: ms[p95_idx],
                max_ms: *ms.last().unwrap(),
                bytes10s: (bytes10s > 0).then_some(bytes10s),
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

    // Why: the registry is process-global and reset() in one test races counts
    // in the others (the long-standing suite flake). Serialize this module's
    // tests so assertions can be exact; tolerate poison from a panicked test.
    static TEST_SERIAL: Mutex<()> = Mutex::new(());
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        TEST_SERIAL.lock().unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn timed_records_and_snapshot_aggregates() {
        let _serial = serial();
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
        let _serial = serial();
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
        assert_eq!(
            row.calls10s, 800,
            "bucket counting stays exact under concurrency, beyond the latency ring"
        );
    }

    #[test]
    fn volume_and_high_rate_counts_are_exact() {
        let _serial = serial();
        // batched PTY emits exceed the 128-sample latency ring by an order of
        // magnitude — counts and byte volume must come from exact buckets
        for _ in 0..300 {
            record("t_vol", Duration::from_micros(10));
            record_volume("t_vol", 100);
        }
        let snap = snapshot();
        let row = snap.iter().find(|r| r.cmd == "t_vol").unwrap();
        assert_eq!(row.calls10s, 300, "exact count above the latency-ring cap");
        assert_eq!(row.bytes10s, Some(30_000), "window byte volume summed");
    }

    #[test]
    fn rows_without_volume_have_no_bytes10s() {
        let _serial = serial();
        record("t_novol", Duration::from_micros(10));
        let snap = snapshot();
        let row = snap.iter().find(|r| r.cmd == "t_novol").unwrap();
        assert_eq!(row.bytes10s, None, "latency-only rows skip the field");
    }

    #[test]
    fn reset_empties_the_registry() {
        let _serial = serial();
        record("t_reset", Duration::from_micros(10));
        assert!(snapshot().iter().any(|r| r.cmd == "t_reset"));
        reset();
        assert!(snapshot().iter().all(|r| r.cmd != "t_reset"));
    }
}
