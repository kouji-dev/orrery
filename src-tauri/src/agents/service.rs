use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::core::database::DB;
use crate::core::errors::{AgentError, AppError, AppResult, DbError};
use crate::git::service::{FileChange, GitService};
use crate::projects::model::CommitView;

use super::model::{Agent, AgentRecord, AgentSpawnRequest, AgentUpdateRequest};

pub struct AgentService {
    db: DB,
    git: GitService,
    worktree_root: PathBuf,
}

impl AgentService {
    pub fn new(db: DB, git: GitService, worktree_root: PathBuf) -> Self {
        let svc = Self { db, git, worktree_root };
        svc.init_schema();
        svc
    }

    /// Filesystem-safe worktree name from the agent's (unique-per-project) name.
    fn worktree_name(name: &str, id: &Uuid) -> String {
        let mut slug = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
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
        let _ = c.execute("ALTER TABLE agents ADD COLUMN started INTEGER NOT NULL DEFAULT 0", []);
        // migrate DBs created before the `session_id` column existed (ignored if present)
        let _ = c.execute("ALTER TABLE agents ADD COLUMN session_id TEXT", []);
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
            commits: 0,
            elapsed: 0,
            progress: 0.0,
            pending: Vec::new(),
            block_reason: None,
            wait_reason: None,
        }
    }

    pub fn list(&self) -> AppResult<Vec<Agent>> {
        Ok(self.records()?.into_iter().map(|r| self.enrich(r)).collect())
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
        if self.name_exists_in_project(req.project_id, &req.name)? {
            return Err(AgentError::Invalid(format!(
                "an agent named '{}' already exists in this project",
                req.name
            ))
            .into());
        }
        let id = Uuid::new_v4();
        let wt_name = Self::worktree_name(&req.name, &id);
        let branch = format!("agent/{wt_name}");
        // flat: worktrees/<name>. Only disambiguate on a real cross-project name clash.
        let mut wt_path = self.worktree_root.join(&wt_name);
        if wt_path.exists() {
            wt_path = self.worktree_root.join(format!("{}-{}", wt_name, &id.to_string()[..6]));
        }

        // best-effort: only real git projects get a worktree
        if self.git.detect(project_path) {
            if let Err(e) =
                self.git.create_worktree(project_path, &wt_name, &branch, Some(&req.base), &wt_path)
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
        };
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "INSERT INTO agents
                    (id, project_id, tool, model, effort, name, task, status, branch, worktree, base, started)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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

    /// Commit selected paths (or all when empty) in the agent's worktree.
    pub fn commit(&self, id: Uuid, message: &str, paths: &[String]) -> AppResult<String> {
        let rec = self.record(id)?;
        self.git.commit(Path::new(&rec.worktree), message, paths)
    }

    /// Commits on the agent's branch — read from its worktree (whose HEAD *is*
    /// the agent branch), newest first, each tagged with the **agent id** (not the
    /// commit author) so the UI can group commits by agent. Empty for a worktree
    /// with no commits / no repo.
    pub fn commits(&self, id: Uuid, limit: usize) -> AppResult<Vec<CommitView>> {
        let rec = self.record(id)?;
        Ok(self
            .git
            .log(Path::new(&rec.worktree), limit)
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
        self.git.push(Path::new(&rec.worktree), "origin", &rec.branch)
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

    pub fn remove(&self, id: Uuid, project_path: Option<&Path>) -> AppResult<()> {
        // best-effort: tear down the worktree before dropping the row
        if let Some(pp) = project_path {
            if let Ok(rec) = self.record(id) {
                if let Some(wt_name) =
                    Path::new(&rec.worktree).file_name().and_then(|n| n.to_str())
                {
                    let _ = self.git.remove_worktree(pp, wt_name);
                }
            }
        }
        let c = self.db.lock().unwrap();
        let n = c
            .execute("DELETE FROM agents WHERE id = ?1", [id.to_string()])
            .map_err(DbError::Sqlite)?;
        if n == 0 {
            return Err(AgentError::NotFound(id.to_string()).into());
        }
        Ok(())
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
        c.execute("UPDATE agents SET started = 1 WHERE id = ?1", [id.to_string()])
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
        let raw: Vec<(String, String, String, String, Option<String>, String, String, String, String, String, String, bool, Option<String>)> = {
            let c = self.db.lock().unwrap();
            let mut stmt = c
                .prepare(
                    "SELECT id, project_id, tool, model, effort, name, task, status, branch, worktree, base, started, session_id FROM agents",
                )
                .map_err(DbError::Sqlite)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                        r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?,
                    ))
                })
                .map_err(DbError::Sqlite)?;
            rows.collect::<Result<_, _>>().map_err(DbError::Sqlite)?
        };

        raw.into_iter()
            .map(|(id, project_id, tool, model, effort, name, task, status, branch, worktree, base, started, session_id)| {
                Ok(AgentRecord {
                    id: Uuid::parse_str(&id).map_err(|e| AppError::Other(e.to_string()))?,
                    project_id: Uuid::parse_str(&project_id).map_err(|e| AppError::Other(e.to_string()))?,
                    tool, model, effort, name, task, status, branch, worktree, base, started, session_id,
                })
            })
            .collect()
    }

    fn record(&self, id: Uuid) -> AppResult<AgentRecord> {
        let row: Option<(String, String, String, Option<String>, String, String, String, String, String, String, bool, Option<String>)> = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT project_id, tool, model, effort, name, task, status, branch, worktree, base, started, session_id FROM agents WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?)),
            )
            .optional()
            .map_err(DbError::Sqlite)?
        };
        match row {
            Some((project_id, tool, model, effort, name, task, status, branch, worktree, base, started, session_id)) => Ok(AgentRecord {
                id,
                project_id: Uuid::parse_str(&project_id).map_err(|e| AppError::Other(e.to_string()))?,
                tool, model, effort, name, task, status, branch, worktree, base, started, session_id,
            }),
            None => Err(AgentError::NotFound(id.to_string()).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    // a unique, persistent worktree root per service (tests don't clean it — temp dir)
    fn svc() -> AgentService {
        let db: DB = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        let wt_root = std::env::temp_dir().join(format!("orrery-wt-{}", Uuid::new_v4()));
        AgentService::new(db, GitService::new(), wt_root)
    }

    // a non-git path so spawn skips real worktree creation
    fn nogit() -> PathBuf {
        std::env::temp_dir()
    }

    fn req(project_id: Uuid, name: &str) -> AgentSpawnRequest {
        AgentSpawnRequest {
            project_id,
            tool: "claude".into(),
            model: "opus".into(),
            effort: None,
            name: name.into(),
            task: "do the thing".into(),
            base: "main".into(),
        }
    }

    #[test]
    fn spawn_derives_branch_and_worktree_from_name() {
        let s = svc();
        let pid = Uuid::new_v4();
        let a = s.spawn(req(pid, "fix login"), &nogit()).unwrap();
        assert_eq!(a.project_id, pid);
        assert_eq!(a.branch, "agent/fix_login", "branch from snake_case(name)");
        assert!(a.worktree.replace('\\', "/").ends_with("/fix_login"), "flat worktree named after agent: {}", a.worktree);
        assert_eq!(a.status, "idle");
    }

    #[test]
    fn spawn_rejects_empty_name() {
        let s = svc();
        let err = s.spawn(req(Uuid::new_v4(), ""), &nogit()).unwrap_err();
        assert!(matches!(err, AppError::Agent(AgentError::Required(_))));
    }

    #[test]
    fn spawn_rejects_duplicate_name_in_project() {
        let s = svc();
        let pid = Uuid::new_v4();
        s.spawn(req(pid, "dup"), &nogit()).unwrap();
        let err = s.spawn(req(pid, "dup"), &nogit()).unwrap_err();
        assert!(matches!(err, AppError::Agent(AgentError::Invalid(_))));
        // same name in a different project is fine
        s.spawn(req(Uuid::new_v4(), "dup"), &nogit()).unwrap();
    }

    #[test]
    fn spawn_creates_real_worktree_for_git_project() {
        let s = svc();
        let proj = tempfile::tempdir().unwrap();
        GitService::new().init(proj.path()).unwrap(); // empty repo — spawn makes the initial commit
        let a = s.spawn(req(Uuid::new_v4(), "wt"), proj.path()).unwrap();
        assert!(Path::new(&a.worktree).exists(), "real worktree at {}", a.worktree);
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

        let commits = s.commits(a.id, 50).unwrap();
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
        let upd = AgentUpdateRequest { status: Some("running".into()), task: None, model: None, name: None };
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
        assert_eq!(a.session_id, None, "a fresh agent has no session id (migration default NULL)");
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
        let upd = |st: &str| AgentUpdateRequest { status: Some(st.into()), task: None, model: None, name: None };
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
        s.remove(a.id, None).unwrap();
        assert_eq!(s.list().unwrap().len(), 0);
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
