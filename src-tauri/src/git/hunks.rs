//! B4.3 — per-file hunks vs HEAD + reverse-apply of one hunk (the editor's
//! gutter change markers and their click-to-revert). The A3.7 hunk-machinery
//! subset: backend-authoritative hunks so markers and revert share coordinates.
//!
//! The git work itself lives behind [`GitBackend`](super::backend::GitBackend)
//! (`file_hunks` / `revert_hunk`); this module is the Tauri command surface.

use std::path::Path;

use tauri::State;
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::core::errors::{AppError, AppResult};

pub use super::types::Hunk;

#[tauri::command]
pub async fn agent_file_hunks(
    agents: State<'_, AgentService>,
    id: Uuid,
    path: String,
) -> AppResult<Vec<Hunk>> {
    let agents = agents.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_file_hunks", || {
            agents
                .git()
                .file_hunks(Path::new(&agents.get(id)?.worktree), &path)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn agent_hunk_revert(
    agents: State<'_, AgentService>,
    id: Uuid,
    path: String,
    new_start: u32,
) -> AppResult<()> {
    let agents = agents.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_hunk_revert", || {
            agents
                .git()
                .revert_hunk(Path::new(&agents.get(id)?.worktree), &path, new_start)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}
