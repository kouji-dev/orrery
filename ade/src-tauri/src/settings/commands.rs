use tauri::State;

use crate::core::errors::AppResult;

use super::model::Settings;
use super::service::SettingsService;

#[tauri::command(async)]
pub fn settings_get(svc: State<'_, SettingsService>) -> AppResult<Settings> {
    crate::perf::timed("settings_get", || svc.get())
}

/// Persist the whole document and echo it back (the store applies optimistically;
/// the echo confirms what actually landed).
#[tauri::command(async)]
pub fn settings_set(svc: State<'_, SettingsService>, settings: Settings) -> AppResult<Settings> {
    crate::perf::timed("settings_set", || {
        svc.set(&settings)?;
        // Apply the raw-trace toggle immediately (idempotent) — the emit
        // funnel owns the runtime state; the DB row only survives restarts.
        crate::core::emit::set_raw_trace(settings.telemetry_raw_trace);
        Ok(settings)
    })
}
