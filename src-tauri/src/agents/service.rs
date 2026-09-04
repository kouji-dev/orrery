use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::core::database::DB;
use crate::core::errors::{AgentError, AppError, AppResult, DbError};
use crate::git::service::{
    is_trash_dir, remove_dir_all_retry, rename_retry, trash_path, FileChange, GitService,
    WorktreeDisposal,
};
use crate::projects::model::CommitView;
use crate::settings::SettingsService;

use super::model::{Agent, AgentRecord, AgentSpawnRequest, AgentUpdateRequest};

#[derive(Clone)]
pub struct AgentService {
    db: DB,
    git: GitService,
    worktree_root: PathBuf,
    /// Consulted at spawn time for the branch template + worktree root. Shares
    /// the same DB; injected so tests can stage settings before spawning.
    settings: SettingsService,
}

impl AgentService {
    pub fn new(db: DB, git: GitService, worktree_root: PathBuf, settings: SettingsService) -> Self {
        let svc = Self {
            db,
            git,
            worktree_root,
            settings,
        };
        svc.init_schema();
        svc
    }

    /// The git handle (shared status cache) for command modules that work on
    /// an agent's worktree path directly.
    pub fn git(&self) -> &GitService {
        &self.git
    }

    /// Filesystem-safe worktree name from the agent's (unique-per-project) name.
    fn worktree_name(name: &str, id: &Uuid) -> String {
        let mut slug = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_");
        slug.truncate(60);
        let slug = slug.trim_matches('_').to_string();
        if slug.is_empty() {
            format!("agent_{}", &id.to_string()[..6]) // empty name → fall back to id
        } else {
            slug
        }
    }

    /// Where new worktrees go: the user's configured root when it is a usable
    /// absolute directory, else the ctor root (app-data/worktrees). "Usable"
    /// means absolute AND creatable — a relative path, a dead drive letter, or a
    /// permission wall must degrade to the known-good default, not fail spawn.
    fn effective_worktree_root(&self, setting: &str) -> PathBuf {
        let s = setting.trim();
        if s.is_empty() {
            return self.worktree_root.clone();
        }
        let p = PathBuf::from(s);
        if !p.is_absolute() {
            log::warn!("worktreeRoot '{s}' is not absolute — using the default root");
            return self.worktree_root.clone();
        }
        if let Err(e) = std::fs::create_dir_all(&p) {
            log::warn!("worktreeRoot '{s}' unusable ({e}) — using the default root");
            return self.worktree_root.clone();
        }
        p
    }

    /// The root new worktrees currently go under: the configured one when
    /// usable, else the app-data default (see `effective_worktree_root`).
    pub fn worktree_root_effective(&self) -> PathBuf {
        let prefs = self.settings.get().unwrap_or_default();
        self.effective_worktree_root(&prefs.worktree_root)
    }

    fn name_exists_in_project(&self, project_id: Uuid, name: &str) -> AppResult<bool> {
        let c = self.db.lock().unwrap();
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM agents WHERE project_id = ?1 AND name = ?2",
                rusqlite::params![project_id.to_string(), name],
                |r| r.get(0),
            )
            .map_err(DbError::Sqlite)?;
        Ok(n > 0)
    }

    fn init_schema(&self) {
        let c = self.db.lock().unwrap();
        // project_id is the persisted link to a project (no FK yet — app-layer integrity;
        // cascade-on-project-delete handled by remove_for_project).
        c.execute(
            "CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                tool TEXT NOT NULL,
                model TEXT NOT NULL,
                effort TEXT,
                name TEXT NOT NULL,
                task TEXT NOT NULL,
                status TEXT NOT NULL,
                branch TEXT NOT NULL,
                worktree TEXT NOT NULL,
                base TEXT NOT NULL,
                started INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .unwrap();
        // migrate DBs created before the `started` column existed (ignored if present)
        let _ = c.execute(
            "ALTER TABLE agents ADD COLUMN started INTEGER NOT NULL DEFAULT 0",
            [],
        );
        // migrate DBs created before the `session_id` column existed (ignored if present)
        let _ = c.execute("ALTER TABLE agents ADD COLUMN session_id TEXT", []);
        // migrate DBs created before the `ticket_id` column existed (ignored if present)
        let _ = c.execute("ALTER TABLE agents ADD COLUMN ticket_id TEXT", []);
    }

    /// Record → view model. Runtime fields are defaulted (no disk/process access yet).
    fn enrich(&self, rec: AgentRecord) -> Agent {
        Agent {
            id: rec.id,
            project_id: rec.project_id,
            tool: rec.tool,
            model: rec.model,
            effort: rec.effort,
            name: rec.name,
            task: rec.task,
            status: rec.status,
            branch: rec.branch,
            worktree: rec.worktree,
            base: rec.base,
            started: rec.started,
            session_id: rec.session_id,
            ticket_id: rec.ticket_id,
            commits: 0,
            elapsed: 0,
            progress: 0.0,
            pending: Vec::new(),
            block_reason: None,
            wait_reason: None,
        }
    }

    pub fn list(&self) -> AppResult<Vec<Agent>> {
        Ok(self
            .records()?
            .into_iter()
            .map(|r| self.enrich(r))
            .collect())
    }

    pub fn get(&self, id: Uuid) -> AppResult<Agent> {
        Ok(self.enrich(self.record(id)?))
    }

    /// Create an agent + its git worktree (named from the task). Lazy: status starts
    /// `idle` and no process is launched — that happens on Start (task #5/#7).
    pub fn spawn(&self, req: AgentSpawnRequest, project_path: &Path) -> AppResult<Agent> {
        if req.name.trim().is_empty() {
            return Err(AgentError::Required("name is empty".into()).into());
        }
        // The client may pre-pick the id (optimistic placeholder row); a clash
        // with an existing row fails the INSERT below, which is the right outcome.
        let id = req.id.unwrap_or_else(Uuid::new_v4);
        // Duplicate names ARE allowed (agents are keyed by their uuid id). When the
        // name is already taken in this project, disambiguate the worktree slug with
        // a short uuid fragment so BOTH the worktree dir and the template-derived
        // branch (built from wt_name below) stay unique.
        let mut wt_name = Self::worktree_name(&req.name, &id);
        if self.name_exists_in_project(req.project_id, &req.name)? {
            wt_name = format!("{}-{}", wt_name, &id.to_string()[..6]);
        }
        // settings are best-effort consumers: a read failure falls back to
        // defaults (the historical branch shape + ctor root), never blocks spawn.
        let prefs = self.settings.get().unwrap_or_default();
        let branch = branch_from_template(&prefs.branch_template, &wt_name, &req.tool, &mmdd_now());
        let root = self.effective_worktree_root(&prefs.worktree_root);
        // flat: worktrees/<name>. Only disambiguate on a real cross-project name clash.
        let mut wt_path = root.join(&wt_name);
        if wt_path.exists() {
            wt_path = root.join(format!("{}-{}", wt_name, &id.to_string()[..6]));
        }

        // best-effort: only real git projects get a worktree
        if self.git.detect(project_path) {
            if let Err(e) =
                self.git
                    .create_worktree(project_path, &wt_name, &branch, Some(&req.base), &wt_path)
            {
                log::warn!("worktree create failed for agent {id}: {e:?}");
            }
        }

        let rec = AgentRecord {
            id,
            project_id: req.project_id,
            tool: req.tool,
            model: req.model,
            effort: req.effort,
            name: req.name,
            task: req.task,
            status: "idle".into(),
            branch,
            worktree: wt_path.to_string_lossy().to_string(),
            base: req.base,
            started: false,
            session_id: None,
            ticket_id: req.ticket_id,
        };
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "INSERT INTO agents
                    (id, project_id, tool, model, effort, name, task, status, branch, worktree, base, started, ticket_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                rusqlite::params![
                    rec.id.to_string(),
                    rec.project_id.to_string(),
                    rec.tool,
                    rec.model,
                    rec.effort,
                    rec.name,
                    rec.task,
                    rec.status,
                    rec.branch,
                    rec.worktree,
                    rec.base,
                    rec.started,
                    rec.ticket_id.map(|u| u.to_string()),
                ],
            )
            .map_err(DbError::Sqlite)?;
        }
        Ok(self.enrich(rec))
    }

    pub fn update(&self, id: Uuid, req: AgentUpdateRequest) -> AppResult<Agent> {
        let mut rec = self.record(id)?;
        if let Some(status) = req.status {
            if !status.trim().is_empty() {
                rec.status = status;
            }
        }
        if let Some(task) = req.task {
            rec.task = task;
        }
        if let Some(model) = req.model {
            rec.model = model;
        }
        if let Some(name) = req.name {
            if !name.trim().is_empty() {
                rec.name = name;
            }
        }
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "UPDATE agents SET status = ?2, task = ?3, model = ?4, name = ?5 WHERE id = ?1",
                rusqlite::params![id.to_string(), rec.status, rec.task, rec.model, rec.name],
            )
            .map_err(DbError::Sqlite)?;
        }
        Ok(self.enrich(rec))
    }

    /// Working-tree changes in the agent's worktree (transient — computed on demand).
    pub fn changes(&self, id: Uuid) -> AppResult<Vec<FileChange>> {
        let rec = self.record(id)?;
        Ok(self.git.status(Path::new(&rec.worktree)))
    }

    /// One worktree scan for the watcher push: working-tree changes + HEAD oid.
    /// A missing agent (mid-removal race) scans empty — its watcher is about to
    /// be dropped anyway. Composition of tested pieces; covered end-to-end by
    /// the watch integration test.
    pub fn scan(&self, id: Uuid) -> crate::watch::ScanResult {
        self.scan_detail(id, true)
    }

    /// [`scan`] with the detail level explicit (A2.2): `full = false` skips
    /// per-file line-count diffing (states/paths only, add/del = 0) — the
    /// cheap background scan for non-focused agents. Full deltas are pushed
    /// on reveal via `WatchService::rescan`.
    pub fn scan_detail(&self, id: Uuid, full: bool) -> crate::watch::ScanResult {
        let Ok(rec) = self.get(id) else {
            return crate::watch::ScanResult {
                changes: Vec::new(),
                head: None,
                full: true,
            };
        };
        // A RUNNING agent always scans full: it is the one actively producing
        // changes, and the sidebar counters must track it without its terminal
        // being on screen. Idle background agents stay on the cheap path.
        let full = full || rec.status == "running";
        let p = Path::new(&rec.worktree);
        crate::watch::ScanResult {
            changes: if full {
                self.git.status(p)
            } else {
                self.git.status_counts_only(p)
            },
            head: self.git.head_oid(p),
            full,
        }
    }

    /// Commit selected paths (or all when empty) in the agent's worktree.
    pub fn commit(&self, id: Uuid, message: &str, paths: &[String]) -> AppResult<String> {
        let rec = self.record(id)?;
        self.git.commit(Path::new(&rec.worktree), message, paths)
    }

    /// Commits on the agent's branch — read from its worktree (whose HEAD *is*
    /// the agent branch), newest first, each tagged with the **agent id** (not the
    /// commit author) so the UI can group commits by agent. Empty for a worktree
    /// with no commits / no repo.
    pub fn commits(&self, id: Uuid, limit: usize, offset: usize) -> AppResult<Vec<CommitView>> {
        let rec = self.record(id)?;
        Ok(self
            .git
            .log(Path::new(&rec.worktree), limit, offset)
            .into_iter()
            .map(|e| CommitView {
                agent: id.to_string(),
                project_id: rec.project_id,
                sha: e.sha,
                msg: e.message,
                when: crate::projects::service::relative_time(e.time),
                ts: e.time,
                files: e.files as i64,
            })
            .collect())
    }

    /// Discard selected paths (or all when empty) in the agent's worktree.
    pub fn discard(&self, id: Uuid, paths: &[String]) -> AppResult<()> {
        let rec = self.record(id)?;
        self.git.discard(Path::new(&rec.worktree), paths)
    }

    /// Push the agent's branch to `origin` (deterministic backend push).
    pub fn push(&self, id: Uuid) -> AppResult<()> {
        let rec = self.record(id)?;
        self.git
            .push(Path::new(&rec.worktree), "origin", &rec.branch)
    }

    /// Old/new content of a file in the agent's worktree, for the diff view.
    /// `old_path` (the pre-move path) is passed through for renamed/moved files so
    /// the diff compares the right OLD content.
    pub fn file_diff(
        &self,
        id: Uuid,
        path: &str,
        old_path: Option<&str>,
    ) -> AppResult<crate::git::service::FileDiff> {
        let rec = self.record(id)?;
        Ok(self.git.file_diff(Path::new(&rec.worktree), path, old_path))
    }

    /// Drop an agent. `disposal` decides the fate of its worktree directory:
    /// [`KeepFolder`](WorktreeDisposal::KeepFolder) deregisters the worktree and
    /// leaves the files, [`DeleteFolder`](WorktreeDisposal::DeleteFolder) erases
    /// them.
    ///
    /// The folder is not deleted in place: it is RENAMED to a `.trash-` sibling
    /// (one metadata op, milliseconds) and purged on a background thread. A
    /// worktree full of `node_modules` took tens of seconds to erase on NTFS,
    /// and the sidebar row could not drop before that finished. A rename fails
    /// for the same reason a delete would (a handle open somewhere below), so
    /// the contract is unchanged: a failed move aborts the whole removal — the
    /// row survives, so the agent is still there to retry from — which is why
    /// the directory goes first, before any git or database state has moved.
    pub fn remove(
        &self,
        id: Uuid,
        project_path: Option<&Path>,
        disposal: WorktreeDisposal,
    ) -> AppResult<()> {
        let rec = self.record(id).ok();

        let mut trash: Option<PathBuf> = None;
        if disposal == WorktreeDisposal::DeleteFolder {
            if let Some(wt) = rec.as_ref().map(|r| r.worktree.as_str()).filter(|w| !w.is_empty()) {
                // Deliberately keyed off the RECORDED path rather than git's
                // registration: spawn suffixes the directory on a name clash
                // without suffixing the worktree name, so find_worktree() can
                // miss a folder that really exists. Hard delete must not depend
                // on the two agreeing.
                let wt_path = Path::new(wt);
                if wt_path.exists() {
                    let aside = trash_path(wt_path);
                    rename_retry(wt_path, &aside).map_err(|e| {
                        AppError::Other(format!("delete worktree folder '{wt}': {e}"))
                    })?;
                    trash = Some(aside);
                }
            }
        }

        // best-effort: git metadata cleanup, once the folder question is settled
        if let (Some(pp), Some(rec)) = (project_path, rec.as_ref()) {
            if let Some(wt_name) = Path::new(&rec.worktree)
                .file_name()
                .and_then(|n| n.to_str())
            {
                let _ = self.git.remove_worktree(pp, wt_name, disposal);
            }
        }
        let n = {
            let c = self.db.lock().unwrap();
            c.execute("DELETE FROM agents WHERE id = ?1", [id.to_string()])
                .map_err(DbError::Sqlite)?
        };
        if n == 0 {
            return Err(AgentError::NotFound(id.to_string()).into());
        }
        if let Some(aside) = trash {
            Self::purge_in_background(aside);
        }
        Ok(())
    }

    /// Erase a moved-aside worktree folder on its own thread. Failure only
    /// logs: the folder is already out of the way and carries the trash marker,
    /// so [`sweep_trash`](Self::sweep_trash) retries it on the next launch.
    fn purge_in_background(aside: PathBuf) {
        let spawned = std::thread::Builder::new()
            .name("worktree-trash".into())
            .spawn(move || {
                let t0 = std::time::Instant::now();
                match remove_dir_all_retry(&aside) {
                    Ok(()) => log::info!(
                        "purged {} in {:?}",
                        aside.display(),
                        t0.elapsed()
                    ),
                    Err(e) => log::warn!("purge of {} failed: {e}", aside.display()),
                }
            });
        if let Err(e) = spawned {
            log::warn!("worktree-trash thread: {e}");
        }
    }

    /// Startup recovery: purge every `*.trash-*` folder left in the worktree
    /// roots (a crash or quit mid-purge). Runs on a background thread so it
    /// never delays setup; the default root and the configured one are both
    /// swept because either may hold moved-aside folders.
    pub fn sweep_trash(&self) {
        let prefs = self.settings.get().unwrap_or_default();
        let mut roots = vec![self.worktree_root.clone()];
        let configured = self.effective_worktree_root(&prefs.worktree_root);
        if configured != self.worktree_root {
            roots.push(configured);
        }
        let leftovers: Vec<PathBuf> = roots
            .iter()
            .filter_map(|root| std::fs::read_dir(root).ok())
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && is_trash_dir(p))
            .collect();
        for dir in leftovers {
            log::info!("sweeping leftover worktree trash {}", dir.display());
            Self::purge_in_background(dir);
        }
    }

    /// Cascade: drop every agent belonging to a project (called when the project is removed).
    /// Returns the ids of the removed agents so the caller can emit per-agent delete events.
    pub fn remove_for_project(&self, project_id: Uuid) -> AppResult<Vec<Uuid>> {
        let recs: Vec<AgentRecord> = self
            .records()?
            .into_iter()
            .filter(|r| r.project_id == project_id)
            .collect();
        let ids: Vec<Uuid> = recs.iter().map(|r| r.id).collect();
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "DELETE FROM agents WHERE project_id = ?1",
                [project_id.to_string()],
            )
            .map_err(DbError::Sqlite)?;
        }
        // best-effort: drop each agent's worktree working dir
        for r in &recs {
            let _ = std::fs::remove_dir_all(&r.worktree);
        }
        Ok(ids)
    }

    // ---- record readers ----

    /// Mark the agent as launched-at-least-once (so its prompt is delivered only once).
    pub fn mark_started(&self, id: Uuid) -> AppResult<()> {
        let c = self.db.lock().unwrap();
        c.execute(
            "UPDATE agents SET started = 1 WHERE id = ?1",
            [id.to_string()],
        )
        .map_err(DbError::Sqlite)?;
        Ok(())
    }

    /// Persist the tool's CLI session id (captured from a hook), so a later
    /// "Continue session" can relaunch with `claude --resume <session_id>`.
    pub fn set_session(&self, id: Uuid, session_id: &str) -> AppResult<()> {
        let c = self.db.lock().unwrap();
        c.execute(
            "UPDATE agents SET session_id = ?2 WHERE id = ?1",
            rusqlite::params![id.to_string(), session_id],
        )
        .map_err(DbError::Sqlite)?;
        Ok(())
    }

    /// Ids of agents currently marked in-flight (running/blocked/waiting).
    /// Read at launch BEFORE [`reset_running`] flips them to idle — these are
    /// the interrupted agents the auto-resume flow may relaunch.
    pub fn running_ids(&self) -> AppResult<Vec<Uuid>> {
        let raw: Vec<String> = {
            let c = self.db.lock().unwrap();
            let mut stmt = c
                .prepare(
                    "SELECT id FROM agents WHERE status IN ('running', 'blocked', 'waiting')",
                )
                .map_err(DbError::Sqlite)?;
            let rows = stmt.query_map([], |r| r.get(0)).map_err(DbError::Sqlite)?;
            rows.collect::<Result<_, _>>().map_err(DbError::Sqlite)?
        };
        raw.into_iter()
            .map(|s| Uuid::parse_str(&s).map_err(|e| AppError::Other(e.to_string())))
            .collect()
    }

    /// Reconcile stale state after a crash/restart: no PTY process can be alive
    /// right after launch, so any agent left mid-flight drops back to idle.
    pub fn reset_running(&self) -> AppResult<usize> {
        let c = self.db.lock().unwrap();
        let n = c
            .execute(
                "UPDATE agents SET status = 'idle' WHERE status IN ('running', 'blocked', 'waiting')",
                [],
            )
            .map_err(DbError::Sqlite)?;
        Ok(n)
    }

    fn records(&self) -> AppResult<Vec<AgentRecord>> {
        let raw: Vec<(
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            String,
            String,
            String,
            bool,
            Option<String>,
            Option<String>,
        )> = {
            let c = self.db.lock().unwrap();
            let mut stmt = c
                .prepare(
                    "SELECT id, project_id, tool, model, effort, name, task, status, branch, worktree, base, started, session_id, ticket_id FROM agents",
                )
                .map_err(DbError::Sqlite)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                        r.get(11)?,
                        r.get(12)?,
                        r.get(13)?,
                    ))
                })
                .map_err(DbError::Sqlite)?;
            rows.collect::<Result<_, _>>().map_err(DbError::Sqlite)?
        };

        raw.into_iter()
            .map(
                |(
                    id,
                    project_id,
                    tool,
                    model,
                    effort,
                    name,
                    task,
                    status,
                    branch,
                    worktree,
                    base,
                    started,
                    session_id,
                    ticket_id,
                )| {
                    let ticket_id = ticket_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|e| AppError::Other(e.to_string()))?;
                    Ok(AgentRecord {
                        id: Uuid::parse_str(&id).map_err(|e| AppError::Other(e.to_string()))?,
                        project_id: Uuid::parse_str(&project_id)
                            .map_err(|e| AppError::Other(e.to_string()))?,
                        tool,
                        model,
                        effort,
                        name,
                        task,
                        status,
                        branch,
                        worktree,
                        base,
                        started,
                        session_id,
                        ticket_id,
                    })
                },
            )
            .collect()
    }

    fn record(&self, id: Uuid) -> AppResult<AgentRecord> {
        let row: Option<(
            String,
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            String,
            String,
            String,
            bool,
            Option<String>,
            Option<String>,
        )> = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT project_id, tool, model, effort, name, task, status, branch, worktree, base, started, session_id, ticket_id FROM agents WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?)),
            )
            .optional()
            .map_err(DbError::Sqlite)?
        };
        match row {
            Some((
                project_id,
                tool,
                model,
                effort,
                name,
                task,
                status,
                branch,
                worktree,
                base,
                started,
                session_id,
                ticket_id,
            )) => {
                let ticket_id = ticket_id
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()
                    .map_err(|e| AppError::Other(e.to_string()))?;
                Ok(AgentRecord {
                    id,
                    project_id: Uuid::parse_str(&project_id)
                        .map_err(|e| AppError::Other(e.to_string()))?,
                    tool,
                    model,
                    effort,
                    name,
                    task,
                    status,
                    branch,
                    worktree,
                    base,
                    started,
                    session_id,
                    ticket_id,
                })
            }
            None => self.project_pseudo_record(id),
        }
    }

    /// v2 project tabs: a PROJECT id resolves to a pseudo agent record rooted
    /// at the repo's MAIN worktree, so every worktree-scoped command (tree,
    /// changes, diff, blame, file CRUD, search, commits, shell PTY) works on
    /// the project checkout with zero per-command special-casing. Nothing is
    /// written to the agents table and `list()`/watchers never see it — this
    /// is a read-time view, not a stored agent.
    fn project_pseudo_record(&self, id: Uuid) -> AppResult<AgentRecord> {
        let row: Option<(String, String)> = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT name, path FROM projects WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(DbError::Sqlite)?
        };
        let Some((name, path)) = row else {
            return Err(AgentError::NotFound(id.to_string()).into());
        };
        let branch = self
            .git
            .head_branch(Path::new(&path))
            .unwrap_or_else(|| "main".into());
        Ok(AgentRecord {
            id,
            project_id: id,
            tool: "shell".into(),
            model: String::new(),
            effort: None,
            name,
            task: String::new(),
            status: "idle".into(),
            branch: branch.clone(),
            worktree: path,
            base: branch,
            started: false,
            session_id: None,
            ticket_id: None,
        })
    }
}

/// One-shot snapshot of the agents that were in-flight when the app launched
/// (captured BEFORE `reset_running` dropped them to idle). Managed state; the
/// `agents_interrupted` command drains it, so auto-resume fires at most once
/// per app run — a later call (HMR, second window) gets an empty list and
/// can't double-relaunch anything.
pub struct InterruptedAgents(Mutex<Vec<Uuid>>);

impl InterruptedAgents {
    pub fn new(ids: Vec<Uuid>) -> Self {
        Self(Mutex::new(ids))
    }

    /// Take the snapshot, leaving it empty.
    pub fn drain(&self) -> Vec<Uuid> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

/// Expand the user's branch template and sanitize the result into a git-safe
/// ref name. Tokens: `{name}` → the worktree slug, `{tool}` → the tool id,
/// `{date}` → MMDD. An empty template, or one whose expansion sanitizes away
/// to nothing, falls back to the historical `agent/{slug}` shape — a bad
/// setting must never block spawning.
fn branch_from_template(template: &str, wt_name: &str, tool: &str, date: &str) -> String {
    let t = template.trim();
    let t = if t.is_empty() { "agent/{name}" } else { t };
    let raw = t
        .replace("{name}", wt_name)
        .replace("{tool}", tool)
        .replace("{date}", date);
    let cleaned = sanitize_branch(&raw);
    if cleaned.is_empty() {
        format!("agent/{wt_name}")
    } else {
        cleaned
    }
}

/// Reduce arbitrary input to a name `git check-ref-format` accepts: only
/// `[A-Za-z0-9._/-]` survive (everything else → `-`), `..` never appears, no
/// component starts/ends with `.` or `-` or ends in `.lock`, and empty
/// components (leading/trailing/double slashes) are dropped. Can return ""
/// (caller falls back).
fn sanitize_branch(raw: &str) -> String {
    let mapped: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/') {
                c
            } else {
                '-'
            }
        })
        .collect();
    mapped
        .split('/')
        .map(|comp| {
            let mut c = comp.replace("..", "-");
            if let Some(stripped) = c.strip_suffix(".lock") {
                c = stripped.to_string();
            }
            c.trim_matches(|ch| ch == '.' || ch == '-').to_string()
        })
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Today as MMDD for the `{date}` token. UTC — a branch date tag doesn't
/// justify a timezone dependency, and UTC is stable across machines.
fn mmdd_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (_, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{m:02}{d:02}")
}

/// Days-since-1970-01-01 → (year, month, day). Howard Hinnant's public-domain
/// `civil_from_days` algorithm — exact for the proleptic Gregorian calendar,
/// no date crate needed.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Arc;

    // a unique, persistent worktree root per service (tests don't clean it — temp dir)
    fn svc() -> AgentService {
        svc_with_settings().0
    }

    /// The service plus a handle to ITS settings (same DB) so tests can stage
    /// branchTemplate / worktreeRoot before spawning.
    fn svc_with_settings() -> (AgentService, SettingsService) {
        let db: DB = Arc::new(std::sync::Mutex::new(Connection::open_in_memory().unwrap()));
        let wt_root = std::env::temp_dir().join(format!("orrery-wt-{}", Uuid::new_v4()));
        let settings = SettingsService::new(db.clone());
        let svc = AgentService::new(db, GitService::new(), wt_root, settings.clone());
        (svc, settings)
    }

    // a non-git path so spawn skips real worktree creation
    fn nogit() -> PathBuf {
        std::env::temp_dir()
    }

    fn req(project_id: Uuid, name: &str) -> AgentSpawnRequest {
        AgentSpawnRequest {
            id: None,
            project_id,
            tool: "claude".into(),
            model: "opus".into(),
            effort: None,
            name: name.into(),
            task: "do the thing".into(),
            base: "main".into(),
            ticket_id: None,
        }
    }

    #[test]
    fn spawn_uses_client_supplied_id() {
        let s = svc();
        let id = Uuid::new_v4();
        let mut r = req(Uuid::new_v4(), "picked");
        r.id = Some(id);
        let a = s.spawn(r, &nogit()).unwrap();
        assert_eq!(a.id, id, "the placeholder id becomes the row id");
        // a second spawn under the same id must fail, not silently duplicate
        let mut again = req(Uuid::new_v4(), "picked-2");
        again.id = Some(id);
        assert!(s.spawn(again, &nogit()).is_err());
        assert_eq!(s.list().unwrap().len(), 1);
    }

    #[test]
    fn spawn_derives_branch_and_worktree_from_name() {
        let s = svc();
        let pid = Uuid::new_v4();
        let a = s.spawn(req(pid, "fix login"), &nogit()).unwrap();
        assert_eq!(a.project_id, pid);
        assert_eq!(a.branch, "agent/fix_login", "branch from snake_case(name)");
        assert!(
            a.worktree.replace('\\', "/").ends_with("/fix_login"),
            "flat worktree named after agent: {}",
            a.worktree
        );
        assert_eq!(a.status, "idle");
    }

    #[test]
    fn spawn_rejects_empty_name() {
        let s = svc();
        let err = s.spawn(req(Uuid::new_v4(), ""), &nogit()).unwrap_err();
        assert!(matches!(err, AppError::Agent(AgentError::Required(_))));
    }

    #[test]
    fn spawn_allows_duplicate_name_with_distinct_worktrees() {
        let s = svc();
        let pid = Uuid::new_v4();
        // duplicate names in the same project are allowed now...
        let a = s.spawn(req(pid, "dup"), &nogit()).unwrap();
        let b = s.spawn(req(pid, "dup"), &nogit()).unwrap();
        assert_eq!(a.name, b.name, "both keep the requested name");
        assert_ne!(a.id, b.id, "each agent has its own id");
        // ...but their worktrees (and thus branches) must be disambiguated
        assert_ne!(
            a.worktree, b.worktree,
            "second worktree gets a uuid-suffixed slug: {} vs {}",
            a.worktree, b.worktree
        );
        assert_ne!(a.branch, b.branch, "branches stay unique too");
        // same name in a different project is fine as well
        s.spawn(req(Uuid::new_v4(), "dup"), &nogit()).unwrap();
    }

    #[test]
    fn spawn_creates_real_worktree_for_git_project() {
        let s = svc();
        let proj = tempfile::tempdir().unwrap();
        GitService::new().init(proj.path()).unwrap(); // empty repo — spawn makes the initial commit
        let a = s.spawn(req(Uuid::new_v4(), "wt"), proj.path()).unwrap();
        assert!(
            Path::new(&a.worktree).exists(),
            "real worktree at {}",
            a.worktree
        );
    }

    #[test]
    fn commits_lists_worktree_commits_tagged_with_agent_id() {
        let s = svc();
        let proj = tempfile::tempdir().unwrap();
        GitService::new().init(proj.path()).unwrap(); // empty repo — spawn makes the initial commit
        let a = s.spawn(req(Uuid::new_v4(), "wt"), proj.path()).unwrap();
        // a commit made in the agent's worktree, on its agent/* branch
        std::fs::write(Path::new(&a.worktree).join("note.txt"), "hi").unwrap();
        s.commit(a.id, "add note", &[]).unwrap();

        let commits = s.commits(a.id, 50, 0).unwrap();
        assert!(!commits.is_empty(), "worktree commits must be listed");
        assert_eq!(commits[0].msg, "add note", "newest commit first");
        assert_eq!(
            commits[0].agent,
            a.id.to_string(),
            "tagged with the agent id, not the commit author name"
        );
        assert_eq!(commits[0].project_id, a.project_id);
    }

    #[test]
    fn list_and_get() {
        let s = svc();
        let pid = Uuid::new_v4();
        let a = s.spawn(req(pid, "nova"), &nogit()).unwrap();
        assert_eq!(s.list().unwrap().len(), 1);
        assert_eq!(s.get(a.id).unwrap().name, "nova");
    }

    #[test]
    fn update_changes_status() {
        let s = svc();
        let a = s.spawn(req(Uuid::new_v4(), "io"), &nogit()).unwrap();
        let upd = AgentUpdateRequest {
            status: Some("running".into()),
            task: None,
            model: None,
            name: None,
        };
        assert_eq!(s.update(a.id, upd).unwrap().status, "running");
        assert_eq!(s.get(a.id).unwrap().status, "running");
    }

    #[test]
    fn spawn_starts_unstarted_then_mark_started_flips_it() {
        let s = svc();
        let a = s.spawn(req(Uuid::new_v4(), "once"), &nogit()).unwrap();
        assert!(!a.started, "new agent has not been started");
        s.mark_started(a.id).unwrap();
        assert!(s.get(a.id).unwrap().started, "mark_started persists");
    }

    #[test]
    fn set_session_round_trips_and_defaults_none() {
        let s = svc();
        let a = s.spawn(req(Uuid::new_v4(), "sess"), &nogit()).unwrap();
        assert_eq!(
            a.session_id, None,
            "a fresh agent has no session id (migration default NULL)"
        );
        s.set_session(a.id, "abc-123").unwrap();
        assert_eq!(
            s.get(a.id).unwrap().session_id.as_deref(),
            Some("abc-123"),
            "set_session persists + reads back"
        );
    }

    #[test]
    fn reset_running_drops_inflight_agents_to_idle() {
        let s = svc();
        let run = s.spawn(req(Uuid::new_v4(), "run"), &nogit()).unwrap();
        let block = s.spawn(req(Uuid::new_v4(), "block"), &nogit()).unwrap();
        let done = s.spawn(req(Uuid::new_v4(), "done"), &nogit()).unwrap();
        let upd = |st: &str| AgentUpdateRequest {
            status: Some(st.into()),
            task: None,
            model: None,
            name: None,
        };
        s.update(run.id, upd("running")).unwrap();
        s.update(block.id, upd("blocked")).unwrap();
        s.update(done.id, upd("done")).unwrap();

        let n = s.reset_running().unwrap();
        assert_eq!(n, 2, "running + blocked reset; done untouched");
        assert_eq!(s.get(run.id).unwrap().status, "idle");
        assert_eq!(s.get(block.id).unwrap().status, "idle");
        assert_eq!(s.get(done.id).unwrap().status, "done");
    }

    #[test]
    fn get_missing_is_not_found() {
        let s = svc();
        let err = s.get(Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, AppError::Agent(AgentError::NotFound(_))));
    }

    #[test]
    fn remove_deletes() {
        let s = svc();
        let a = s.spawn(req(Uuid::new_v4(), "rm"), &nogit()).unwrap();
        s.remove(a.id, None, WorktreeDisposal::KeepFolder).unwrap();
        assert_eq!(s.list().unwrap().len(), 0);
    }

    /// Soft delete (the default) drops the agent but never the files.
    #[test]
    fn remove_keeps_the_worktree_folder_by_default() {
        let s = svc();
        let a = s.spawn(req(Uuid::new_v4(), "keepme"), &nogit()).unwrap();
        let wt = PathBuf::from(&a.worktree);
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join("uncommitted.txt"), "work").unwrap();

        s.remove(a.id, None, WorktreeDisposal::KeepFolder).unwrap();

        assert_eq!(s.list().unwrap().len(), 0, "agent gone");
        assert!(wt.join("uncommitted.txt").exists(), "folder left on disk");
        std::fs::remove_dir_all(&wt).ok();
    }

    /// Hard delete is the checkbox: the folder goes with the agent.
    #[test]
    fn remove_hard_deletes_the_worktree_folder() {
        let s = svc();
        let a = s.spawn(req(Uuid::new_v4(), "nukeme"), &nogit()).unwrap();
        let wt = PathBuf::from(&a.worktree);
        std::fs::create_dir_all(wt.join("nested")).unwrap();
        std::fs::write(wt.join("nested/deep.txt"), "work").unwrap();

        s.remove(a.id, None, WorktreeDisposal::DeleteFolder).unwrap();

        assert_eq!(s.list().unwrap().len(), 0, "agent gone");
        assert!(!wt.exists(), "folder deleted with it");
    }

    /// Regression guard: spawn suffixes the DIRECTORY on a clash without
    /// suffixing the git worktree name, so the two can disagree. A hard delete
    /// keys off the recorded path, and must erase the folder anyway.
    #[test]
    fn remove_hard_deletes_a_folder_git_does_not_know_about() {
        let s = svc();
        let pid = Uuid::new_v4();
        let first = s.spawn(req(pid, "clash"), &nogit()).unwrap();
        std::fs::create_dir_all(&first.worktree).unwrap();
        let second = s.spawn(req(pid, "clash"), &nogit()).unwrap();

        let wt = PathBuf::from(&second.worktree);
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join("f.txt"), "work").unwrap();
        assert_ne!(first.worktree, second.worktree, "paths disambiguated");

        s.remove(second.id, None, WorktreeDisposal::DeleteFolder)
            .unwrap();
        assert!(!wt.exists(), "folder deleted despite the name mismatch");

        std::fs::remove_dir_all(&first.worktree).ok();
    }

    /// Wait (bounded) for the background purge to erase every trash sibling.
    fn wait_for_trash_gone(root: &Path) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let left = std::fs::read_dir(root)
                .map(|d| d.flatten().any(|e| is_trash_dir(&e.path())))
                .unwrap_or(false);
            if !left {
                return true;
            }
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// Hard delete moves the folder aside first (the row can drop at once) and
    /// the files disappear shortly after, from a background thread.
    #[test]
    fn remove_hard_moves_folder_aside_then_purges() {
        let s = svc();
        let a = s.spawn(req(Uuid::new_v4(), "aside"), &nogit()).unwrap();
        let wt = PathBuf::from(&a.worktree);
        std::fs::create_dir_all(wt.join("node_modules/pkg")).unwrap();
        std::fs::write(wt.join("node_modules/pkg/index.js"), "x").unwrap();

        s.remove(a.id, None, WorktreeDisposal::DeleteFolder).unwrap();

        assert!(!wt.exists(), "the recorded path is gone on return");
        assert_eq!(s.list().unwrap().len(), 0, "row gone on return");
        assert!(
            wait_for_trash_gone(wt.parent().unwrap()),
            "the trash sibling is purged in the background"
        );
    }

    /// A leftover `*.trash-*` folder (crash mid-purge) is swept on the next launch.
    #[test]
    fn sweep_trash_removes_leftovers() {
        let s = svc();
        let root = s.worktree_root.clone();
        let leftover = trash_path(&root.join("dead"));
        std::fs::create_dir_all(leftover.join("deep")).unwrap();
        std::fs::write(leftover.join("deep/f.txt"), "x").unwrap();
        let keep = root.join("alive");
        std::fs::create_dir_all(&keep).unwrap();

        s.sweep_trash();

        assert!(wait_for_trash_gone(&root), "leftover trash purged");
        assert!(keep.exists(), "a real worktree folder is untouched");
    }

    /// Windows: a handle held open below the folder makes the rename fail, and
    /// the removal must abort with the row intact — the same contract the
    /// in-place delete had, just without the long wait.
    #[cfg(windows)]
    #[test]
    fn remove_hard_locked_folder_aborts() {
        use std::os::windows::fs::OpenOptionsExt;
        let s = svc();
        let a = s.spawn(req(Uuid::new_v4(), "locked"), &nogit()).unwrap();
        let wt = PathBuf::from(&a.worktree);
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join("held.txt"), "x").unwrap();
        // share_mode(0): no other opener may read, write or delete/rename
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(wt.join("held.txt"))
            .unwrap();

        let err = s.remove(a.id, None, WorktreeDisposal::DeleteFolder);
        assert!(err.is_err(), "locked folder aborts the removal");
        assert_eq!(s.list().unwrap().len(), 1, "row survives to retry from");
        assert!(wt.join("held.txt").exists(), "nothing was moved");
        drop(_held);
        std::fs::remove_dir_all(&wt).ok();
    }

    /// A worktree that was never created on disk is not an error to hard-delete.
    #[test]
    fn remove_hard_tolerates_a_missing_folder() {
        let s = svc();
        let a = s.spawn(req(Uuid::new_v4(), "ghost"), &nogit()).unwrap();
        assert!(!PathBuf::from(&a.worktree).exists());
        s.remove(a.id, None, WorktreeDisposal::DeleteFolder).unwrap();
        assert_eq!(s.list().unwrap().len(), 0);
    }

    // ---- branch template expansion ----

    #[test]
    fn branch_template_expands_name_tool_date_tokens() {
        assert_eq!(
            branch_from_template("{name}/{tool}/{date}", "fix_login", "claude", "0611"),
            "fix_login/claude/0611"
        );
    }

    #[test]
    fn branch_template_empty_falls_back_to_agent_name() {
        assert_eq!(
            branch_from_template("", "fix_login", "claude", "0611"),
            "agent/fix_login"
        );
        assert_eq!(
            branch_from_template("   ", "fix_login", "claude", "0611"),
            "agent/fix_login"
        );
    }

    #[test]
    fn branch_template_sanitizes_unsafe_characters() {
        // spaces/!/~ map to '-', '..' collapses, component edges are trimmed
        assert_eq!(
            branch_from_template("feat {name}!", "fix_login", "claude", "0611"),
            "feat-fix_login"
        );
        assert_eq!(
            branch_from_template("{name}..{tool}", "fix_login", "claude", "0611"),
            "fix_login-claude"
        );
        // empty components (leading/trailing/double slashes) are dropped
        assert_eq!(
            branch_from_template("/x//{name}/", "fix_login", "claude", "0611"),
            "x/fix_login"
        );
        // a `.lock` component suffix is invalid for a ref — stripped
        assert_eq!(
            branch_from_template("{name}.lock", "fix_login", "claude", "0611"),
            "fix_login"
        );
    }

    #[test]
    fn branch_template_unsalvageable_falls_back() {
        // expands to nothing but separators/invalid chars → historical shape
        assert_eq!(
            branch_from_template("///", "fix_login", "claude", "0611"),
            "agent/fix_login"
        );
        assert_eq!(
            branch_from_template("!!!", "fix_login", "claude", "0611"),
            "agent/fix_login"
        );
    }

    #[test]
    fn spawn_uses_persisted_branch_template() {
        let (s, settings) = svc_with_settings();
        let mut prefs = crate::settings::Settings::default();
        prefs.branch_template = "{tool}/{name}".into();
        settings.set(&prefs).unwrap();
        let a = s.spawn(req(Uuid::new_v4(), "fix login"), &nogit()).unwrap();
        assert_eq!(a.branch, "claude/fix_login", "template-driven branch");
    }

    #[test]
    fn spawn_expands_date_token_as_mmdd() {
        let (s, settings) = svc_with_settings();
        let mut prefs = crate::settings::Settings::default();
        prefs.branch_template = "agent/{name}/{date}".into();
        settings.set(&prefs).unwrap();
        let a = s.spawn(req(Uuid::new_v4(), "dated"), &nogit()).unwrap();
        let date = a.branch.rsplit('/').next().unwrap();
        assert_eq!(date.len(), 4, "MMDD: {}", a.branch);
        assert!(date.chars().all(|c| c.is_ascii_digit()), "{}", a.branch);
        let mm: u32 = date[..2].parse().unwrap();
        let dd: u32 = date[2..].parse().unwrap();
        assert!((1..=12).contains(&mm), "month in range: {}", a.branch);
        assert!((1..=31).contains(&dd), "day in range: {}", a.branch);
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1), "leap year 2000");
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    // ---- worktree root consumption ----

    #[test]
    fn spawn_uses_configured_absolute_worktree_root() {
        let (s, settings) = svc_with_settings();
        let custom = tempfile::tempdir().unwrap();
        let mut prefs = crate::settings::Settings::default();
        prefs.worktree_root = custom.path().to_string_lossy().to_string();
        settings.set(&prefs).unwrap();
        let a = s.spawn(req(Uuid::new_v4(), "rooted"), &nogit()).unwrap();
        assert!(
            Path::new(&a.worktree).starts_with(custom.path()),
            "worktree under the configured root: {}",
            a.worktree
        );
    }

    #[test]
    fn spawn_falls_back_to_ctor_root_for_relative_or_empty_setting() {
        for bad in ["", "   ", "not/absolute"] {
            let (s, settings) = svc_with_settings();
            let mut prefs = crate::settings::Settings::default();
            prefs.worktree_root = bad.into();
            settings.set(&prefs).unwrap();
            let a = s.spawn(req(Uuid::new_v4(), "fb"), &nogit()).unwrap();
            assert!(
                Path::new(&a.worktree).starts_with(&s.worktree_root),
                "'{bad}' must fall back to the ctor root: {}",
                a.worktree
            );
        }
    }

    // ---- interrupted-agents snapshot ----

    #[test]
    fn running_ids_reports_inflight_before_reset() {
        let s = svc();
        let run = s.spawn(req(Uuid::new_v4(), "run"), &nogit()).unwrap();
        let wait = s.spawn(req(Uuid::new_v4(), "wait"), &nogit()).unwrap();
        let idle = s.spawn(req(Uuid::new_v4(), "idle"), &nogit()).unwrap();
        let upd = |st: &str| AgentUpdateRequest {
            status: Some(st.into()),
            task: None,
            model: None,
            name: None,
        };
        s.update(run.id, upd("running")).unwrap();
        s.update(wait.id, upd("waiting")).unwrap();

        let mut ids = s.running_ids().unwrap();
        ids.sort();
        let mut want = vec![run.id, wait.id];
        want.sort();
        assert_eq!(ids, want, "running + waiting; idle excluded");
        assert!(!ids.contains(&idle.id));

        // the launch sequence: snapshot THEN reset — afterwards nothing is in-flight
        s.reset_running().unwrap();
        assert!(s.running_ids().unwrap().is_empty());
    }

    #[test]
    fn interrupted_agents_drain_is_one_shot() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let state = InterruptedAgents::new(vec![a, b]);
        assert_eq!(state.drain(), vec![a, b], "first drain yields the snapshot");
        assert!(state.drain().is_empty(), "second drain is empty (one-shot)");
    }

    #[test]
    fn remove_for_project_cascades_only_that_project() {
        let s = svc();
        let p1 = Uuid::new_v4();
        let p2 = Uuid::new_v4();
        s.spawn(req(p1, "a"), &nogit()).unwrap();
        s.spawn(req(p1, "b"), &nogit()).unwrap();
        let keep = s.spawn(req(p2, "c"), &nogit()).unwrap();
        let removed = s.remove_for_project(p1).unwrap();
        assert_eq!(removed.len(), 2);
        let left = s.list().unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, keep.id);
    }
}
