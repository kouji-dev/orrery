use tauri::{AppHandle, Runtime, State};
use uuid::Uuid;

use crate::agents::adapters::HookEnv;
use crate::core::errors::{AppError, AppResult};
use crate::core::events::{emit_entity, Change};
use crate::hooks::{hook_binary, HookBridge};
use crate::projects::service::ProjectService;
use crate::runtime::RuntimeService;
use crate::settings::SettingsService;
use crate::tickets::service::TicketService;
use crate::watch::WatchService;

use super::model::{Agent, AgentSpawnRequest, AgentUpdateRequest};
use super::service::{AgentService, InterruptedAgents};

#[tauri::command(async)]
pub fn agent_list(svc: State<'_, AgentService>) -> AppResult<Vec<Agent>> {
    crate::perf::timed("agent_list", || svc.list())
}

/// Blocking pool: creates the git worktree + branch.
#[tauri::command]
pub async fn agent_spawn<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    tickets: State<'_, TicketService>,
    req: AgentSpawnRequest,
) -> AppResult<Agent> {
    let (svc, projects, tickets) = (
        svc.inner().clone(),
        projects.inner().clone(),
        tickets.inner().clone(),
    );
    let agent = tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_spawn", || {
            let project_path = projects.path_of(req.project_id)?;
            svc.spawn(req, std::path::Path::new(&project_path))
                .inspect_err(|e| log::error!("agent_spawn failed: {e:?}"))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))??;
    emit_entity(&app, "agent", Change::Created, agent.clone());
    // If a ticket was requested, attach this agent to it (best-effort: a missing
    // or already-assigned ticket must not fail the spawn).
    if let Some(ticket_id) = agent.ticket_id {
        match tickets.attach_agent(ticket_id, agent.id) {
            Ok(updated_ticket) => {
                emit_entity(&app, "ticket", Change::Updated, updated_ticket);
            }
            Err(e) => {
                log::warn!("agent_spawn: attach_agent({ticket_id}) failed: {e:?}");
            }
        }
    }
    Ok(agent)
}

#[tauri::command(async)]
pub fn agent_update<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, AgentService>,
    id: Uuid,
    req: AgentUpdateRequest,
) -> AppResult<Agent> {
    crate::perf::timed("agent_update", || {
        let agent = svc.update(id, req)?;
        emit_entity(&app, "agent", Change::Updated, agent.clone());
        Ok(agent)
    })
}

/// Blocking pool: removes the worktree directory + prunes git metadata.
#[tauri::command]
pub async fn agent_remove<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    watch: State<'_, WatchService>,
    id: Uuid,
) -> AppResult<()> {
    let project_path = svc
        .get(id)
        .ok()
        .and_then(|a| projects.path_of(a.project_id).ok());
    watch.unwatch(id); // stop watching its worktree before tearing it down
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_remove", || {
            svc.remove(id, project_path.as_deref().map(std::path::Path::new))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))??;
    emit_entity(
        &app,
        "agent",
        Change::Deleted,
        serde_json::json!({ "id": id }),
    );
    Ok(())
}

/// Worktree file tree (source recursed, ignored dirs as lazy stubs).
/// Blocking pool: walks up to 10k fs entries.
#[tauri::command]
pub async fn agent_tree(
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<Vec<crate::fs::FileNode>> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_tree", || {
            let agent = svc.get(id)?;
            Ok(crate::fs::tree(std::path::Path::new(&agent.worktree)))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Working-tree changes in an agent's worktree (transient `git_changes`).
/// Blocking pool: full git2 status with line counts.
#[tauri::command]
pub async fn agent_changes(
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<Vec<crate::git::service::FileChange>> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_changes", || svc.changes(id))
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Commits on the agent's branch — read from its worktree HEAD, tagged with the
/// agent id (transient `git_commits`). Blocking pool: revwalk + per-commit tree diff.
#[tauri::command]
pub async fn agent_commits(
    svc: State<'_, AgentService>,
    id: Uuid,
    limit: Option<usize>,
    offset: Option<usize>,
) -> AppResult<Vec<crate::projects::model::CommitView>> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_commits", || {
            svc.commits(id, limit.unwrap_or(50), offset.unwrap_or(0))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Blocking pool: git2 index add + commit.
#[tauri::command]
pub async fn agent_commit(
    svc: State<'_, AgentService>,
    id: Uuid,
    message: String,
    paths: Vec<String>,
) -> AppResult<String> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_commit", || svc.commit(id, &message, &paths))
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Blocking pool: git2 checkout-head over the selected paths.
#[tauri::command]
pub async fn agent_discard(
    svc: State<'_, AgentService>,
    id: Uuid,
    paths: Vec<String>,
) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_discard", || svc.discard(id, &paths))
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Backend push: push the agent's branch to origin (deterministic).
/// Blocking pool: shells out to the system git for credential handling.
#[tauri::command]
pub async fn agent_push(svc: State<'_, AgentService>, id: Uuid) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || crate::perf::timed("agent_push", || svc.push(id)))
        .await
        .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// AI-driven completion action: resolve the predefined prompt for `kind`
/// (commit/push/rebase/merge) and type it into the agent's RUNNING PTY. Errors if
/// the process isn't running (the UI enables these only when running) or `kind` is
/// unknown. Fire-and-forward — the agent runs the git with its own tools.
#[tauri::command(async)]
pub fn agent_action(
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    id: Uuid,
    kind: String,
) -> AppResult<()> {
    crate::perf::timed("agent_action", || {
        let agent = svc.get(id)?;
        let prompt = super::prompts::action_prompt(&kind, &agent.branch, &agent.base)
            .ok_or_else(|| AppError::Other(format!("unknown action: {kind}")))?;
        rt.write(id, &format!("{prompt}\r"))
            .map_err(AppError::Other)
    })
}

/// Blocking pool: reads HEAD blob + working file content.
#[tauri::command]
pub async fn agent_diff(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
    old_path: Option<String>,
) -> AppResult<crate::git::service::FileDiff> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_diff", || {
            svc.file_diff(id, &path, old_path.as_deref())
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Launch the agent's tool in its worktree (PTY-streamed) and mark it running.
///
/// `resume` (default false) requests a "Continue session" launch: when the agent
/// has a captured `session_id`, the tool is relaunched into that CLI session
/// (e.g. `claude --resume <id>`) instead of a fresh/bare run. Falls back to the
/// normal launch when there's no session id (or the tool has no resume flow), so
/// the existing Start/Resume button (resume=false) is unchanged.
#[tauri::command(async)]
pub fn agent_start<R: Runtime>(
    app: AppHandle<R>,
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    settings: State<'_, SettingsService>,
    bridge: State<'_, HookBridge>,
    id: Uuid,
    rows: u16,
    cols: u16,
    resume: Option<bool>,
) -> AppResult<()> {
    crate::perf::timed("agent_start", || {
        let agent = svc.get(id)?;
        // per-tool autoApprove policy → the adapter's skip-permissions flag
        // ("off" when unset/unreadable — the tool's own flow is the safe default)
        let approve_policy = settings
            .get()
            .unwrap_or_default()
            .auto_approve
            .get(&agent.tool)
            .cloned()
            .unwrap_or_else(|| "off".into());
        // resume-into-session only when asked AND a session id was captured
        let resume_session = if resume.unwrap_or(false) {
            agent.session_id.clone()
        } else {
            None
        };
        // deliver the initial task prompt only on the very first launch (never on a
        // resume-into-session, which continues an existing conversation)
        let send_prompt = !agent.started && resume_session.is_none();
        // Hooks are installed globally at startup; here we only stamp the ORRERY_*
        // env so this orrery-launched agent's hook brokers with the bridge. Gated on
        // the hook binary resolving (current_exe) — without it the hook can't run, so
        // we launch bare and rely on the PTY-parsing fallback.
        let hook_env = hook_binary().map(|_hook_bin| HookEnv {
            agent_id: id.to_string(),
            tool: agent.tool.clone(),
            endpoint: bridge.endpoint(),
            token: bridge.token().to_string(),
        });
        rt.start(
            app.clone(),
            &agent,
            rows,
            cols,
            send_prompt,
            hook_env.as_ref(),
            resume_session.as_deref(),
            &approve_policy,
        )
        .map_err(AppError::Other)?;
        if send_prompt {
            svc.mark_started(id)?;
        }
        let updated = svc.update(
            id,
            AgentUpdateRequest {
                status: Some("running".into()),
                task: None,
                model: None,
                name: None,
            },
        )?;
        emit_entity(&app, "agent", Change::Updated, updated);
        Ok(())
    })
}

/// Stop the agent's running process and mark it idle.
#[tauri::command(async)]
pub fn agent_stop<R: Runtime>(
    app: AppHandle<R>,
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<()> {
    crate::perf::timed("agent_stop", || {
        rt.stop(id);
        let updated = svc.update(
            id,
            AgentUpdateRequest {
                status: Some("idle".into()),
                task: None,
                model: None,
                name: None,
            },
        )?;
        emit_entity(&app, "agent", Change::Updated, updated);
        Ok(())
    })
}

/// Forward UI-terminal keystrokes into the agent's PTY stdin.
#[tauri::command(async)]
pub fn agent_input(rt: State<'_, RuntimeService>, id: Uuid, data: String) -> AppResult<()> {
    crate::perf::timed("agent_input", || {
        rt.write(id, &data).map_err(AppError::Other)
    })
}

/// Approve the agent's pending permission prompt by typing the tool's
/// approve-keystrokes into its PTY. Resolves the agent's tool → adapter →
/// `allow_keys()` so the RIGHT keys go to each tool's prompt UI (best-effort
/// until hook decision-forwarding lands; see `AgentAdapter::allow_keys`).
#[tauri::command(async)]
pub fn agent_allow(
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<()> {
    crate::perf::timed("agent_allow", || {
        let tool = svc.get(id)?.tool;
        let adapter = super::adapters::adapter_for(&tool)
            .ok_or_else(|| AppError::Other(format!("unknown tool: {tool}")))?;
        rt.write(id, adapter.allow_keys()).map_err(AppError::Other)
    })
}

/// Deny the agent's pending permission prompt by typing the tool's
/// deny-keystrokes into its PTY (mirror of [`agent_allow`], using `deny_keys()`).
#[tauri::command(async)]
pub fn agent_deny(
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<()> {
    crate::perf::timed("agent_deny", || {
        let tool = svc.get(id)?.tool;
        let adapter = super::adapters::adapter_for(&tool)
            .ok_or_else(|| AppError::Other(format!("unknown tool: {tool}")))?;
        rt.write(id, adapter.deny_keys()).map_err(AppError::Other)
    })
}

/// Select option `choice` (1-based) in the agent's pending numbered SELECT prompt
/// (e.g. an AskUserQuestion-style ask) by typing the tool's decide-keystrokes
/// into its PTY. Resolves the agent's tool → adapter → `decide_keys(choice)`
/// (mirror of [`agent_allow`]). Best-effort, fire-and-forget — NOT a forwarded
/// decision; assumes a numbered select where 1..N maps to the displayed options
/// (see `AgentAdapter::decide_keys`).
#[tauri::command(async)]
pub fn agent_decide(
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    id: Uuid,
    choice: u32,
) -> AppResult<()> {
    crate::perf::timed("agent_decide", || {
        let tool = svc.get(id)?.tool;
        let adapter = super::adapters::adapter_for(&tool)
            .ok_or_else(|| AppError::Other(format!("unknown tool: {tool}")))?;
        rt.write(id, &adapter.decide_keys(choice))
            .map_err(AppError::Other)
    })
}

/// Tell the output mux which agent's terminal is focused (`None` = none): the
/// focused agent's PTY output drains every ~16ms frame (typing echo stays
/// snappy) while unfocused agents coalesce losslessly at a ~150ms cadence.
#[tauri::command(async)]
pub fn agent_focus(rt: State<'_, RuntimeService>, id: Option<Uuid>) -> AppResult<()> {
    crate::perf::timed("agent_focus", || {
        rt.focus(id);
        Ok(())
    })
}

/// Resize the agent's PTY to match the visible terminal (cols × rows).
#[tauri::command(async)]
pub fn agent_resize(
    rt: State<'_, RuntimeService>,
    id: Uuid,
    rows: u16,
    cols: u16,
) -> AppResult<()> {
    crate::perf::timed("agent_resize", || {
        rt.resize(id, rows, cols).map_err(AppError::Other)
    })
}

/// Start watching an agent's worktree (replaces any previous watch). The
/// watcher scans backend-side and pushes `agent://changed` {id, changes, head}.
#[tauri::command(async)]
pub fn agent_watch<R: Runtime>(
    app: AppHandle<R>,
    watch: State<'_, WatchService>,
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<()> {
    crate::perf::timed("agent_watch", || {
        let worktree = svc.get(id)?.worktree;
        let scan_svc = svc.inner().clone();
        watch.watch(app, id, std::path::PathBuf::from(worktree), move || {
            // Why: scans run on the watcher thread, not in a command — timing
            // them here is what keeps backend scan cost/rate visible in the
            // perf table now that the frontend no longer pulls agent_changes.
            crate::perf::timed("agent_scan", || scan_svc.scan(id))
        });
        Ok(())
    })
}

/// One directory's immediate children — used to lazily expand an unloaded folder.
#[tauri::command(async)]
pub fn agent_dir(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
) -> AppResult<Vec<crate::fs::FileNode>> {
    crate::perf::timed("agent_dir", || {
        let agent = svc.get(id)?;
        Ok(crate::fs::list_dir(
            std::path::Path::new(&agent.worktree),
            &path,
        ))
    })
}

/// Ids of the agents that were in-flight when the app launched — captured at
/// startup BEFORE `reset_running` dropped them to idle (see lib.rs setup).
/// ONE-SHOT: the snapshot is drained on first read, so the auto-resume flow
/// can't relaunch the same agents twice (e.g. on a frontend reload).
#[tauri::command(async)]
pub fn agents_interrupted(state: State<'_, InterruptedAgents>) -> AppResult<Vec<Uuid>> {
    crate::perf::timed("agents_interrupted", || Ok(state.drain()))
}

/// Detection of which CLI coding agents are installed — delegated to the adapter
/// registry so only-installed tools are offered (and, later, hooked).
/// Blocking pool: shells out per known tool.
#[tauri::command]
pub async fn detect_tools() -> AppResult<Vec<super::adapters::ToolStatus>> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::perf::timed("detect_tools", super::adapters::installed)
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))
}
