//! Tauri commands for Local History (B4.4). Small file IO — sync bodies on
//! the async command pool.

use std::path::Path;

use tauri::State;
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::core::errors::{AppError, AppResult};
use crate::git::service::{lang_from_path, FileDiff};

use super::{HistoryService, Snapshot};

/// Newest-first snapshot timeline for one agent.
#[tauri::command(async)]
pub fn history_list(history: State<'_, HistoryService>, id: Uuid) -> AppResult<Vec<Snapshot>> {
    crate::perf::timed("history_list", || Ok(history.list(id)))
}

/// Snapshot content vs CURRENT worktree content of one file — the `FileDiff`
/// shape so the existing diff surface renders it unmodified (old = snapshot).
#[tauri::command(async)]
pub fn history_file(
    history: State<'_, HistoryService>,
    agents: State<'_, AgentService>,
    id: Uuid,
    snap: String,
    path: String,
) -> AppResult<FileDiff> {
    crate::perf::timed("history_file", || {
        let old = history.file_at(id, &snap, &path)?;
        let worktree = agents.get(id)?.worktree;
        let new = std::fs::read_to_string(Path::new(&worktree).join(&path)).unwrap_or_default();
        Ok(FileDiff {
            old,
            new,
            lang: lang_from_path(&path).to_string(),
        })
    })
}

/// Restore files to their snapshot content (guard-snapshotting current state
/// first). `paths` None = every file of the snapshot. Watcher pushes follow.
#[tauri::command(async)]
pub fn history_restore(
    history: State<'_, HistoryService>,
    agents: State<'_, AgentService>,
    id: Uuid,
    snap: String,
    paths: Option<Vec<String>>,
) -> AppResult<Vec<String>> {
    crate::perf::timed("history_restore", || {
        let worktree = agents.get(id)?.worktree;
        if worktree.is_empty() {
            return Err(AppError::Other("agent has no worktree".into()));
        }
        history.restore(id, Path::new(&worktree), &snap, paths)
    })
}
