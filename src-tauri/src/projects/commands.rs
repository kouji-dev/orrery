use tauri::{AppHandle, Runtime, State};
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::core::errors::AppResult;
use crate::core::events::{emit_entity, Change};

use super::model::{CommitView, Project, ProjectCreateRequest, ProjectUpdateRequest};
use super::service::ProjectService;

#[tauri::command(async)]
pub fn project_list(svc: State<'_, ProjectService>) -> AppResult<Vec<Project>> {
    svc.list()
}

#[tauri::command(async)]
pub fn project_create<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, ProjectService>,
    req: ProjectCreateRequest,
) -> AppResult<Project> {
    let project = svc.create(req).inspect_err(|e| log::error!("project_create failed: {e:?}"))?;
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
    let project = svc.update(id, req)?;
    emit_entity(&app, "project", Change::Updated, project.clone());
    Ok(project)
}

#[tauri::command(async)]
pub fn project_init_git<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<Project> {
    let project = svc.init_git(id)?;
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
    svc.remove(id)?;
    // cascade: drop the project's agents and announce each removal
    if let Ok(removed) = agents.remove_for_project(id) {
        for aid in removed {
            emit_entity(&app, "agent", Change::Deleted, serde_json::json!({ "id": aid }));
        }
    }
    emit_entity(&app, "project", Change::Deleted, serde_json::json!({ "id": id }));
    Ok(())
}

#[tauri::command(async)]
pub fn project_detect_git(svc: State<'_, ProjectService>, path: String) -> AppResult<bool> {
    Ok(svc.detect_git(&path))
}

#[tauri::command(async)]
pub fn project_commits(
    svc: State<'_, ProjectService>,
    id: Uuid,
    limit: Option<usize>,
) -> AppResult<Vec<CommitView>> {
    svc.commits(id, limit.unwrap_or(50))
}
