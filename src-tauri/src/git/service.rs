use std::path::Path;

use git2::Repository;

use crate::core::errors::{AppError, AppResult};

#[derive(Clone, Default)]
pub struct GitService;

impl GitService {
    pub fn new() -> Self {
        Self
    }

    /// True if `path` is (inside) a git repository.
    pub fn detect(&self, path: &Path) -> bool {
        Repository::open(path).is_ok()
    }

    /// Initialize a new repository at `path`.
    pub fn init(&self, path: &Path) -> AppResult<()> {
        Repository::init(path).map_err(|e| AppError::Other(format!("git init: {e}")))?;
        Ok(())
    }

    /// (branch, short-sha) of HEAD, or None for a repo with no commits / no repo.
    pub fn head_info(&self, path: &Path) -> Option<(String, String)> {
        let repo = Repository::open(path).ok()?;
        let head = repo.head().ok()?;
        let branch = head.shorthand().ok()?.to_string();
        let short = head.target()?.to_string().chars().take(7).collect::<String>();
        Some((branch, short))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_then_init_then_detect() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        assert!(!svc.detect(dir.path()), "fresh dir is not a repo");
        svc.init(dir.path()).unwrap();
        assert!(svc.detect(dir.path()), "after init it is a repo");
    }

    #[test]
    fn head_info_is_none_without_commits() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        assert!(svc.head_info(dir.path()).is_none());
    }
}
