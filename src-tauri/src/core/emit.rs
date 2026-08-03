//! The one emit funnel (A0.7 Phase 1): every Rust→frontend push goes through
//! [`emit_tracked`], which RECORDS (name + payload byte count only) and then
//! forwards to `tauri::Emitter::emit`. Raw `app.emit` is banned repo-wide via
//! clippy `disallowed-methods` (same enforcement as `Command::new` → proc.rs).
//!
//! Two recording modes with opposite costs:
//! - **Aggregate** (always on): per event NAME — count, total/max bytes,
//!   p50/p95 payload size, calls/10s. In-memory counters behind one short
//!   mutex; flushed periodically to `telemetry/emit-summary-YYYY-MM-DD.json`.
//! - **Raw trace** (opt-in, bounded): one NDJSON line per emit
//!   (`ts · name · key · bytes`) appended to `telemetry/emits-YYYY-MM-DD.ndjson`.
//!   Auto-disables after 30 min or 200 MB — tracing a flood must never become
//!   the flood.
//!
//! PRIVACY — non-negotiable: names, keys and byte counts ONLY. Payload
//! contents (source code, prompts, diffs) are NEVER written to disk here.
//! Aggregation is by event NAME, never by key — keys are per-agent and would
//! grow an unbounded counter map; only the raw trace may carry keys.

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

// ---------------------------------------------------------------- constants

/// Why 256: enough samples for a stable p95 at the highest sustained emit rate
/// (the mux caps frames at ~62/s, so this is ~4s of the worst flood) while the
/// per-name ring stays a few KB.
const SIZE_RING: usize = 256;
/// Why 16 one-second buckets for a 10s rate window: mirrors perf::BUCKETS —
/// counts stay exact at any rate (a sample ring saturates under flood).
const RATE_BUCKETS: usize = 16;
/// Rolling rate window (mirrors the perf table's 10s window).
const RATE_WINDOW_S: u128 = 10;
/// Why 30 min / 200 MB: roadmap open decision 9 — stopping (not rotating) is
/// safer; a trace left on during a week of floods is its own perf problem.
const TRACE_MAX_MS: u64 = 30 * 60 * 1000;
const TRACE_MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Why 50 MB / 7 days: the acceptance-budget cap for total telemetry footprint;
/// oldest files pruned first, daily rotation.
const DISK_CAP_BYTES: u64 = 50 * 1024 * 1024;
const KEEP_DAYS: u64 = 7;
/// Why 60s: the summary is a cumulative day file, so flush cadence only bounds
/// data lost on a crash — one write/min is noise next to what it measures.
const FLUSH_EVERY_S: u64 = 60;

// ---------------------------------------------------------------- aggregate

#[derive(Default)]
struct NameAgg {
    count: u64,
    total_bytes: u64,
    max_bytes: u64,
    /// Last SIZE_RING payload sizes (bytes) — p50/p95 source. Ring, newest last.
    sizes: Vec<u32>,
    /// (epoch second, calls) — newest last, capped at RATE_BUCKETS.
    buckets: Vec<(u128, u32)>,
    /// Lifetime max of any one-second bucket — the summary's "peak rate".
    peak_per_sec: u32,
}

static REG: OnceLock<Mutex<HashMap<String, NameAgg>>> = OnceLock::new();
fn reg() -> &'static Mutex<HashMap<String, NameAgg>> {
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// `io::Write` sink that only counts — measuring payload size without
/// allocating a buffer on the hot path (`serde_json::to_writer` writes
/// straight into it).
struct ByteCounter(u64);
impl std::io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn record(name: &str, key: Option<&str>, bytes: u64) {
    let now = now_ms();
    {
        let mut m = reg().lock().unwrap();
        // Why contains_key-then-get_mut (two lookups): `entry()` would demand
        // `name.to_string()` on EVERY call — this shape keeps the steady state
        // (name already known) allocation-free, which is the hot-path budget.
        if !m.contains_key(name) {
            m.insert(name.to_string(), NameAgg::default());
        }
        let agg = m.get_mut(name).expect("just inserted");
        agg.count += 1;
        agg.total_bytes += bytes;
        agg.max_bytes = agg.max_bytes.max(bytes);
        let b = bytes.min(u32::MAX as u64) as u32;
        agg.sizes.push(b);
        if agg.sizes.len() > SIZE_RING {
            agg.sizes.remove(0);
        }
        let sec = now / 1000;
        match agg.buckets.last_mut() {
            Some(last) if last.0 == sec => last.1 += 1,
            _ => {
                agg.buckets.push((sec, 1));
                if agg.buckets.len() > RATE_BUCKETS {
                    agg.buckets.remove(0);
                }
            }
        }
        if let Some(last) = agg.buckets.last() {
            agg.peak_per_sec = agg.peak_per_sec.max(last.1);
        }
    }
    // Raw trace: opt-in, bounded — checked with one relaxed atomic load so the
    // always-on path pays nothing when the trace is off.
    if TRACE_ON.load(Ordering::Relaxed) {
        trace_append(now, name, key, bytes);
    }
}

/// Record + forward one event. The Phase-1 funnel: this is the ONLY sanctioned
/// way to emit (clippy bans `Emitter::emit` everywhere else). Phase 2's
/// scheduler becomes a change inside this module, not a second refactor.
pub fn emit_tracked<R: Runtime, S: Serialize>(
    app: &AppHandle<R>,
    name: &str,
    payload: &S,
) -> tauri::Result<()> {
    emit_keyed(app, name, None, payload)
}

/// Same as [`emit_tracked`], carrying a per-agent `key` for the raw trace.
/// The AGGREGATE never sees the key (unbounded cardinality); only the opt-in
/// trace lines do.
pub fn emit_keyed<R: Runtime, S: Serialize>(
    app: &AppHandle<R>,
    name: &str,
    key: Option<&str>,
    payload: &S,
) -> tauri::Result<()> {
    // Measure size by serializing into a counting sink — no buffer allocated.
    // This is one extra serialization per emit; small payloads stay well under
    // the µs budget, and for bulk payloads the cost is dwarfed by the emit's
    // own serialize+dispatch.
    let mut counter = ByteCounter(0);
    let _ = serde_json::to_writer(&mut counter, payload);
    record(name, key, counter.0);
    #[allow(clippy::disallowed_methods)] // the funnel itself — sole sanctioned call site
    app.emit(name, payload)
}

// ---------------------------------------------------------------- snapshot

/// One aggregate row, shaped for the dev panel's Emits table and the summary
/// file. camelCase for the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmitAggRow {
    pub name: String,
    pub count: u64,
    pub total_bytes: u64,
    pub max_bytes: u64,
    pub p50_bytes: u32,
    pub p95_bytes: u32,
    /// Calls in the rolling 10s window.
    pub calls10s: u32,
    /// Lifetime max calls in any one second — "peak rate".
    pub peak_per_sec: u32,
}

fn percentile(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1;
    sorted[idx]
}

/// Current aggregate table (session-cumulative counts, rolling 10s rate).
pub fn snapshot() -> Vec<EmitAggRow> {
    let m = reg().lock().unwrap();
    let now_sec = now_ms() / 1000;
    m.iter()
        .map(|(name, agg)| {
            let mut sizes = agg.sizes.clone();
            sizes.sort_unstable();
            let calls10s = agg
                .buckets
                .iter()
                .filter(|(sec, _)| now_sec.saturating_sub(*sec) < RATE_WINDOW_S)
                .map(|(_, c)| *c)
                .sum();
            EmitAggRow {
                name: name.clone(),
                count: agg.count,
                total_bytes: agg.total_bytes,
                max_bytes: agg.max_bytes,
                p50_bytes: percentile(&sizes, 0.50),
                p95_bytes: percentile(&sizes, 0.95),
                calls10s,
                peak_per_sec: agg.peak_per_sec,
            }
        })
        .collect()
}

// ---------------------------------------------------------------- raw trace

static TRACE_ON: AtomicBool = AtomicBool::new(false);
static TRACE_STARTED_MS: AtomicU64 = AtomicU64::new(0);
static TRACE_BYTES: AtomicU64 = AtomicU64::new(0);

struct TraceFile {
    /// "YYYY-MM-DD" the open file belongs to — mismatch rotates.
    day: String,
    file: std::io::BufWriter<fs::File>,
}

static TRACE_FILE: Mutex<Option<TraceFile>> = Mutex::new(None);
static TELEMETRY_DIR: OnceLock<PathBuf> = OnceLock::new();
/// `(active, reason)` — set at init; emits `telemetry://trace` to the UI and
/// persists an auto-disable back into settings so the toggle doesn't lie.
type TraceNotify = Box<dyn Fn(bool, &str) + Send + Sync>;
static NOTIFY: OnceLock<TraceNotify> = OnceLock::new();

/// Days since the unix epoch → "YYYY-MM-DD" (UTC). Hand-rolled (civil-from-days)
/// to avoid pulling `chrono` for one date string.
fn today() -> String {
    let days = (now_ms() / 86_400_000) as i64;
    // Howard Hinnant's civil_from_days algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn trace_append(ts: u128, name: &str, key: Option<&str>, bytes: u64) {
    let Some(dir) = TELEMETRY_DIR.get() else {
        return; // init not run (tests) — aggregate still works, trace is a no-op
    };
    let mut guard = TRACE_FILE.lock().unwrap();
    let day = today();
    let rotate = match guard.as_ref() {
        Some(t) => t.day != day,
        None => true,
    };
    if rotate {
        let _ = fs::create_dir_all(dir);
        prune(dir);
        match fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("emits-{day}.ndjson")))
        {
            Ok(f) => {
                *guard = Some(TraceFile {
                    day,
                    file: std::io::BufWriter::new(f),
                })
            }
            Err(e) => {
                log::warn!("emit trace file open failed: {e}");
                return;
            }
        }
    }
    let Some(t) = guard.as_mut() else { return };
    // names/keys/byte counts ONLY — never payload contents.
    let line = match key {
        Some(k) => format!("{{\"ts\":{ts},\"name\":{:?},\"key\":{k:?},\"bytes\":{bytes}}}\n", name),
        None => format!("{{\"ts\":{ts},\"name\":{:?},\"bytes\":{bytes}}}\n", name),
    };
    let n = line.len() as u64;
    if t.file.write_all(line.as_bytes()).is_ok() {
        let total = TRACE_BYTES.fetch_add(n, Ordering::Relaxed) + n;
        let started = TRACE_STARTED_MS.load(Ordering::Relaxed);
        let elapsed = (now_ms() as u64).saturating_sub(started);
        if total > TRACE_MAX_BYTES || elapsed > TRACE_MAX_MS {
            drop(guard); // set_raw_trace re-locks TRACE_FILE to flush
            auto_disable(if total > TRACE_MAX_BYTES {
                "size cap (200MB)"
            } else {
                "time cap (30min)"
            });
        }
    }
}

fn auto_disable(reason: &str) {
    if TRACE_ON.swap(false, Ordering::Relaxed) {
        log::info!("raw emit trace auto-disabled: {reason}");
        flush_trace_file();
        if let Some(n) = NOTIFY.get() {
            n(false, reason);
        }
    }
}

fn flush_trace_file() {
    if let Some(t) = TRACE_FILE.lock().unwrap().as_mut() {
        let _ = t.file.flush();
    }
}

/// Turn the raw trace on/off (settings toggle → `settings_set`; also the init
/// restore path). Idempotent.
pub fn set_raw_trace(on: bool) {
    let was = TRACE_ON.swap(on, Ordering::Relaxed);
    if was == on {
        return;
    }
    if on {
        TRACE_STARTED_MS.store(now_ms() as u64, Ordering::Relaxed);
        TRACE_BYTES.store(0, Ordering::Relaxed);
        log::info!("raw emit trace ON (auto-off after 30min / 200MB)");
    } else {
        flush_trace_file();
        log::info!("raw emit trace OFF (user)");
    }
    if let Some(n) = NOTIFY.get() {
        n(on, "user");
    }
}

/// Trace state for the UI indicator (status-bar chip + Emits tab).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceState {
    pub active: bool,
    /// Epoch ms the current window started (0 when inactive).
    pub started_ms: u64,
    /// Bytes written in the current window.
    pub bytes: u64,
}

pub fn trace_state() -> TraceState {
    let active = TRACE_ON.load(Ordering::Relaxed);
    TraceState {
        active,
        started_ms: if active {
            TRACE_STARTED_MS.load(Ordering::Relaxed)
        } else {
            0
        },
        bytes: if active {
            TRACE_BYTES.load(Ordering::Relaxed)
        } else {
            0
        },
    }
}

// ---------------------------------------------------------------- disk

/// Keep the telemetry dir within budget: files older than KEEP_DAYS go first,
/// then oldest-first until the 50MB cap holds. Runs on trace rotation and on
/// each summary flush — cheap (a dozen dirents).
fn prune(dir: &std::path::Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime, u64)> = entries
        .flatten()
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            md.is_file()
                .then(|| (e.path(), md.modified().unwrap_or(UNIX_EPOCH), md.len()))
        })
        .collect();
    files.sort_by_key(|(_, t, _)| *t); // oldest first
    let now = SystemTime::now();
    let mut total: u64 = files.iter().map(|(_, _, s)| s).sum();
    for (path, modified, size) in &files {
        let too_old = now
            .duration_since(*modified)
            .map(|d| d.as_secs() > KEEP_DAYS * 86_400)
            .unwrap_or(false);
        if too_old || total > DISK_CAP_BYTES {
            if fs::remove_file(path).is_ok() {
                total = total.saturating_sub(*size);
            }
        }
    }
}

/// Wire the telemetry dir + trace-change notifier and start the summary flush
/// thread. Called once from setup; before this, emits are still tracked
/// in-memory (nothing is lost) — they just aren't flushed to disk yet.
pub fn init(dir: PathBuf, raw_trace_on: bool, notify: TraceNotify) {
    let _ = NOTIFY.set(notify);
    let _ = TELEMETRY_DIR.set(dir.clone());
    if raw_trace_on {
        set_raw_trace(true);
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(FLUSH_EVERY_S));
        flush_summary(&dir);
        flush_trace_file(); // bound trace data-loss on crash to one flush period
    });
}

/// Write the cumulative day summary (aggregate mode's disk artifact — always
/// on). Overwrites in place: the aggregate is session-cumulative, so the last
/// write of the day is the day's truth.
fn flush_summary(dir: &std::path::Path) {
    let rows = snapshot();
    if rows.is_empty() {
        return;
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Summary {
        date: String,
        flushed_at_ms: u64,
        events: Vec<EmitAggRow>,
    }
    let day = today();
    let s = Summary {
        date: day.clone(),
        flushed_at_ms: now_ms() as u64,
        events: rows,
    };
    if fs::create_dir_all(dir).is_ok() {
        prune(dir);
        let path = dir.join(format!("emit-summary-{day}.json"));
        match serde_json::to_vec_pretty(&s) {
            Ok(json) => {
                if let Err(e) = fs::write(&path, json) {
                    log::warn!("emit summary write failed: {e}");
                }
            }
            Err(e) => log::warn!("emit summary encode failed: {e}"),
        }
    }
}

// ---------------------------------------------------------------- commands

/// The dev panel's Emits table source (polled while the panel is open).
#[tauri::command(async)]
pub fn telemetry_emits() -> Vec<EmitAggRow> {
    snapshot()
}

/// Raw-trace indicator state (status bar + Emits tab poll this on open).
#[tauri::command(async)]
pub fn telemetry_trace_state() -> TraceState {
    trace_state()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_counter_counts_without_buffering() {
        let mut c = ByteCounter(0);
        serde_json::to_writer(&mut c, &serde_json::json!({"a": 1, "b": "xy"})).unwrap();
        assert_eq!(c.0, r#"{"a":1,"b":"xy"}"#.len() as u64);
    }

    #[test]
    fn record_aggregates_by_name_only() {
        record("t://emit-agg", Some("agent-1"), 100);
        record("t://emit-agg", Some("agent-2"), 300);
        let rows = snapshot();
        let row = rows.iter().find(|r| r.name == "t://emit-agg").unwrap();
        assert_eq!(row.count, 2, "keys never split the aggregate");
        assert_eq!(row.total_bytes, 400);
        assert_eq!(row.max_bytes, 300);
        assert!(row.calls10s >= 2);
    }

    #[test]
    fn percentiles_from_size_ring() {
        for i in 1..=100u64 {
            record("t://emit-pct", None, i);
        }
        let rows = snapshot();
        let row = rows.iter().find(|r| r.name == "t://emit-pct").unwrap();
        assert_eq!(row.p50_bytes, 50);
        assert_eq!(row.p95_bytes, 95);
    }

    #[test]
    fn today_is_iso_shaped() {
        let d = today();
        assert_eq!(d.len(), 10);
        assert_eq!(&d[4..5], "-");
        assert_eq!(&d[7..8], "-");
    }

    #[test]
    fn trace_state_inactive_by_default_reports_zeroes() {
        // TRACE_ON is process-global; only assert the inactive shape here.
        if !TRACE_ON.load(Ordering::Relaxed) {
            let s = trace_state();
            assert!(!s.active);
            assert_eq!(s.started_ms, 0);
            assert_eq!(s.bytes, 0);
        }
    }
}
