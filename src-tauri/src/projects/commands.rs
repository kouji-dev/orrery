use tauri::{AppHandle, Runtime, State};
use uuid::Uuid;

use crate::core::errors::AppResult;
use crate::core::events::{emit_entity, Change};

use super::model::{Project, ProjectCreateRequest};
use super::service::ProjectService;

#[tauri::command]
pub fn project_list(svc: State<'_, ProjectService>) -> AppResult<Vec<Project>> {
    svc.list()
}

#[tauri::command]
pub fn project_create<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, ProjectService>,
    req: ProjectCreateRequest,
) -> AppResult<Project> {
    let project = svc.create(req)?;
    emit_entity(&app, "project", Change::Created, project.clone());
    Ok(project)
}

#[tauri::command]
pub fn project_remove<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<()> {
    svc.remove(id)?;
    emit_entity(&app, "project", Change::Deleted, serde_json::json!({ "id": id }));
    Ok(())
}

#[tauri::command]
pub fn project_detect_git(svc: State<'_, ProjectService>, path: String) -> AppResult<bool> {
    Ok(svc.detect_git(&path))
}
