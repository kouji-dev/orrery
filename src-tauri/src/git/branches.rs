//! A3.2 — branch & remote commands for the Branches panel.
//!
//! The operations live behind [`GitBackend`](super::backend::GitBackend):
//! native for list/create/rename/delete/upstream/checkout, and a `git` CLI
//! shell-out for the auth-touching ops (fetch, pull) so the OS credential
//! helper handles auth. The branch→checkout occupancy pre-checks (git refuses
//! to check out a branch another worktree holds; the libraries do not enforce
//! that for rename/delete) are the backend's responsibility.

use std::path::Path;

use tauri::State;
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::core::errors::{AppError, AppResult};
use crate::projects::service::ProjectService;

pub use super::types::{BranchInfo, RemoteInfo};

#[tauri::command]
pub async fn project_branches_detail(
    projects: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<Vec<BranchInfo>> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_branches_detail", || {
            projects
                .git()
                .branches_detail(Path::new(&projects.path_of(id)?))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_remotes(
    projects: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<Vec<RemoteInfo>> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_remotes", || {
            projects.git().remotes(Path::new(&projects.path_of(id)?))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_fetch(
    projects: State<'_, ProjectService>,
    id: Uuid,
    remote: Option<String>,
) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_fetch", || {
            projects
                .git()
                .fetch(Path::new(&projects.path_of(id)?), remote.as_deref())
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_pull(projects: State<'_, ProjectService>, id: Uuid) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_pull", || {
            projects.git().pull_ff(Path::new(&projects.path_of(id)?))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn agent_pull(agents: State<'_, AgentService>, id: Uuid) -> AppResult<()> {
    let agents = agents.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_pull", || {
            agents
                .git()
                .pull_ff(Path::new(&agents.get(id)?.worktree))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_branch_create(
    projects: State<'_, ProjectService>,
    id: Uuid,
    name: String,
    from: Option<String>,
) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_branch_create", || {
            projects
                .git()
                .branch_create(Path::new(&projects.path_of(id)?), &name, from.as_deref())
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_branch_rename(
    projects: State<'_, ProjectService>,
    id: Uuid,
    old: String,
    new: String,
) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_branch_rename", || {
            projects
                .git()
                .branch_rename(Path::new(&projects.path_of(id)?), &old, &new)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_branch_delete(
    projects: State<'_, ProjectService>,
    id: Uuid,
    name: String,
    force: Option<bool>,
) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_branch_delete", || {
            projects.git().branch_delete(
                Path::new(&projects.path_of(id)?),
                &name,
                force.unwrap_or(false),
            )
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_branch_upstream(
    projects: State<'_, ProjectService>,
    id: Uuid,
    name: String,
    upstream: Option<String>,
) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_branch_upstream", || {
            projects.git().branch_set_upstream(
                Path::new(&projects.path_of(id)?),
                &name,
                upstream.as_deref(),
            )
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn agent_checkout(
    agents: State<'_, AgentService>,
    id: Uuid,
    branch: String,
) -> AppResult<()> {
    let agents = agents.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_checkout", || {
            agents
                .git()
                .checkout_branch(Path::new(&agents.get(id)?.worktree), &branch)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}
