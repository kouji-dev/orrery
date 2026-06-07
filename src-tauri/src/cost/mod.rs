//! Total Claude usage cost, read from the `ccusage` CLI. We don't persist
//! anything — ccusage reads the on-disk transcripts fresh each run, so the total
//! survives app restarts and agent deletion. Global all-time total (all Claude
//! Code usage on the machine, not just katrix agents).

use serde::Serialize;

pub mod commands;

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
        Self { total_cost: 0.0, currency: "USD", available: false }
    }
    pub fn usd(total: f64) -> Self {
        Self { total_cost: total, currency: "USD", available: true }
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
    std::process::Command::new("cmd")
        .args(["/C", "npx", "ccusage", "daily", "--json"])
        .output()
        .ok()
}

#[cfg(not(windows))]
fn run_ccusage() -> Option<std::process::Output> {
    std::process::Command::new("npx").args(["ccusage", "daily", "--json"]).output().ok()
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
}
