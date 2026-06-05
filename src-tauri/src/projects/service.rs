use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use crate::core::database::DB;
use crate::core::errors::{AppError, AppResult, DbError, ProjectError};
use crate::git::service::GitService;

use super::model::{Project, ProjectCreateRequest};

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
                color TEXT NOT NULL,
                has_git INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();
    }

    pub fn detect_git(&self, path: &str) -> bool {
        self.git.detect(Path::new(path))
    }

    pub fn list(&self) -> AppResult<Vec<Project>> {
        // collect raw rows, then drop the lock before touching the filesystem (git)
        let raw: Vec<(String, String, String, String, String, i64)> = {
            let c = self.db.lock().unwrap();
            let mut stmt = c
                .prepare("SELECT id, name, path, icon, color, has_git FROM projects")
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
                    ))
                })
                .map_err(DbError::Sqlite)?;
            rows.collect::<Result<_, _>>().map_err(DbError::Sqlite)?
        };

        raw.into_iter()
            .map(|(id, name, path, icon, color, has_git)| {
                let id = Uuid::parse_str(&id).map_err(|e| AppError::Other(e.to_string()))?;
                let has_git = has_git != 0;
                let (branch, head) = self.head(&path, has_git);
                Ok(Project { id, name, path, icon, color, has_git, branch, head })
            })
            .collect()
    }

    pub fn get(&self, id: Uuid) -> AppResult<Project> {
        let row: Option<(String, String, String, String, i64)> = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT name, path, icon, color, has_git FROM projects WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(DbError::Sqlite)?
        };
        match row {
            Some((name, path, icon, color, has_git)) => {
                let has_git = has_git != 0;
                let (branch, head) = self.head(&path, has_git);
                Ok(Project { id, name, path, icon, color, has_git, branch, head })
            }
            None => Err(ProjectError::NotFound(id.to_string()).into()),
        }
    }

    pub fn create(&self, req: ProjectCreateRequest) -> AppResult<Project> {
        if req.name.trim().is_empty() || req.path.trim().is_empty() {
            return Err(ProjectError::Required("name or path is empty".into()).into());
        }
        if self.exists_by_path(&req.path)? {
            return Err(ProjectError::Exists(req.path.clone()).into());
        }

        let path = Path::new(&req.path);
        let detected = self.git.detect(path);
        if req.with_git && !detected {
            self.git.init(path)?;
        }
        let has_git = req.with_git || detected;
        let (branch, head) = self.head(&req.path, has_git);

        let id = Uuid::new_v4();
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "INSERT INTO projects (id, name, path, icon, color, has_git) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    id.to_string(),
                    req.name,
                    req.path,
                    req.icon,
                    req.color,
                    has_git as i64
                ],
            )
            .map_err(DbError::Sqlite)?;
        }

        Ok(Project {
            id,
            name: req.name,
            path: req.path,
            icon: req.icon,
            color: req.color,
            has_git,
            branch,
            head,
        })
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

    fn head(&self, path: &str, has_git: bool) -> (Option<String>, Option<String>) {
        if !has_git {
            return (None, None);
        }
        match self.git.head_info(Path::new(path)) {
            Some((b, h)) => (Some(b), Some(h)),
            None => (None, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
    }

    #[test]
    fn create_and_list() {
        let s = svc();
        let dir = tempfile::tempdir().unwrap();
        let p = s.create(req("pay", dir.path().to_str().unwrap(), false)).unwrap();
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
        let p = s.create(req("g", dir.path().to_str().unwrap(), true)).unwrap();
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
        let p = s.create(req("x", dir.path().to_str().unwrap(), false)).unwrap();
        s.remove(p.id).unwrap();
        assert_eq!(s.list().unwrap().len(), 0);
    }
}
