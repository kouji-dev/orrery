//! `GitService` — the handle the rest of the app holds. It is a thin `Arc`
//! around the [`GitBackend`] and derefs to it, so every caller writes
//! `git.status(path)` without naming a library.
//!
//! The backend is gitoxide ([`GixBackend`]); the trait stays as the seam so a
//! second implementation can be dropped in (`from_backend`) and tested by the
//! same `backend_tests!`. libgit2 is no longer linked into the app — it
//! survives only as a test oracle (a dev-dependency). The worktree-folder
//! helpers below are plain filesystem code shared by the agent service and
//! the backend.

use std::path::Path;
use std::sync::Arc;

pub use super::backend::{GitBackend, IgnoreMatcher};
use super::gix_backend::GixBackend;
pub use super::types::*;

/// `remove_dir_all` with a few retries.
///
/// On Windows a directory tree that was open a moment ago routinely refuses the
/// first delete: antivirus, the file indexer, or a just-killed child process can
/// still hold a handle for tens of milliseconds. One retry loop turns almost all
/// of those into a success, and a real failure (locked file, permissions) still
/// surfaces as an error rather than being swallowed.
pub fn remove_dir_all_retry(path: &Path) -> std::io::Result<()> {
    const ATTEMPTS: u32 = 5;
    for attempt in 1..=ATTEMPTS {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            // already gone — that is the outcome we wanted
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) if attempt < ATTEMPTS => {
                std::thread::sleep(std::time::Duration::from_millis(60 * attempt as u64));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Suffix that marks a worktree folder moved aside for background deletion.
pub const TRASH_MARKER: &str = ".trash-";

/// Where a worktree folder goes when it is hard-deleted: a sibling named
/// `<basename>.trash-<6 hex>`. A rename is one metadata operation, so the
/// agent's row can drop the moment it succeeds; the actual file deletion
/// (tens of thousands of `node_modules` entries on a slow NTFS) runs later.
pub fn trash_path(worktree: &Path) -> std::path::PathBuf {
    let base = worktree
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("worktree");
    let tag = &uuid::Uuid::new_v4().to_string()[..6];
    worktree.with_file_name(format!("{base}{TRASH_MARKER}{tag}"))
}

/// True for a folder produced by [`trash_path`] (the startup sweep's filter).
pub fn is_trash_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.contains(TRASH_MARKER))
}

/// `rename` with the same retry loop as [`remove_dir_all_retry`]: a directory
/// with a handle still open somewhere below it refuses to move on Windows, and
/// a just-killed child or the AV scanner routinely holds one for a few tens of
/// milliseconds. A source that is already gone counts as success.
pub fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    const ATTEMPTS: u32 = 5;
    for attempt in 1..=ATTEMPTS {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !from.exists() => return Ok(()),
            Err(_) if attempt < ATTEMPTS => {
                std::thread::sleep(std::time::Duration::from_millis(60 * attempt as u64));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The app's git handle: cheap to clone, shared status cache inside the
/// backend, derefs to [`GitBackend`].
#[derive(Clone)]
pub struct GitService(Arc<dyn GitBackend>);

impl GitService {
    /// The gitoxide backend.
    pub fn new() -> Self {
        Self(Arc::new(GixBackend::new()))
    }

    /// Wrap any backend (tests, or a future second implementation).
    pub fn from_backend(backend: Arc<dyn GitBackend>) -> Self {
        Self(backend)
    }
}

impl Default for GitService {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for GitService {
    type Target = dyn GitBackend;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_dir_all_retry_is_ok_when_the_path_is_already_gone() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("not-there");
        remove_dir_all_retry(&gone).unwrap();

        let real = dir.path().join("real");
        std::fs::create_dir_all(real.join("nested")).unwrap();
        std::fs::write(real.join("nested/f.txt"), "x").unwrap();
        remove_dir_all_retry(&real).unwrap();
        assert!(!real.exists());
    }

    #[test]
    fn trash_path_is_a_marked_sibling() {
        let wt = Path::new("/root/worktrees/my_task");
        let t = trash_path(wt);
        assert_eq!(t.parent(), wt.parent());
        assert!(is_trash_dir(&t));
        assert!(!is_trash_dir(wt));
        assert!(t.file_name().unwrap().to_str().unwrap().starts_with("my_task.trash-"));
    }

    #[test]
    fn rename_retry_moves_and_tolerates_a_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("deep")).unwrap();
        let dst = dir.path().join("dst");
        rename_retry(&src, &dst).unwrap();
        assert!(!src.exists() && dst.join("deep").is_dir());
        rename_retry(&src, &dir.path().join("elsewhere")).unwrap();
    }

    #[test]
    fn service_derefs_to_the_backend() {
        let dir = tempfile::tempdir().unwrap();
        let git = GitService::new();
        assert!(!git.detect(dir.path()));
        git.init(dir.path()).unwrap();
        assert!(git.detect(dir.path()));
    }
}
