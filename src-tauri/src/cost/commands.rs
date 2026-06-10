use tauri::State;

use super::{CostCache, CostSnapshot, COST_FRESH};

/// Initial value so the status bar can paint a cost without waiting for the
/// push loop (whose first emit fires before the webview subscribes). Shares the
/// loop's cache — at startup the loop's run is in-flight, so this waits for its
/// result instead of spawning a second multi-second ccusage/node process.
#[tauri::command]
pub async fn system_cost(cache: State<'_, CostCache>) -> Result<CostSnapshot, String> {
    let cache = cache.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("system_cost", move || cache.get(COST_FRESH))
    })
    .await
    .map_err(|e| format!("join: {e}"))
}
