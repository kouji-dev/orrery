use super::{snapshot, CostSnapshot};

/// Initial value so the status bar can paint a cost without waiting for the 60s
/// push loop (whose first emit fires before the webview subscribes). The ccusage
/// shell-out takes seconds — blocking pool.
#[tauri::command]
pub async fn system_cost() -> Result<CostSnapshot, String> {
    tauri::async_runtime::spawn_blocking(|| crate::perf::timed("system_cost", snapshot))
        .await
        .map_err(|e| format!("join: {e}"))
}
