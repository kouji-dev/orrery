use tauri::{AppHandle, Runtime, State};
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::core::errors::AppResult;
use crate::core::events::{emit_entity, Change};

use super::model::{CommitView, Project, ProjectCreateRequest, ProjectUpdateRequest};
use super::service::ProjectService;

#[tauri::command(async)]
pub fn project_list(svc: State<'_, ProjectService>) -> AppResult<Vec<Project>> {
    crate::perf::timed("project_list", || svc.list())
}

/// Blocking pool: may clone a remote (network) / git-init / inspect the repo on disk.
#[tauri::command]
pub async fn project_create<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, ProjectService>,
    req: ProjectCreateRequest,
) -> AppResult<Project> {
    let svc = svc.inner().clone();
    let project = tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_create", || {
            svc.create(req)
                .inspect_err(|e| log::error!("project_create failed: {e:?}"))
        })
    })
    .await
    .map_err(|e| crate::core::errors::AppError::Other(format!("join: {e}")))??;
    emit_entity(&app, "project", Change::Created, project.clone());
    Ok(project)
}

#[tauri::command(async)]
pub fn project_update<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, ProjectService>,
    id: Uuid,
    req: ProjectUpdateRequest,
) -> AppResult<Project> {
    crate::perf::timed("project_update", || {
        let project = svc.update(id, req)?;
        emit_entity(&app, "project", Change::Updated, project.clone());
        Ok(project)
    })
}

/// Blocking pool: git init on disk.
#[tauri::command]
pub async fn project_init_git<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<Project> {
    let svc = svc.inner().clone();
    let project = tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_init_git", || svc.init_git(id))
    })
    .await
    .map_err(|e| crate::core::errors::AppError::Other(format!("join: {e}")))??;
    emit_entity(&app, "project", Change::Updated, project.clone());
    Ok(project)
}

#[tauri::command(async)]
pub fn project_remove<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, ProjectService>,
    agents: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<()> {
    crate::perf::timed("project_remove", || {
        svc.remove(id)?;
        // cascade: drop the project's agents and announce each removal
        if let Ok(removed) = agents.remove_for_project(id) {
            for aid in removed {
                emit_entity(
                    &app,
                    "agent",
                    Change::Deleted,
                    serde_json::json!({ "id": aid }),
                );
            }
        }
        emit_entity(
            &app,
            "project",
            Change::Deleted,
            serde_json::json!({ "id": id }),
        );
        Ok(())
    })
}

/// Blocking pool: opens the repo on disk to probe it.
#[tauri::command]
pub async fn project_detect_git(svc: State<'_, ProjectService>, path: String) -> AppResult<bool> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_detect_git", || Ok(svc.detect_git(&path)))
    })
    .await
    .map_err(|e| crate::core::errors::AppError::Other(format!("join: {e}")))?
}

/// Repo-root file tree (source recursed, ignored dirs as lazy stubs) — the
/// project-scoped twin of `agent_tree`, rooted at the main worktree.
/// Blocking pool: walks up to 10k fs entries.
#[tauri::command]
pub async fn project_tree(
    svc: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<Vec<crate::fs::FileNode>> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_tree", || {
            let path = svc.path_of(id)?;
            Ok(crate::fs::tree(std::path::Path::new(&path)))
        })
    })
    .await
    .map_err(|e| crate::core::errors::AppError::Other(format!("join: {e}")))?
}

/// One directory's immediate children under the repo root — the project-scoped
/// twin of `agent_dir`, used to lazily expand an unloaded folder.
#[tauri::command(async)]
pub fn project_dir(
    svc: State<'_, ProjectService>,
    id: Uuid,
    path: String,
) -> AppResult<Vec<crate::fs::FileNode>> {
    crate::perf::timed("project_dir", || {
        let root = svc.path_of(id)?;
        Ok(crate::fs::list_dir(std::path::Path::new(&root), &path))
    })
}

/// Blocking pool: revwalk + per-commit tree diff.
#[tauri::command]
pub async fn project_commits(
    svc: State<'_, ProjectService>,
    id: Uuid,
    limit: Option<usize>,
) -> AppResult<Vec<CommitView>> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_commits", || svc.commits(id, limit.unwrap_or(50)))
    })
    .await
    .map_err(|e| crate::core::errors::AppError::Other(format!("join: {e}")))?
}
