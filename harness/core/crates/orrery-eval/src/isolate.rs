//! One workspace per case, and nothing shared between them.
//!
//! Cases that share state are not cases, they are one long case with an
//! ordering bug, so isolation is not optional and there is no "reuse the
//! checkout" mode. A [`CaseWorkspace`] cleans itself up in `Drop`, which is
//! what makes the cleanup survive a panic in the middle of a case.
//!
//! Phase 9 ships worktree isolation only. [`Isolation::Container`] exists in
//! the enum and returns [`EvalError::Unsupported`] by name, because a case that
//! installs its own dependencies needs a container and pretending a worktree is
//! one would be worse than saying no (plan 16, open question 1).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::case::{EvalCase, WorkspaceSpec};
use crate::error::EvalError;
use crate::matrix::MatrixPoint;
use crate::run::Isolation;

/// Makes a fresh workspace per case.
#[derive(Debug, Clone)]
pub struct Isolator {
    base: PathBuf,
    isolation: Isolation,
    keep: bool,
}

impl Isolator {
    /// Put case workspaces under `base`.
    #[must_use]
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self {
            base: base.into(),
            isolation: Isolation::Worktree,
            keep: false,
        }
    }

    /// Choose the isolation mode.
    #[must_use]
    pub const fn with_isolation(mut self, isolation: Isolation) -> Self {
        self.isolation = isolation;
        self
    }

    /// Leave the workspaces on disk after the run, for looking at a failure.
    #[must_use]
    pub const fn keeping(mut self, keep: bool) -> Self {
        self.keep = keep;
        self
    }

    /// Build the workspace one case will run in.
    ///
    /// # Errors
    ///
    /// [`EvalError::Unsupported`] for container isolation, [`EvalError::Io`]
    /// when the directory cannot be made, [`EvalError::Adapter`] when git
    /// refuses a worktree.
    pub fn prepare(
        &self,
        case: &EvalCase,
        point: &MatrixPoint,
    ) -> Result<CaseWorkspace, EvalError> {
        if self.isolation == Isolation::Container {
            return Err(EvalError::Unsupported {
                what: "container isolation",
            });
        }

        let dir = self.base.join(slug(case, point));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| EvalError::io("clearing", &dir, e))?;
        }

        match &case.workspace {
            WorkspaceSpec::Empty => {
                std::fs::create_dir_all(&dir).map_err(|e| EvalError::io("creating", &dir, e))?;
                Ok(CaseWorkspace {
                    path: dir,
                    repo: None,
                    keep: self.keep,
                })
            }
            WorkspaceSpec::Fixture { archive } => {
                std::fs::create_dir_all(&dir).map_err(|e| EvalError::io("creating", &dir, e))?;
                copy_tree(archive, &dir)?;
                Ok(CaseWorkspace {
                    path: dir,
                    repo: None,
                    keep: self.keep,
                })
            }
            WorkspaceSpec::Repo { repo, commit } => {
                if let Some(parent) = dir.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| EvalError::io("creating", parent, e))?;
                }
                let mut args = vec![
                    "worktree".to_owned(),
                    "add".to_owned(),
                    "--detach".to_owned(),
                    dir.display().to_string(),
                ];
                if let Some(commit) = commit {
                    args.push(commit.clone());
                }
                let out = Command::new("git")
                    .current_dir(repo)
                    .args(&args)
                    .output()
                    .map_err(|e| EvalError::Adapter {
                        adapter: "git".to_owned(),
                        detail: format!("could not run git: {e}"),
                    })?;
                if !out.status.success() {
                    return Err(EvalError::Adapter {
                        adapter: "git".to_owned(),
                        detail: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
                    });
                }
                Ok(CaseWorkspace {
                    path: dir,
                    repo: Some(repo.clone()),
                    keep: self.keep,
                })
            }
        }
    }
}

/// One case's private directory, removed when it is dropped.
#[derive(Debug)]
pub struct CaseWorkspace {
    path: PathBuf,
    repo: Option<PathBuf>,
    keep: bool,
}

impl CaseWorkspace {
    /// Where the case runs.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Keep this one after all.
    pub fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for CaseWorkspace {
    /// Cleanup in `Drop` rather than at the end of the run, so that a case
    /// which panics still takes its workspace with it.
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if let Some(repo) = &self.repo {
            // Best effort: the worktree registration has to go as well, or the
            // next run finds a stale entry in the source repository.
            let _ = Command::new("git")
                .current_dir(repo)
                .args([
                    "worktree",
                    "remove",
                    "--force",
                    &self.path.display().to_string(),
                ])
                .output();
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A directory name that is unique per case and per matrix point, and readable.
fn slug(case: &EvalCase, point: &MatrixPoint) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect()
    };
    let seed = point.seed.map_or_else(|| "n".to_owned(), |s| s.to_string());
    format!(
        "{}-{}-{}-{}-{}",
        clean(&case.id),
        clean(&point.profile),
        clean(&point.model),
        seed,
        uuid::Uuid::new_v4().simple()
    )
}

/// Copy a fixture directory in, recursively.
fn copy_tree(from: &Path, to: &Path) -> Result<(), EvalError> {
    let entries = std::fs::read_dir(from).map_err(|e| EvalError::io("reading", from, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| EvalError::io("reading", from, e))?;
        let target = to.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|e| EvalError::io("reading", entry.path(), e))?;
        if kind.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| EvalError::io("creating", &target, e))?;
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .map_err(|e| EvalError::io("copying", entry.path(), e))?;
        }
    }
    Ok(())
}
