use tauri::{AppHandle, Runtime, State};
use uuid::Uuid;

use crate::agents::adapters::HookEnv;
use crate::core::errors::{AppError, AppResult};
use crate::core::events::{emit_entity, Change};
use crate::hooks::{hook_binary, HookBridge};
use crate::projects::service::ProjectService;
use crate::runtime::RuntimeService;
use crate::watch::WatchService;

use super::model::{Agent, AgentSpawnRequest, AgentUpdateRequest};
use super::service::AgentService;

#[tauri::command]
pub fn agent_list(svc: State<'_, AgentService>) -> AppResult<Vec<Agent>> {
    svc.list()
}

#[tauri::command]
pub fn agent_spawn<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    req: AgentSpawnRequest,
) -> AppResult<Agent> {
    let project_path = projects.path_of(req.project_id)?;
    let agent = svc
        .spawn(req, std::path::Path::new(&project_path))
        .inspect_err(|e| log::error!("agent_spawn failed: {e:?}"))?;
    emit_entity(&app, "agent", Change::Created, agent.clone());
    Ok(agent)
}

#[tauri::command]
pub fn agent_update<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, AgentService>,
    id: Uuid,
    req: AgentUpdateRequest,
) -> AppResult<Agent> {
    let agent = svc.update(id, req)?;
    emit_entity(&app, "agent", Change::Updated, agent.clone());
    Ok(agent)
}

#[tauri::command]
pub fn agent_remove<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    watch: State<'_, WatchService>,
    id: Uuid,
) -> AppResult<()> {
    let project_path = svc.get(id).ok().and_then(|a| projects.path_of(a.project_id).ok());
    watch.unwatch(id); // stop watching its worktree before tearing it down
    svc.remove(id, project_path.as_deref().map(std::path::Path::new))?;
    emit_entity(&app, "agent", Change::Deleted, serde_json::json!({ "id": id }));
    Ok(())
}

/// Worktree file tree (source recursed, ignored dirs as lazy stubs).
#[tauri::command]
pub fn agent_tree(svc: State<'_, AgentService>, id: Uuid) -> AppResult<Vec<crate::fs::FileNode>> {
    let agent = svc.get(id)?;
    Ok(crate::fs::tree(std::path::Path::new(&agent.worktree)))
}

/// Working-tree changes in an agent's worktree (transient `git_changes`).
#[tauri::command]
pub fn agent_changes(
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<Vec<crate::git::service::FileChange>> {
    svc.changes(id)
}

/// Commits on the agent's branch — read from its worktree HEAD, tagged with the
/// agent id (transient `git_commits`).
#[tauri::command]
pub fn agent_commits(
    svc: State<'_, AgentService>,
    id: Uuid,
    limit: Option<usize>,
) -> AppResult<Vec<crate::projects::model::CommitView>> {
    svc.commits(id, limit.unwrap_or(50))
}

#[tauri::command]
pub fn agent_commit(
    svc: State<'_, AgentService>,
    id: Uuid,
    message: String,
    paths: Vec<String>,
) -> AppResult<String> {
    svc.commit(id, &message, &paths)
}

#[tauri::command]
pub fn agent_discard(svc: State<'_, AgentService>, id: Uuid, paths: Vec<String>) -> AppResult<()> {
    svc.discard(id, &paths)
}

#[tauri::command]
pub fn agent_merge(
    svc: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<()> {
    let agent = svc.get(id)?;
    let project_path = projects.path_of(agent.project_id)?;
    svc.merge(id, std::path::Path::new(&project_path))
}

#[tauri::command]
pub fn agent_diff(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
) -> AppResult<crate::git::service::FileDiff> {
    svc.file_diff(id, &path)
}

/// Launch the agent's tool in its worktree (PTY-streamed) and mark it running.
///
/// `resume` (default false) requests a "Continue session" launch: when the agent
/// has a captured `session_id`, the tool is relaunched into that CLI session
/// (e.g. `claude --resume <id>`) instead of a fresh/bare run. Falls back to the
/// normal launch when there's no session id (or the tool has no resume flow), so
/// the existing Start/Resume button (resume=false) is unchanged.
#[tauri::command]
pub fn agent_start<R: Runtime>(
    app: AppHandle<R>,
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    bridge: State<'_, HookBridge>,
    id: Uuid,
    rows: u16,
    cols: u16,
    resume: Option<bool>,
) -> AppResult<()> {
    let agent = svc.get(id)?;
    // resume-into-session only when asked AND a session id was captured
    let resume_session = if resume.unwrap_or(false) { agent.session_id.clone() } else { None };
    // deliver the initial task prompt only on the very first launch (never on a
    // resume-into-session, which continues an existing conversation)
    let send_prompt = !agent.started && resume_session.is_none();
    // Hooks are installed globally at startup; here we only stamp the KATRIX_*
    // env so this katrix-launched agent's hook brokers with the bridge. Gated on
    // the hook binary resolving (current_exe) — without it the hook can't run, so
    // we launch bare and rely on the PTY-parsing fallback.
    let hook_env = hook_binary().map(|_hook_bin| HookEnv {
        agent_id: id.to_string(),
        tool: agent.tool.clone(),
        endpoint: bridge.endpoint(),
        token: bridge.token().to_string(),
    });
    rt.start(app.clone(), &agent, rows, cols, send_prompt, hook_env.as_ref(), resume_session.as_deref())
        .map_err(AppError::Other)?;
    if send_prompt {
        svc.mark_started(id)?;
    }
    let updated = svc.update(
        id,
        AgentUpdateRequest { status: Some("running".into()), task: None, model: None, name: None },
    )?;
    emit_entity(&app, "agent", Change::Updated, updated);
    Ok(())
}

/// Stop the agent's running process and mark it idle.
#[tauri::command]
pub fn agent_stop<R: Runtime>(
    app: AppHandle<R>,
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<()> {
    rt.stop(id);
    let updated = svc.update(
        id,
        AgentUpdateRequest { status: Some("idle".into()), task: None, model: None, name: None },
    )?;
    emit_entity(&app, "agent", Change::Updated, updated);
    Ok(())
}

/// Forward UI-terminal keystrokes into the agent's PTY stdin.
#[tauri::command]
pub fn agent_input(rt: State<'_, RuntimeService>, id: Uuid, data: String) -> AppResult<()> {
    rt.write(id, &data).map_err(AppError::Other)
}

/// Approve the agent's pending permission prompt by typing the tool's
/// approve-keystrokes into its PTY. Resolves the agent's tool → adapter →
/// `allow_keys()` so the RIGHT keys go to each tool's prompt UI (best-effort
/// until hook decision-forwarding lands; see `AgentAdapter::allow_keys`).
#[tauri::command]
pub fn agent_allow(rt: State<'_, RuntimeService>, svc: State<'_, AgentService>, id: Uuid) -> AppResult<()> {
    let tool = svc.get(id)?.tool;
    let adapter = super::adapters::adapter_for(&tool)
        .ok_or_else(|| AppError::Other(format!("unknown tool: {tool}")))?;
    rt.write(id, adapter.allow_keys()).map_err(AppError::Other)
}

/// Deny the agent's pending permission prompt by typing the tool's
/// deny-keystrokes into its PTY (mirror of [`agent_allow`], using `deny_keys()`).
#[tauri::command]
pub fn agent_deny(rt: State<'_, RuntimeService>, svc: State<'_, AgentService>, id: Uuid) -> AppResult<()> {
    let tool = svc.get(id)?.tool;
    let adapter = super::adapters::adapter_for(&tool)
        .ok_or_else(|| AppError::Other(format!("unknown tool: {tool}")))?;
    rt.write(id, adapter.deny_keys()).map_err(AppError::Other)
}

/// Select option `choice` (1-based) in the agent's pending numbered SELECT prompt
/// (e.g. an AskUserQuestion-style ask) by typing the tool's decide-keystrokes
/// into its PTY. Resolves the agent's tool → adapter → `decide_keys(choice)`
/// (mirror of [`agent_allow`]). Best-effort, fire-and-forget — NOT a forwarded
/// decision; assumes a numbered select where 1..N maps to the displayed options
/// (see `AgentAdapter::decide_keys`).
#[tauri::command]
pub fn agent_decide(
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    id: Uuid,
    choice: u32,
) -> AppResult<()> {
    let tool = svc.get(id)?.tool;
    let adapter = super::adapters::adapter_for(&tool)
        .ok_or_else(|| AppError::Other(format!("unknown tool: {tool}")))?;
    rt.write(id, &adapter.decide_keys(choice)).map_err(AppError::Other)
}

/// Resize the agent's PTY to match the visible terminal (cols × rows).
#[tauri::command]
pub fn agent_resize(
    rt: State<'_, RuntimeService>,
    id: Uuid,
    rows: u16,
    cols: u16,
) -> AppResult<()> {
    rt.resize(id, rows, cols).map_err(AppError::Other)
}

/// Start watching an agent's worktree (replaces any previous watch); emits `agent://changed`.
#[tauri::command]
pub fn agent_watch<R: Runtime>(
    app: AppHandle<R>,
    watch: State<'_, WatchService>,
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<()> {
    let worktree = svc.get(id)?.worktree;
    watch.watch(app, id, std::path::PathBuf::from(worktree));
    Ok(())
}

/// One directory's immediate children — used to lazily expand an unloaded folder.
#[tauri::command]
pub fn agent_dir(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
) -> AppResult<Vec<crate::fs::FileNode>> {
    let agent = svc.get(id)?;
    Ok(crate::fs::list_dir(std::path::Path::new(&agent.worktree), &path))
}

/// Detection of which CLI coding agents are installed — delegated to the adapter
/// registry so only-installed tools are offered (and, later, hooked).
#[tauri::command]
pub fn detect_tools() -> Vec<super::adapters::ToolStatus> {
    super::adapters::installed()
}
