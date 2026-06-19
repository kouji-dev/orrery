//! Small app-level commands that don't belong to a domain service.

use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

/// Absolute path to the rolling diagnostics log file. The logger plugin
/// (`core::logger`) writes to `app_log_dir/orrery.log` via its `LogDir` target
/// with `file_name = "orrery"`, so this must stay in sync with that file name.
fn log_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("no log dir: {e}"))?;
    Ok(dir.join("orrery.log"))
}

/// Open the app's rolling diagnostics log file in the OS default handler.
/// The file is created by the logger plugin on the first log write (which
/// happens during startup), so it exists by the time the UI can call this.
#[tauri::command]
pub fn open_log(app: tauri::AppHandle) -> Result<(), String> {
    let path = log_path(&app)?;
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| format!("open log failed: {e}"))
}
