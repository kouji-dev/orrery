//! `libsrc_*` commands (M4). `id` in `libsrc_ensure` is an agent uuid OR a
//! project id (the project pseudo-agent record) → its OWNING PROJECT (an
//! agent's worktree shares the project's lockfile sources); `sourceId` is a
//! library source id from `libsrc_sources` / `libsrc://status`.

use tauri::State;
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::core::errors::AppResult;

use super::{LibSourceView, LibSrcService};

/// Every known library source with its index state.
#[tauri::command(async)]
pub fn libsrc_sources(libsrc: State<'_, LibSrcService>) -> AppResult<Vec<LibSourceView>> {
    crate::perf::timed("libsrc_sources", || Ok(libsrc.sources()))
}

/// Index what the project owning `id` needs (`Cargo.lock` → its crates,
/// `pom.xml` → its jars + the JDK). Returns the sources considered.
#[tauri::command(async)]
pub fn libsrc_ensure(
    libsrc: State<'_, LibSrcService>,
    agents: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<Vec<LibSourceView>> {
    crate::perf::timed("libsrc_ensure", || {
        let project_id = agents.get(id)?.project_id;
        Ok(libsrc.ensure_for_project(&project_id.to_string()))
    })
}

/// Dev console "Re-scan projects": discovery from scratch, every project's
/// sources ensured; returns the full list.
#[tauri::command(async)]
pub fn libsrc_rescan(libsrc: State<'_, LibSrcService>) -> AppResult<Vec<LibSourceView>> {
    crate::perf::timed("libsrc_rescan", || Ok(libsrc.rescan()))
}

#[tauri::command(async)]
pub fn libsrc_reindex(libsrc: State<'_, LibSrcService>, source_id: String) -> AppResult<LibSourceView> {
    crate::perf::timed("libsrc_reindex", || libsrc.reindex(&source_id))
}

#[tauri::command(async)]
pub fn libsrc_cancel(libsrc: State<'_, LibSrcService>, source_id: String) -> AppResult<()> {
    crate::perf::timed("libsrc_cancel", || {
        libsrc.cancel(&source_id);
        Ok(())
    })
}

#[tauri::command(async)]
pub fn libsrc_remove(libsrc: State<'_, LibSrcService>, source_id: String) -> AppResult<()> {
    crate::perf::timed("libsrc_remove", || libsrc.remove(&source_id))
}
