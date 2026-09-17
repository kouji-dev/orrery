//! `lsp_*` + `nav_virtual_read` commands. `id` is an agent uuid OR a project
//! id (the project pseudo-agent), exactly like the `nav_*` commands: it
//! resolves to the worktree for document uris and to the owning project for
//! the server key. Every mutation is followed by an `lsp://status` snapshot.

use std::path::{Path, PathBuf};

use tauri::State;
use uuid::Uuid;

use crate::agents::fs_commands::safe_join;
use crate::agents::service::AgentService;
use crate::core::errors::{AppError, AppResult};
use crate::libsrc::LibSrcService;
use crate::projects::service::ProjectService;
use crate::symbols::norm_rel;

use super::server::ProjectRef;
use super::virtual_docs::{self, Dispatch, VirtualDoc};
use super::{language_of, Lang, LspService, LspStatus};

/// `(worktree root, owning project)` of an agent / project id.
pub(crate) fn resolve_project(
    agents: &AgentService,
    projects: &ProjectService,
    id: Uuid,
) -> AppResult<(PathBuf, ProjectRef)> {
    let agent = agents.get(id)?;
    let project_id = agent.project_id;
    let (root, name) = if agent.id == project_id {
        (PathBuf::from(&agent.worktree), agent.name.clone())
    } else {
        (
            PathBuf::from(projects.path_of(project_id)?),
            projects.name_of(project_id).unwrap_or_default(),
        )
    };
    Ok((
        PathBuf::from(agent.worktree),
        ProjectRef {
            id: project_id,
            name,
            root,
        },
    ))
}

/// `(absolute path, language)` of a worktree-relative document.
fn resolve_doc(worktree: &Path, path: &str) -> AppResult<(PathBuf, Option<Lang>)> {
    let rel = norm_rel(path);
    let abs = safe_join(worktree, &rel)?;
    Ok((abs, language_of(&rel)))
}

#[tauri::command(async)]
pub fn lsp_status(lsp: State<'_, LspService>) -> AppResult<LspStatus> {
    crate::perf::timed("lsp_status", || Ok(lsp.status_snapshot()))
}

/// Start `ext_id` for the project owning `id` (forces past a manual stop /
/// exhausted backoff).
#[tauri::command(async)]
pub fn lsp_start(
    lsp: State<'_, LspService>,
    agents: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    ext_id: String,
    id: Uuid,
) -> AppResult<()> {
    crate::perf::timed("lsp_start", || {
        let (_, project) = resolve_project(&agents, &projects, id)?;
        lsp.start(&ext_id, project)
    })
}

#[tauri::command(async)]
pub fn lsp_stop(lsp: State<'_, LspService>, ext_id: String, project_id: Uuid) -> AppResult<()> {
    crate::perf::timed("lsp_stop", || lsp.stop(&ext_id, project_id))
}

#[tauri::command(async)]
pub fn lsp_restart(lsp: State<'_, LspService>, ext_id: String, project_id: Uuid) -> AppResult<()> {
    crate::perf::timed("lsp_restart", || lsp.restart(&ext_id, project_id))
}

/// An agent started in the project owning `id` (`pinned`) / the last one
/// there stopped (`!pinned`): auto-start the servers of the languages the
/// project uses and hold them, or release them to the idle reaper. Answers
/// the pack ids acquired.
#[tauri::command(async)]
pub fn lsp_pin_project(
    lsp: State<'_, LspService>,
    agents: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    id: Uuid,
    pinned: bool,
) -> AppResult<Vec<String>> {
    crate::perf::timed("lsp_pin_project", || {
        let (_, project) = resolve_project(&agents, &projects, id)?;
        if pinned {
            Ok(lsp.pin_project(project))
        } else {
            lsp.unpin_project(project.id);
            Ok(Vec::new())
        }
    })
}

#[tauri::command(async)]
pub fn lsp_stop_all(lsp: State<'_, LspService>) -> AppResult<()> {
    crate::perf::timed("lsp_stop_all", || {
        lsp.stop_all();
        lsp.emit_status();
        Ok(())
    })
}

// ---- document sync (cheap no-ops without a running server) ----

/// `languageId` is Monaco's and informational — the LSP id comes from the
/// file extension table, which is what the server pack was matched on.
#[tauri::command(async)]
pub fn lsp_doc_open(
    lsp: State<'_, LspService>,
    agents: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    id: Uuid,
    path: String,
    text: String,
    language_id: Option<String>,
) -> AppResult<()> {
    let _ = language_id;
    crate::perf::timed("lsp_doc_open", || {
        let (worktree, project) = resolve_project(&agents, &projects, id)?;
        let (abs, lang) = resolve_doc(&worktree, &path)?;
        if let Some(lang) = lang {
            lsp.doc_open(&lang, project, &abs, text, 1);
        }
        Ok(())
    })
}

#[tauri::command(async)]
pub fn lsp_doc_change(
    lsp: State<'_, LspService>,
    agents: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    id: Uuid,
    path: String,
    text: String,
    version: i64,
) -> AppResult<()> {
    crate::perf::timed("lsp_doc_change", || {
        let (worktree, project) = resolve_project(&agents, &projects, id)?;
        let (abs, lang) = resolve_doc(&worktree, &path)?;
        if let Some(lang) = lang {
            lsp.doc_change(&lang, project.id, &abs, text, version);
        }
        Ok(())
    })
}

#[tauri::command(async)]
pub fn lsp_doc_close(
    lsp: State<'_, LspService>,
    agents: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    id: Uuid,
    path: String,
) -> AppResult<()> {
    crate::perf::timed("lsp_doc_close", || {
        let (worktree, project) = resolve_project(&agents, &projects, id)?;
        let (abs, lang) = resolve_doc(&worktree, &path)?;
        if let Some(lang) = lang {
            lsp.doc_close(&lang, project.id, &abs);
        }
        Ok(())
    })
}

/// Text of a virtual document: `orrery-lib://…` from the library index (M4),
/// every other scheme (`jdt://…`) via the owning language server.
#[tauri::command(async)]
pub fn nav_virtual_read(
    lsp: State<'_, LspService>,
    libsrc: State<'_, LibSrcService>,
    uri: String,
) -> AppResult<VirtualDoc> {
    crate::perf::timed("nav_virtual_read", || {
        if uri.trim().is_empty() {
            return Err(AppError::Other("empty uri".into()));
        }
        if matches!(virtual_docs::dispatch(&uri), Ok(Dispatch::Library)) {
            return libsrc.read(&uri);
        }
        lsp.virtual_read(&uri)
    })
}
