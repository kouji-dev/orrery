use tauri::{AppHandle, Manager as _, Runtime, State};
use uuid::Uuid;

use crate::agents::adapters::HookEnv;
use crate::core::errors::{AppError, AppResult};
use crate::core::events::{emit_entity, Change};
use crate::git::service::WorktreeDisposal;
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

/// Blocking pool: prunes the worktree's git metadata and drops the agent.
///
/// `hard` is the confirm modal's "also delete the folder" checkbox. Default
/// (false) leaves the directory on disk — deleting someone's uncommitted work
/// is opt-in, never a side effect of removing an agent.
#[tauri::command]
pub async fn agent_remove<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    watch: State<'_, WatchService>,
    rt: State<'_, RuntimeService>,
    id: Uuid,
    hard: Option<bool>,
) -> AppResult<()> {
    let project_path = svc
        .get(id)
        .ok()
        .and_then(|a| projects.path_of(a.project_id).ok());
    // Kill the agent's PTY first (no-op when idle) — a live process holds its
    // cwd open, which makes remove_dir_all fail on Windows.
    rt.stop(id);
    rt.drop_scrollback(id); // the A1.2 ring dies with the agent
    watch.unwatch(id); // stop watching its worktree before tearing it down
    // B4.4: the agent's local-history snapshots die with it
    if let Some(history) = app.try_state::<crate::history::HistoryService>() {
        history.purge(id);
    }
    let svc = svc.inner().clone();
    let disposal = if hard.unwrap_or(false) {
        WorktreeDisposal::DeleteFolder
    } else {
        WorktreeDisposal::KeepFolder
    };
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_remove", || {
            svc.remove(
                id,
                project_path.as_deref().map(std::path::Path::new),
                disposal,
            )
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

/// Sidebar counter totals for ONE agent (camelCase over the bridge).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeTotals {
    pub id: String,
    pub add: i64,
    pub del: i64,
    pub files: usize,
}

/// Initialization pass for the sidebar's per-agent change counters: FULL
/// line-count totals for EVERY agent, in one call. After this, only watcher
/// scans update them (full detail for running/visible agents — see
/// `scan_detail`), so idle projects cost nothing further.
#[tauri::command]
pub async fn agent_change_totals(svc: State<'_, AgentService>) -> AppResult<Vec<ChangeTotals>> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_change_totals", || {
            let mut out = Vec::new();
            for a in svc.list()? {
                let Ok(changes) = svc.changes(a.id) else {
                    continue; // missing worktree — no counters to report
                };
                out.push(ChangeTotals {
                    id: a.id.to_string(),
                    add: changes.iter().map(|c| c.add).sum(),
                    del: changes.iter().map(|c| c.del).sum(),
                    files: changes.len(),
                });
            }
            Ok(out)
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
        let cfg = settings.get().unwrap_or_default();
        // per-tool autoApprove policy → the adapter's skip-permissions flag
        // ("off" when unset/unreadable — the tool's own flow is the safe default)
        let approve_policy = cfg
            .auto_approve
            .get(&agent.tool)
            .cloned()
            .unwrap_or_else(|| "off".into());
        // user-set manual executable path (Settings → Agent defaults), if any —
        // launched instead of resolving the tool's binary on PATH
        let program_override = cfg
            .tool_paths
            .get(&agent.tool)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
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
            program_override.as_deref(),
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

/// v2 project tabs: launch the user's default shell in the project's MAIN
/// worktree, keyed by the PROJECT id — input/resize/snapshot/interest reuse
/// the agent PTY commands (they only touch the runtime's proc map).
#[tauri::command(async)]
pub fn shell_start<R: Runtime>(
    app: AppHandle<R>,
    rt: State<'_, RuntimeService>,
    svc: State<'_, AgentService>,
    id: Uuid,
    rows: u16,
    cols: u16,
) -> AppResult<()> {
    crate::perf::timed("shell_start", || {
        // resolves via the project pseudo record → worktree = the repo root
        let rec = svc.get(id)?;
        rt.start_shell(app, id, std::path::Path::new(&rec.worktree), rows, cols)
            .map_err(AppError::Other)
    })
}

/// Stop a project tab's shell session (no agent-table update — the shell is
/// not a stored agent).
#[tauri::command(async)]
pub fn shell_stop(rt: State<'_, RuntimeService>, id: Uuid) -> AppResult<()> {
    crate::perf::timed("shell_stop", || {
        rt.stop(id);
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

/// One entry of the frontend's interest set (A0.2): which agent, and how much
/// of its PTY output the renderer wants. `mode` is "stream" | "digest" |
/// "none" — anything else degrades to "none" (emit nothing; safe direction).
#[derive(Debug, serde::Deserialize)]
pub struct InterestEntry {
    pub id: Uuid,
    pub mode: String,
}

/// A0.2 interest subscription — SUPERSEDES `agent_focus`/`set_focus`. The
/// frontend publishes the full set derived from what is visible (terminal
/// panes = stream, overview mini-previews = digest, diff/file views &
/// unmounted agents = absent/none, a HIDDEN window demotes stream → digest);
/// the backend diffs it against the current set. `none` only ever means
/// do-not-EMIT — every PTY keeps being read and its bytes land in the bounded
/// A1.2 scrollback ring.
///
/// Each mode transition is recorded (A0.7 correlate note): a tiny
/// `runtime://interest` telemetry event carries the new set's counts + how
/// many agents changed, so emit volume can be interpreted against what was on
/// screen.
#[tauri::command(async)]
pub fn runtime_subscribe<R: Runtime>(
    app: AppHandle<R>,
    rt: State<'_, RuntimeService>,
    watch: State<'_, WatchService>,
    entries: Vec<InterestEntry>,
) -> AppResult<()> {
    crate::perf::timed("runtime_subscribe", || {
        use crate::runtime::output_mux::Mode;
        let parsed: Vec<(String, Mode)> = entries
            .into_iter()
            .map(|e| (e.id.to_string(), Mode::parse(&e.mode)))
            .collect();
        let (stream, digest): (usize, usize) = parsed.iter().fold((0, 0), |(s, d), (_, m)| {
            match m {
                Mode::Stream => (s + 1, d),
                Mode::Digest => (s, d + 1),
                Mode::None => (s, d),
            }
        });
        let transitions = rt.subscribe(parsed);
        for t in &transitions {
            log::debug!("interest {}: {} → {}", t.id, t.from.as_str(), t.to.as_str());
        }
        // A2.2 reveal: an agent transitioning TO stream had counts-only
        // background scans — push one full-detail scan so its changes panel
        // shows real line counts without waiting for the next fs event.
        for t in &transitions {
            if t.to == Mode::Stream {
                if let Ok(aid) = uuid::Uuid::parse_str(&t.id) {
                    watch.rescan(aid);
                }
            }
        }
        if !transitions.is_empty() {
            let _ = crate::core::emit::emit_tracked(
                &app,
                "runtime://interest",
                &serde_json::json!({
                    "stream": stream,
                    "digest": digest,
                    "changed": transitions.len(),
                }),
            );
        }
        Ok(())
    })
}

/// A1.2 scrollback snapshot: the agent's recent raw output (lossy UTF-8
/// string — same shape output ships in) + the cumulative byte seq of its last
/// byte. Recovery contract: `term.clear()` → write `text` → drop queued live
/// chunks with `seq <= endSeq` → resume the live stream.
#[tauri::command(async)]
pub fn runtime_snapshot(
    rt: State<'_, RuntimeService>,
    id: Uuid,
) -> AppResult<crate::runtime::scrollback::Snapshot> {
    crate::perf::timed("runtime_snapshot", || Ok(rt.snapshot(id)))
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
            // A2.2: only a STREAM-subscribed agent (its terminal is on screen)
            // pays for per-file line-count diffing; everyone else scans
            // counts-only. Full deltas are pushed on reveal — see
            // runtime_subscribe → watch.rescan on a →stream transition.
            // TODO(A2.2 follow-up): an agent whose DIFF view is open is
            // `none`-mode by design (A0.2) — those panes pull full counts via
            // agent_changes; consider a git-interest mode if that ever pushes.
            let full =
                crate::runtime::interest_mode(id) == crate::runtime::output_mux::Mode::Stream;
            crate::perf::timed("agent_scan", || scan_svc.scan_detail(id, full))
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

// ── git-inspection commands ────────────────────────────────────────────────

/// Files changed by a single commit in the agent's worktree.
/// Returns the same `FileChange[]` shape as `agent_changes`.
#[tauri::command]
pub async fn agent_commit_diff(
    svc: State<'_, AgentService>,
    id: Uuid,
    sha: String,
) -> AppResult<Vec<crate::git::service::FileChange>> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_commit_diff", || {
            let agent = svc.get(id)?;
            svc.git().commit_files(std::path::Path::new(&agent.worktree), &sha)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Old/new content for a single file at a given commit in the agent's worktree.
#[tauri::command]
pub async fn agent_commit_file_diff(
    svc: State<'_, AgentService>,
    id: Uuid,
    sha: String,
    path: String,
) -> AppResult<crate::git::service::FileDiff> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_commit_file_diff", || {
            let agent = svc.get(id)?;
            svc.git().commit_file_diff(std::path::Path::new(&agent.worktree), &sha, &path)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Return value for `agent_range_files`: the aggregate file change list plus
/// the boundary commit shas (oldest → newest, after sorting by commit time).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeFiles {
    pub files: Vec<crate::git::service::FileChange>,
    pub from: String,
    pub to: String,
}

/// Files changed across a set of commit shas. Shas are sorted by commit time
/// (oldest first); the diff is from the oldest commit's tree to the newest
/// commit's tree. Returns `{ files, from, to }`.
#[tauri::command]
pub async fn agent_range_files(
    svc: State<'_, AgentService>,
    id: Uuid,
    shas: Vec<String>,
) -> AppResult<RangeFiles> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_range_files", || {
            if shas.is_empty() {
                return Err(AppError::Other("shas must not be empty".into()));
            }
            let agent = svc.get(id)?;
            let wt = std::path::Path::new(&agent.worktree);

            // Resolve all shas and sort by commit time (ascending)
            let mut commits: Vec<(i64, String)> = shas
                .iter()
                .map(|sha| svc.git().commit_time(wt, sha))
                .collect::<AppResult<Vec<_>>>()?;
            commits.sort_by_key(|(t, _)| *t);
            let from = commits.first().map(|(_, s)| s.clone()).unwrap();
            let to = commits.last().map(|(_, s)| s.clone()).unwrap();
            let files = svc.git().range_diff(wt, &from, &to)?;
            Ok(RangeFiles { files, from, to })
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Old/new content of `path` across the range `from`..`to` (commit shas).
#[tauri::command]
pub async fn agent_range_file_diff(
    svc: State<'_, AgentService>,
    id: Uuid,
    from: String,
    to: String,
    path: String,
) -> AppResult<crate::git::service::FileDiff> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_range_file_diff", || {
            let agent = svc.get(id)?;
            svc.git().range_file_diff(std::path::Path::new(&agent.worktree), &from, &to, &path)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Blame for `path` at `rev` (or HEAD when omitted) in the agent's worktree.
#[tauri::command]
pub async fn agent_blame(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
    rev: Option<String>,
) -> AppResult<crate::git::service::Blame> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_blame", || {
            let agent = svc.get(id)?;
            svc.git().blame(std::path::Path::new(&agent.worktree), &path, rev.as_deref())
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Commits that touch `path` in the agent's worktree, paged.
#[tauri::command]
pub async fn agent_file_history(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
    limit: Option<usize>,
    offset: Option<usize>,
) -> AppResult<Vec<crate::git::service::FileHistoryEntry>> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_file_history", || {
            let agent = svc.get(id)?;
            svc.git().file_history(std::path::Path::new(&agent.worktree),
                &path,
                limit.unwrap_or(100),
                offset.unwrap_or(0),
            )
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Both sides' per-line blame for the working-tree diff of `path`: `old` blamed
/// at HEAD, `new` blamed against the working tree (uncommitted lines flagged).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingBlame {
    pub old: crate::git::service::Blame,
    pub new: crate::git::service::Blame,
}

#[tauri::command]
pub async fn agent_working_blame(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
) -> AppResult<WorkingBlame> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_working_blame", || {
            let agent = svc.get(id)?;
            let (old, new) = svc.git().working_blame(std::path::Path::new(&agent.worktree), &path)?;
            Ok(WorkingBlame { old, new })
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

// ── native merge + conflict session commands (A3.5 / A3.6) ────────────────

/// Native merge of `branch` into the agent's branch, inside its worktree.
/// Clean/FF merges resolve immediately (empty `conflicts`); on conflict the
/// merge state is kept and the returned session lists the conflicted files —
/// finish with `agent_merge_continue` or discard with `agent_merge_abort`.
/// Blocking pool: git2 merge + conflicted checkout.
#[tauri::command]
pub async fn agent_merge(
    svc: State<'_, AgentService>,
    id: Uuid,
    branch: String,
) -> AppResult<crate::git::service::MergeSession> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_merge", || {
            let agent = svc.get(id)?;
            svc.git().merge(std::path::Path::new(&agent.worktree), &branch)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// The still-conflicted files of the in-progress session (index stages 1/2/3
/// + the marker-bearing working-tree content).
#[tauri::command]
pub async fn agent_conflicts(
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<Vec<crate::git::service::ConflictFile>> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_conflicts", || {
            let agent = svc.get(id)?;
            svc.git().conflict_files(std::path::Path::new(&agent.worktree))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Write `content` as the resolution of `path` and stage it.
#[tauri::command]
pub async fn agent_conflict_resolve(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
    content: String,
) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_conflict_resolve", || {
            let agent = svc.get(id)?;
            svc.git().conflict_resolve(
                std::path::Path::new(&agent.worktree),
                &path,
                &content,
            )
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Abort the in-progress merge (cleanup_state + hard reset to HEAD).
#[tauri::command]
pub async fn agent_merge_abort(svc: State<'_, AgentService>, id: Uuid) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_merge_abort", || {
            let agent = svc.get(id)?;
            svc.git().merge_abort(std::path::Path::new(&agent.worktree))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Commit the fully-resolved merge (HEAD + MERGE_HEAD parents) → short sha.
#[tauri::command]
pub async fn agent_merge_continue(
    svc: State<'_, AgentService>,
    id: Uuid,
    message: Option<String>,
) -> AppResult<String> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_merge_continue", || {
            let agent = svc.get(id)?;
            svc.git().merge_continue(std::path::Path::new(&agent.worktree), message.as_deref())
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Whether a merge/rebase/cherry-pick is in progress in the agent's worktree.
#[tauri::command]
pub async fn agent_session_state(
    svc: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<crate::git::service::SessionState> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_session_state", || {
            let agent = svc.get(id)?;
            svc.git().session_state(std::path::Path::new(&agent.worktree))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Detection of which CLI coding agents are installed — delegated to the adapter
/// registry so only-installed tools are offered (and, later, hooked).
/// Blocking pool: shells out per known tool.
#[tauri::command]
pub async fn detect_tools(
    settings: State<'_, SettingsService>,
) -> AppResult<Vec<super::adapters::ToolStatus>> {
    let overrides = settings.get().unwrap_or_default().tool_paths;
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("detect_tools", || super::adapters::installed(&overrides))
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))
}

/// Verify a candidate executable path for a tool by running `<path> --version`.
/// Returns the same `ToolStatus` shape (status ok|error, path, version, reason)
/// so the Settings "Use this path" flow can show whether the binary launches.
/// Blocking pool: shells out once.
#[tauri::command]
pub async fn verify_tool_path(id: String, path: String) -> AppResult<super::adapters::ToolStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("verify_tool_path", || {
            super::adapters::detect_at(&id, path.trim())
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))
}
