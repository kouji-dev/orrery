//! Tauri commands for find-in-files (B3.1) + the Search-Everywhere file corpus.
//!
//! `search_start` resolves the scope to concrete roots, validates the pattern,
//! then hands off to a worker thread that streams `search://results` batches
//! and one final `search://done` — the command itself returns the search id
//! immediately so the UI stays responsive and can `search_cancel` mid-flight.

use std::path::PathBuf;

use serde::Deserialize;
use tauri::{AppHandle, Manager, Runtime, State};
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::core::errors::{AppError, AppResult};
use crate::projects::service::ProjectService;

use super::{SearchRoot, SearchService};

/// Scope of a search: one agent worktree, the project's main checkout, or the
/// project checkout plus every agent worktree of that project.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub regex: bool,
    /// "worktree" | "project" | "all"
    pub scope: String,
    pub agent_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub max_results: Option<usize>,
}

fn resolve_roots(
    req: &SearchRequest,
    agents: &AgentService,
    projects: &ProjectService,
) -> AppResult<Vec<SearchRoot>> {
    let project_id = match (req.project_id, req.agent_id) {
        (Some(p), _) => Some(p),
        (None, Some(a)) => Some(agents.get(a)?.project_id),
        _ => None,
    };
    match req.scope.as_str() {
        "worktree" => {
            let id = req
                .agent_id
                .ok_or_else(|| AppError::Other("worktree scope needs agentId".into()))?;
            let agent = agents.get(id)?;
            Ok(vec![SearchRoot {
                path: PathBuf::from(agent.worktree),
                agent_id: Some(agent.id),
                label: Some(agent.name),
            }])
        }
        "project" => {
            let pid =
                project_id.ok_or_else(|| AppError::Other("project scope needs projectId".into()))?;
            Ok(vec![SearchRoot {
                path: PathBuf::from(projects.path_of(pid)?),
                agent_id: None,
                label: None,
            }])
        }
        "all" => {
            let pid = project_id
                .ok_or_else(|| AppError::Other("all-worktrees scope needs projectId".into()))?;
            let project_path = projects.path_of(pid)?;
            let mut roots = vec![SearchRoot {
                path: PathBuf::from(&project_path),
                agent_id: None,
                label: None,
            }];
            for a in agents.list()?.into_iter().filter(|a| a.project_id == pid) {
                if a.worktree.is_empty() || a.worktree == project_path {
                    continue;
                }
                roots.push(SearchRoot {
                    path: PathBuf::from(&a.worktree),
                    agent_id: Some(a.id),
                    label: Some(a.name),
                });
            }
            Ok(roots)
        }
        other => Err(AppError::Other(format!("unknown search scope: {other}"))),
    }
}

/// Kick off a streaming search; resolves to the search id. Results arrive as
/// `search://results` events (batched) and the run ends with `search://done`.
#[tauri::command(async)]
pub fn search_start<R: Runtime>(
    app: AppHandle<R>,
    search: State<'_, SearchService>,
    agents: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    req: SearchRequest,
) -> AppResult<Uuid> {
    crate::perf::timed("search_start", || {
        if req.query.is_empty() {
            return Err(AppError::Other("empty query".into()));
        }
        // Fail fast on a bad regex so the panel shows the error inline.
        let matcher = super::build_matcher(
            &req.query,
            req.case_sensitive,
            req.whole_word,
            req.regex,
        )
        .map_err(AppError::Other)?;
        let roots = resolve_roots(&req, &agents, &projects)?;
        let max = req.max_results.unwrap_or(super::DEFAULT_MAX_RESULTS);
        let (id, cancel) = search.register();
        // Registered-state handle for the cleanup at thread end. SearchService
        // is behind Tauri managed state, so grab what we need by value.
        let app2 = app.clone();
        std::thread::Builder::new()
            .name(format!("search-{id}"))
            .spawn(move || {
                crate::perf::timed("search_run", || {
                    super::run(app2.clone(), id, cancel, roots, matcher, max)
                });
                if let Some(svc) = app2.try_state::<SearchService>() {
                    svc.finish(id);
                }
            })
            .map_err(|e| AppError::Other(format!("spawn search thread: {e}")))?;
        Ok(id)
    })
}

/// Cancel a running search. Safe to call after it finished (no-op).
#[tauri::command(async)]
pub fn search_cancel(search: State<'_, SearchService>, id: Uuid) -> AppResult<()> {
    crate::perf::timed("search_cancel", || {
        search.cancel(id);
        Ok(())
    })
}

/// All (gitignore-filtered) file paths in an agent's worktree — the Files
/// corpus for Search Everywhere / Go to File. Capped at 20k entries.
/// Blocking pool: walks the whole tree.
#[tauri::command]
pub async fn search_files(
    agents: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<Vec<String>> {
    let agents = agents.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("search_files", || {
            let agent = agents.get(id)?;
            Ok(super::list_files(
                std::path::Path::new(&agent.worktree),
                20_000,
            ))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}
