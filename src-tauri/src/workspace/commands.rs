use tauri::State;

use crate::core::errors::AppResult;

use super::service::WorkspaceService;

#[tauri::command(async)]
pub fn workspace_get(svc: State<'_, WorkspaceService>) -> AppResult<Option<serde_json::Value>> {
    crate::perf::timed("workspace_get", || svc.get())
}

#[tauri::command(async)]
pub fn workspace_set(svc: State<'_, WorkspaceService>, doc: serde_json::Value) -> AppResult<()> {
    crate::perf::timed("workspace_set", || svc.set(&doc))
}
