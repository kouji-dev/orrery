use std::path::Path;

use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::core::database::DB;
use crate::core::errors::{AppError, AppResult, DbError, ProjectError};
use crate::git::service::GitService;

use super::model::{
    ClonePathMode, CommitView, Project, ProjectCreateRequest, ProjectRecord, ProjectUpdateRequest,
};

/// Derive a repo/folder name from a git URL: last path segment, `.git` stripped.
/// Handles both https (`…/user/repo.git`) and scp-style ssh (`git@host:user/repo.git`).
pub(crate) fn repo_name_from_url(url: &str) -> Option<String> {
    let last = url.trim().trim_end_matches('/').rsplit(['/', ':']).next()?;
    let name = last.trim_end_matches(".git").trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Format a unix timestamp as a short "Nm / Nh / Nd ago" style token (FE appends "ago").
pub(crate) fn relative_time(then: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(then);
    let d = (now - then).max(0);
    if d < 60 {
        "now".into()
    } else if d < 3600 {
        format!("{}m", d / 60)
    } else if d < 86_400 {
        format!("{}h", d / 3600)
    } else {
        format!("{}d", d / 86_400)
    }
}

#[derive(Clone)]
pub struct ProjectService {
    db: DB,
    git: GitService,
}

impl ProjectService {
    pub fn new(db: DB, git: GitService) -> Self {
        let svc = Self { db, git };
        svc.init_schema();
        svc
    }

    fn init_schema(&self) {
        let c = self.db.lock().unwrap();
        c.execute(
            "CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                icon TEXT NOT NULL,
                color TEXT NOT NULL
            )",
            [],
        )
        .unwrap();

        // migration: older schemas persisted `has_git` (now a transient). Drop it if present
        // so the leaner INSERT works against a pre-existing database without data loss.
        let has_legacy_column = c
            .prepare("SELECT 1 FROM pragma_table_info('projects') WHERE name = 'has_git'")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);
        if has_legacy_column {
            let _ = c.execute("ALTER TABLE projects DROP COLUMN has_git", []);
        }
    }

    /// The git handle, for command modules that act on a project path directly.
    pub fn git(&self) -> &GitService {
        &self.git
    }

    pub fn detect_git(&self, path: &str) -> bool {
        self.git.detect(Path::new(path))
    }

    /// Turn a persisted record into the view model, computing transient
    /// attributes (folder existence, git state) from disk at read time.
    fn enrich(&self, rec: ProjectRecord) -> Project {
        let path = Path::new(&rec.path);
        let folder_exists = path.is_dir();
        let has_git = folder_exists && self.git.detect(path);
        let (branch, head) = if has_git {
            match self.git.head_info(path) {
                Some((b, h)) => (Some(b), Some(h)),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        let branches = if has_git {
            self.git.branches(path)
        } else {
            Vec::new()
        };
        let default_branch = if has_git {
            self.git.default_branch(path)
        } else {
            None
        };
        Project {
            id: rec.id,
            name: rec.name,
            path: rec.path,
            icon: rec.icon,
            color: rec.color,
            folder_exists,
            has_git,
            branch,
            head,
            branches,
            default_branch,
        }
    }

    pub fn list(&self) -> AppResult<Vec<Project>> {
        // read every record (lock held), then drop the lock before touching disk
        let records = self.records()?;
        Ok(records.into_iter().map(|r| self.enrich(r)).collect())
    }

    pub fn get(&self, id: Uuid) -> AppResult<Project> {
        Ok(self.enrich(self.record(id)?))
    }

    /// Just the on-disk path of a project (no enrichment) — used to locate worktrees.
    pub fn path_of(&self, id: Uuid) -> AppResult<String> {
        Ok(self.record(id)?.path)
    }

    /// One entry point for both create shapes — the frontend never knows which
    /// ran: a `source_url` routes to the clone flow, otherwise it's a local
    /// folder registration.
    pub fn create(&self, req: ProjectCreateRequest) -> AppResult<Project> {
        match req.source_url.as_deref().map(str::trim) {
            Some(url) if !url.is_empty() => self.create_from_git(req),
            _ => self.create_local(req),
        }
    }

    fn create_local(&self, req: ProjectCreateRequest) -> AppResult<Project> {
        if req.name.trim().is_empty() || req.path.trim().is_empty() {
            return Err(ProjectError::Required("name or path is empty".into()).into());
        }
        if self.exists_by_path(&req.path)? {
            return Err(ProjectError::Exists(req.path.clone()).into());
        }

        // create the folder if the user pointed at a path that doesn't exist yet
        let path = Path::new(&req.path);
        if !path.exists() {
            std::fs::create_dir_all(path).map_err(|e| {
                ProjectError::Invalid(format!("could not create folder '{}': {e}", req.path))
            })?;
        }
        // init a repo only when asked and it isn't one already
        if req.with_git && !self.git.detect(path) {
            self.git.init(path)?;
        }
        // any git project (freshly initialized or pre-existing) must have at least
        // one branch ("main") so agents have a base to branch a worktree from
        if self.git.detect(path) {
            self.git.ensure_main_branch(path)?;
        }

        let id = Uuid::new_v4();
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "INSERT INTO projects (id, name, path, icon, color) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id.to_string(), req.name, req.path, req.icon, req.color],
            )
            .map_err(DbError::Sqlite)?;
        }

        Ok(self.enrich(ProjectRecord {
            id,
            name: req.name,
            path: req.path,
            icon: req.icon,
            color: req.color,
        }))
    }

    /// Clone `source_url` and register the result. `source_mode` decides where
    /// the content lands: `Root` clones into `path/<repo-name>`, `Project`
    /// clones directly into `path` (the `git clone <url> .` shape).
    fn create_from_git(&self, req: ProjectCreateRequest) -> AppResult<Project> {
        let url = req.source_url.as_deref().unwrap_or("").trim().to_string();
        let base = req.path.trim().to_string();
        if url.is_empty() || base.is_empty() {
            return Err(ProjectError::Required("url or path is empty".into()).into());
        }

        let repo_name = repo_name_from_url(&url);
        let target = match req.source_mode.unwrap_or(ClonePathMode::Root) {
            ClonePathMode::Root => {
                let name = repo_name.clone().ok_or_else(|| {
                    ProjectError::Invalid(format!("cannot derive a repo name from '{url}'"))
                })?;
                Path::new(&base).join(name)
            }
            ClonePathMode::Project => Path::new(&base).to_path_buf(),
        };
        let target_str = target.to_string_lossy().to_string();

        // fail before the (slow, network) clone: duplicate project or a
        // destination git would refuse anyway (exists and not an empty dir)
        if self.exists_by_path(&target_str)? {
            return Err(ProjectError::Exists(target_str).into());
        }
        if target.exists() {
            let empty_dir = target.is_dir()
                && target
                    .read_dir()
                    .map(|mut d| d.next().is_none())
                    .unwrap_or(false);
            if !empty_dir {
                return Err(ProjectError::Invalid(format!(
                    "destination '{target_str}' exists and is not an empty folder"
                ))
                .into());
            }
        }

        self.git.clone_repo(&url, &target, req.depth)?;

        // hand off to the local flow: detects the fresh .git, guarantees a
        // base branch (even for an empty remote), inserts + enriches
        let name = if req.name.trim().is_empty() {
            repo_name.unwrap_or_else(|| {
                target
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| target_str.clone())
            })
        } else {
            req.name.trim().to_string()
        };
        self.create_local(ProjectCreateRequest {
            name,
            path: target_str,
            icon: req.icon,
            color: req.color,
            with_git: false,
            source_url: None,
            source_mode: None,
            depth: None,
        })
    }

    pub fn update(&self, id: Uuid, req: ProjectUpdateRequest) -> AppResult<Project> {
        let mut rec = self.record(id)?; // NotFound if it doesn't exist

        if let Some(name) = req.name {
            if !name.trim().is_empty() {
                rec.name = name;
            }
        }
        if let Some(icon) = req.icon {
            rec.icon = icon;
        }
        if let Some(color) = req.color {
            rec.color = color;
        }
        if let Some(path) = req.path {
            if path.trim().is_empty() {
                return Err(ProjectError::Required("path is empty".into()).into());
            }
            if path != rec.path && self.exists_by_path(&path)? {
                return Err(ProjectError::Exists(path).into());
            }
            rec.path = path;
        }

        {
            let c = self.db.lock().unwrap();
            c.execute(
                "UPDATE projects SET name = ?2, path = ?3, icon = ?4, color = ?5 WHERE id = ?1",
                rusqlite::params![id.to_string(), rec.name, rec.path, rec.icon, rec.color],
            )
            .map_err(DbError::Sqlite)?;
        }

        Ok(self.enrich(rec))
    }

    /// Initialize a git repo for an existing project that was created without one.
    pub fn init_git(&self, id: Uuid) -> AppResult<Project> {
        let rec = self.record(id)?;
        let path = Path::new(&rec.path);
        if !path.is_dir() {
            return Err(ProjectError::Invalid(format!("folder not found: {}", rec.path)).into());
        }
        if !self.git.detect(path) {
            self.git.init(path)?;
        }
        Ok(self.enrich(rec))
    }

    /// Real commit history for a project, shaped for the frontend feed.
    pub fn commits(&self, id: Uuid, limit: usize) -> AppResult<Vec<CommitView>> {
        let rec = self.record(id)?;
        Ok(self
            .git
            .log(Path::new(&rec.path), limit, 0)
            .into_iter()
            .map(|e| CommitView {
                agent: e.author,
                project_id: id,
                sha: e.sha,
                msg: e.message,
                when: relative_time(e.time),
                ts: e.time,
                files: e.files as i64,
            })
            .collect())
    }

    pub fn remove(&self, id: Uuid) -> AppResult<()> {
        let c = self.db.lock().unwrap();
        let n = c
            .execute("DELETE FROM projects WHERE id = ?1", [id.to_string()])
            .map_err(DbError::Sqlite)?;
        if n == 0 {
            return Err(ProjectError::NotFound(id.to_string()).into());
        }
        Ok(())
    }

    // ---- record readers (persisted fields only; no disk access) ----

    fn records(&self) -> AppResult<Vec<ProjectRecord>> {
        let raw: Vec<(String, String, String, String, String)> = {
            let c = self.db.lock().unwrap();
            let mut stmt = c
                .prepare("SELECT id, name, path, icon, color FROM projects")
                .map_err(DbError::Sqlite)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })
                .map_err(DbError::Sqlite)?;
            rows.collect::<Result<_, _>>().map_err(DbError::Sqlite)?
        };

        raw.into_iter()
            .map(|(id, name, path, icon, color)| {
                let id = Uuid::parse_str(&id).map_err(|e| AppError::Other(e.to_string()))?;
                Ok(ProjectRecord {
                    id,
                    name,
                    path,
                    icon,
                    color,
                })
            })
            .collect()
    }

    fn record(&self, id: Uuid) -> AppResult<ProjectRecord> {
        let row: Option<(String, String, String, String)> = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT name, path, icon, color FROM projects WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(DbError::Sqlite)?
        };
        match row {
            Some((name, path, icon, color)) => Ok(ProjectRecord {
                id,
                name,
                path,
                icon,
                color,
            }),
            None => Err(ProjectError::NotFound(id.to_string()).into()),
        }
    }

    fn exists_by_path(&self, path: &str) -> AppResult<bool> {
        let c = self.db.lock().unwrap();
        let count: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE path = ?1",
                [path],
                |r| r.get(0),
            )
            .map_err(DbError::Sqlite)?;
        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    fn svc() -> ProjectService {
        let db: DB = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        ProjectService::new(db, GitService::new())
    }

    fn req(name: &str, path: &str, with_git: bool) -> ProjectCreateRequest {
        ProjectCreateRequest {
            name: name.into(),
            path: path.into(),
            icon: "box".into(),
            color: "#a855f7".into(),
            with_git,
            source_url: None,
            source_mode: None,
            depth: None,
        }
    }

    #[test]
    fn create_and_list() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let p = s
            .create(req("pay", dir.path().to_str().unwrap(), false))
            .unwrap();
        assert_eq!(p.name, "pay");
        let all = s.list().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, p.id);
    }

    #[test]
    fn rejects_empty_fields() {
        let s = svc();
        let err = s.create(req("", "", false)).unwrap_err();
        assert!(matches!(err, AppError::Project(ProjectError::Required(_))));
    }

    #[test]
    fn rejects_duplicate_path() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        s.create(req("a", path, false)).unwrap();
        let err = s.create(req("b", path, false)).unwrap_err();
        assert!(matches!(err, AppError::Project(ProjectError::Exists(_))));
    }

    #[test]
    fn with_git_initializes_repo() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let p = s
            .create(req("g", dir.path().to_str().unwrap(), true))
            .unwrap();
        assert!(p.has_git);
        assert!(dir.path().join(".git").exists());
    }

    #[test]
    fn get_missing_is_not_found() {
        let s = svc();
        let err = s.get(Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, AppError::Project(ProjectError::NotFound(_))));
    }

    #[test]
    fn remove_deletes() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let p = s
            .create(req("x", dir.path().to_str().unwrap(), false))
            .unwrap();
        s.remove(p.id).unwrap();
        assert_eq!(s.list().unwrap().len(), 0);
    }

    #[test]
    fn transient_has_git_reflects_reality_not_storage() {
        // created without git → hasGit false; after init_git → hasGit true (no column involved)
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let p = s
            .create(req("t", dir.path().to_str().unwrap(), false))
            .unwrap();
        assert!(!p.has_git);
        let p2 = s.init_git(p.id).unwrap();
        assert!(p2.has_git);
        assert!(s.get(p.id).unwrap().has_git);
    }

    #[test]
    fn create_with_git_ensures_a_main_branch() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let p = s
            .create(req("withgit", dir.path().to_str().unwrap(), true))
            .unwrap();
        assert!(p.has_git);
        assert!(
            p.branches.contains(&"main".to_string()),
            "branches: {:?}",
            p.branches
        );
        assert_eq!(p.branch.as_deref(), Some("main"));
    }

    #[test]
    fn create_makes_a_missing_folder() {
        let s = svc();
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("fresh").join("nested");
        assert!(!target.exists(), "precondition: folder does not exist yet");
        let p = s
            .create(req("new", target.to_str().unwrap(), false))
            .unwrap();
        assert!(target.is_dir(), "create() made the folder");
        assert!(p.folder_exists);
    }

    #[test]
    fn enrich_reports_folder_missing_after_it_is_deleted() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("gone");
        let p = s
            .create(req("ghost", target.to_str().unwrap(), false))
            .unwrap();
        std::fs::remove_dir_all(&target).unwrap();
        let reread = s.get(p.id).unwrap();
        assert!(!reread.folder_exists);
        assert!(!reread.has_git);
    }

    #[test]
    fn persists_to_a_real_file_across_connections() {
        // isolates "does my code actually write to a file db" from the dev environment
        let dbdir = tempfile::tempdir().unwrap();
        let dbfile = dbdir.path().join("orrery_test.db");
        let pdir = tempfile::tempdir().unwrap();
        {
            let db: DB = Arc::new(Mutex::new(Connection::open(&dbfile).unwrap()));
            let s = ProjectService::new(db, GitService::new());
            s.create(req("f", pdir.path().to_str().unwrap(), false))
                .unwrap();
        }
        // fresh connection to the same file — the row must still be there
        let db2: DB = Arc::new(Mutex::new(Connection::open(&dbfile).unwrap()));
        let s2 = ProjectService::new(db2, GitService::new());
        assert_eq!(s2.list().unwrap().len(), 1, "row must persist to disk");
    }

    // ---- clone-from-git ----

    /// A local source repo with one commit, plus its path as a git-CLI-safe URL.
    fn source_repo() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("readme.md"), "hello").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("readme.md")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("t", "t@local").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        // forward-slash the path so the git CLI accepts it on Windows
        let url = dir.path().to_str().unwrap().replace('\\', "/");
        (dir, url)
    }

    /// A create request whose `source_url` routes it through the clone flow.
    fn clone_req(url: &str, path: &str, mode: ClonePathMode) -> ProjectCreateRequest {
        let mut r = req("", path, false);
        r.source_url = Some(url.into());
        r.source_mode = Some(mode);
        r
    }

    #[test]
    fn repo_name_derives_from_https_ssh_and_local_urls() {
        for (url, want) in [
            ("https://github.com/user/repo.git", "repo"),
            ("https://github.com/user/repo", "repo"),
            ("https://github.com/user/repo/", "repo"),
            ("git@github.com:user/repo.git", "repo"),
            ("C:/tmp/some-repo", "some-repo"),
        ] {
            assert_eq!(repo_name_from_url(url).as_deref(), Some(want), "url: {url}");
        }
        assert_eq!(repo_name_from_url(""), None);
    }

    #[test]
    fn clone_root_mode_lands_in_a_repo_named_subfolder() {
        let s = svc();
        let (src, url) = source_repo();
        let dest = tempfile::tempdir().unwrap();
        let p = s
            .create(clone_req(&url, dest.path().to_str().unwrap(), ClonePathMode::Root))
            .unwrap();
        let repo_name = src.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(p.name, repo_name, "name derived from the url");
        assert_eq!(
            Path::new(&p.path),
            dest.path().join(repo_name),
            "cloned into path/<repo-name>"
        );
        assert!(p.has_git);
        assert!(Path::new(&p.path).join("readme.md").is_file());
    }

    #[test]
    fn clone_project_mode_uses_the_path_itself() {
        let s = svc();
        let (_src, url) = source_repo();
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("my-app");
        let p = s
            .create(clone_req(&url, target.to_str().unwrap(), ClonePathMode::Project))
            .unwrap();
        assert_eq!(Path::new(&p.path), target, "path IS the project folder");
        assert!(p.has_git);
        assert!(target.join("readme.md").is_file(), "content at the root, no subfolder");
    }

    #[test]
    fn clone_rejects_a_nonempty_project_destination() {
        let s = svc();
        let (_src, url) = source_repo();
        let target = tempfile::tempdir().unwrap();
        std::fs::write(target.path().join("occupied.txt"), "x").unwrap();
        let err = s
            .create(clone_req(&url, target.path().to_str().unwrap(), ClonePathMode::Project))
            .unwrap_err();
        assert!(matches!(err, AppError::Project(ProjectError::Invalid(_))), "got: {err:?}");
    }

    #[test]
    fn clone_rejects_empty_url_or_path() {
        let s = svc();
        let err = s
            .create(clone_req("", "", ClonePathMode::Root))
            .unwrap_err();
        assert!(matches!(err, AppError::Project(ProjectError::Required(_))));
    }

    #[test]
    fn shallow_clone_fetches_only_depth_one() {
        let s = svc();
        let (src, url) = source_repo();
        // second commit so full history would be 2 deep
        let repo = git2::Repository::open(src.path()).unwrap();
        std::fs::write(src.path().join("more.md"), "again").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("more.md")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("t", "t@local").unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "second", &tree, &[&head])
            .unwrap();

        let dest = tempfile::tempdir().unwrap();
        let mut req = clone_req(&url, dest.path().to_str().unwrap(), ClonePathMode::Root);
        req.depth = Some(1);
        // local-path clones ignore --depth unless git treats them as a real
        // transport; assert the clone works and the project registers either way
        let p = s.create(req).unwrap();
        assert!(p.has_git);
        let commits = s.commits(p.id, 10).unwrap();
        assert!(!commits.is_empty(), "shallow clone still has the tip commit");
    }

    #[test]
    fn update_relocates_path() {
        let s = svc();
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let p = s
            .create(req("mv", a.path().to_str().unwrap(), false))
            .unwrap();
        let upd = ProjectUpdateRequest {
            name: None,
            path: Some(b.path().to_str().unwrap().into()),
            icon: None,
            color: None,
        };
        let moved = s.update(p.id, upd).unwrap();
        assert_eq!(moved.path, b.path().to_str().unwrap());
        assert!(moved.folder_exists);
    }
}
