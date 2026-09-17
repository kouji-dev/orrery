use tauri::State;

use super::{CostCache, CostSnapshot, COST_FRESH};

/// Initial value so the status bar can paint a cost without waiting for the
/// next push. PEEKS the loop's cache — never runs ccusage itself (A1.8): at
/// startup the push loop's first run is already in flight and will emit
/// `system://cost` the moment it lands, so blocking here behind that run only
/// parked a thread for the full multi-second ccusage scan to duplicate the
/// loop's emit. `None` on a cold/stale cache — the store keeps the readout
/// hidden and the loop's push fills it in.
#[tauri::command]
pub async fn system_cost(cache: State<'_, CostCache>) -> Result<Option<CostSnapshot>, String> {
    Ok(cache.peek(COST_FRESH))
}
