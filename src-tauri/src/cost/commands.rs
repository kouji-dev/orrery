use super::{snapshot, CostSnapshot};

/// Optional synchronous initial value so the status bar can paint a cost before
/// the first 60s push. Runs ccusage inline (a few hundred ms) — fine for a prime.
#[tauri::command(async)]
pub fn system_cost() -> CostSnapshot {
    snapshot()
}
