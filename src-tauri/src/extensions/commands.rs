//! `ext_*` commands. Every mutation is followed by an `ext://status` snapshot
//! so the UI has one source of truth; `ext_install` returns as soon as the
//! job thread is running (progress + result arrive as events).

use tauri::{AppHandle, State};

use crate::core::errors::AppResult;

use super::{ExtRegistryView, ExtensionService};

/// Catalog + local state. `refresh` forces a registry fetch even when the
/// 6h cache is fresh.
#[tauri::command(async)]
pub fn ext_registry_list(
    svc: State<'_, ExtensionService>,
    refresh: Option<bool>,
) -> AppResult<ExtRegistryView> {
    crate::perf::timed("ext_registry_list", || Ok(svc.view(refresh.unwrap_or(false))))
}

/// Spawn the install job for `id`; `ext://progress` then `ext://status`.
#[tauri::command(async)]
pub fn ext_install(
    app: AppHandle,
    svc: State<'_, ExtensionService>,
    id: String,
) -> AppResult<()> {
    crate::perf::timed("ext_install", || svc.install(app, id))
}

#[tauri::command(async)]
pub fn ext_uninstall(
    app: AppHandle,
    svc: State<'_, ExtensionService>,
    id: String,
) -> AppResult<()> {
    crate::perf::timed("ext_uninstall", || {
        let r = svc.uninstall(&id);
        svc.emit_status(&app);
        r
    })
}

#[tauri::command(async)]
pub fn ext_set_enabled(
    app: AppHandle,
    svc: State<'_, ExtensionService>,
    id: String,
    enabled: bool,
) -> AppResult<()> {
    crate::perf::timed("ext_set_enabled", || {
        let r = svc.set_enabled(&id, enabled);
        svc.emit_status(&app);
        r
    })
}

/// Manual language-server path (`settings.lspPaths[id]`); `null` clears it.
#[tauri::command(async)]
pub fn ext_set_path(
    app: AppHandle,
    svc: State<'_, ExtensionService>,
    id: String,
    path: Option<String>,
) -> AppResult<()> {
    crate::perf::timed("ext_set_path", || {
        let r = svc.set_path(&id, path);
        svc.emit_status(&app);
        r
    })
}
