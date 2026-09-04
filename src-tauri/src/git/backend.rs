//! The one git seam: every git operation the app performs is a method on
//! [`GitBackend`], and the rest of the backend never names a git library.
//!
//! Why a trait: the app started on libgit2, whose checkout is single-threaded
//! (its parallel-checkout PR never merged) and made creating a worktree for a
//! 10k-file project a 10–30 s wait on Windows. gitoxide (`gix`) checks out,
//! diffs and scans status on every core, so the implementation moved there
//! behind this seam, one operation group at a time, with both implementations
//! passing the same `backend_tests!` until the last group landed. The trait
//! stays: it is what let the two coexist, and what keeps the Tauri commands
//! library-agnostic. libgit2 now exists only as a test oracle.
//!
//! Every method takes a PATH (repository or worktree), never a library handle.
//! Opening a repository is cheap; the one hot loop that would suffer (the
//! gitignore check inside the file tree walk) gets a reusable
//! [`IgnoreMatcher`] instead.
//!
//! Network operations (`clone_repo`, `push`, `fetch`, `pull_ff`) are NOT
//! library calls: they shell out to the system `git` so the OS credential
//! helper handles auth (gix has no push at all). They are default methods
//! here.

use std::path::{Path, PathBuf};

use crate::core::errors::{AppError, AppResult};

use super::types::{
    Blame, BranchInfo, ConflictFile, FileChange, FileDiff, FileHistoryEntry, Hunk, LogEntry,
    MergeSession, RemoteInfo, SessionState, WorktreeDisposal,
};

/// Where a repository keeps its parts — the file watcher's root set. For a
/// linked worktree `gitdir` is `<main>/.git/worktrees/<name>` and `common_dir`
/// is `<main>/.git`; for the main checkout the two coincide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoLayout {
    /// This checkout's private git dir.
    pub gitdir: PathBuf,
    /// The shared git dir (objects, refs) — the main repository's `.git`.
    pub common_dir: PathBuf,
    /// The main repository's working directory (None for a bare repo).
    pub main_workdir: Option<PathBuf>,
    /// Every registered linked worktree: (name, working directory).
    pub worktrees: Vec<(String, PathBuf)>,
}

/// A prepared gitignore oracle for one worktree, reused across a directory
/// walk (thousands of lookups) instead of reopening the repository per entry.
pub trait IgnoreMatcher: Send {
    /// The worktree's root directory (what paths are relative to).
    fn workdir(&self) -> &Path;
    /// Is `rel` (worktree-relative, `/`-separated) ignored by git's rules —
    /// nested `.gitignore`, `.git/info/exclude`, the global excludes file.
    fn is_ignored(&self, rel: &str) -> bool;
}

pub trait GitBackend: Send + Sync {
    // ------------------------------------------------------------ repository

    /// True if `path` is (inside) a git repository.
    fn detect(&self, path: &Path) -> bool;
    /// `git init` at `path`.
    fn init(&self, path: &Path) -> AppResult<()>;
    /// Guarantee the repo has a branch: an unborn HEAD is pointed at `main`
    /// and given an initial commit so worktrees have a base to branch from.
    fn ensure_main_branch(&self, path: &Path) -> AppResult<()>;
    /// Current branch shorthand (`main`), None when detached/unborn/not a repo.
    fn head_branch(&self, path: &Path) -> Option<String>;
    /// (branch shorthand, short sha) of HEAD.
    fn head_info(&self, path: &Path) -> Option<(String, String)>;
    /// Full HEAD oid — the watcher's "did HEAD move" probe.
    fn head_oid(&self, path: &Path) -> Option<String>;
    /// Directory layout of the repository containing `path` (see [`RepoLayout`]).
    fn repo_layout(&self, path: &Path) -> Option<RepoLayout>;
    /// gitignore oracle for the worktree at `path`; None when not a repo.
    fn ignore_matcher(&self, path: &Path) -> Option<Box<dyn IgnoreMatcher>>;

    // -------------------------------------------------------------- branches

    /// Local branch names, sorted.
    fn branches(&self, path: &Path) -> Vec<String>;
    /// `origin/HEAD`'s target, else `main`/`master` if they exist, else HEAD's branch.
    fn default_branch(&self, path: &Path) -> Option<String>;
    /// Local branches with upstream, ahead/behind and which checkout holds them.
    fn branches_detail(&self, repo_path: &Path) -> AppResult<Vec<BranchInfo>>;
    fn remotes(&self, repo_path: &Path) -> AppResult<Vec<RemoteInfo>>;
    /// Create `name` at `from` (a revision) or HEAD.
    fn branch_create(&self, repo_path: &Path, name: &str, from: Option<&str>) -> AppResult<()>;
    /// Rename; refuses while any checkout holds the branch.
    fn branch_rename(&self, repo_path: &Path, old: &str, new: &str) -> AppResult<()>;
    /// Delete; refuses occupied branches and, unless `force`, unmerged ones.
    fn branch_delete(&self, repo_path: &Path, name: &str, force: bool) -> AppResult<()>;
    /// Set (`Some("origin/main")`) or unset (`None`) the upstream.
    fn branch_set_upstream(&self, repo_path: &Path, name: &str, upstream: Option<&str>)
        -> AppResult<()>;
    /// Switch `checkout` to `branch` (safe: dirty conflicts abort); refuses a
    /// branch another checkout holds.
    fn checkout_branch(&self, checkout: &Path, branch: &str) -> AppResult<()>;

    // --------------------------------------------------------------- history

    /// Newest-first page of commits reachable from HEAD, each with its
    /// changed-file count.
    fn log(&self, path: &Path, limit: usize, offset: usize) -> Vec<LogEntry>;
    /// (commit time in epoch seconds, full sha) for a revision — the range
    /// commands sort by it.
    fn commit_time(&self, path: &Path, rev: &str) -> AppResult<(i64, String)>;
    /// Files changed by one commit (vs its first parent).
    fn commit_files(&self, path: &Path, sha: &str) -> AppResult<Vec<FileChange>>;
    /// Old/new text of `rel` at one commit (vs its first parent).
    fn commit_file_diff(&self, path: &Path, sha: &str, rel: &str) -> AppResult<FileDiff>;
    /// Files changed between two revisions' trees.
    fn range_diff(&self, path: &Path, from: &str, to: &str) -> AppResult<Vec<FileChange>>;
    /// Old/new text of `rel` between two revisions.
    fn range_file_diff(&self, path: &Path, from: &str, to: &str, rel: &str)
        -> AppResult<FileDiff>;
    /// Commits that touched `rel`, newest first, paged.
    fn file_history(
        &self,
        path: &Path,
        rel: &str,
        limit: usize,
        offset: usize,
    ) -> AppResult<Vec<FileHistoryEntry>>;
    /// Blame of `rel` at `rev` (HEAD when None).
    fn blame(&self, path: &Path, rel: &str, rev: Option<&str>) -> AppResult<Blame>;
    /// (HEAD blame, working-copy blame with uncommitted lines marked).
    fn working_blame(&self, path: &Path, rel: &str) -> AppResult<(Blame, Blame)>;

    // ---------------------------------------------------------- working tree

    /// Working-tree changes vs HEAD with per-file line counts (cached per repo).
    fn status(&self, path: &Path) -> Vec<FileChange>;
    /// Same set without line counts (`add`/`del` = 0) — the cheap background scan.
    fn status_counts_only(&self, path: &Path) -> Vec<FileChange>;
    /// HEAD content vs working file; `old_rel` is the pre-move path of a rename.
    fn file_diff(&self, worktree: &Path, rel: &str, old_rel: Option<&str>) -> FileDiff;
    /// Exact (0-context) hunks of the working file vs HEAD.
    fn file_hunks(&self, worktree: &Path, rel: &str) -> AppResult<Vec<Hunk>>;
    /// Reverse-apply the hunk starting at `new_start` byte-exactly.
    fn revert_hunk(&self, worktree: &Path, rel: &str, new_start: u32) -> AppResult<()>;
    /// Stage `paths` (everything when empty) and commit; returns the short sha.
    fn commit(&self, worktree: &Path, message: &str, paths: &[String]) -> AppResult<String>;
    /// Reset `paths` (everything when empty) to HEAD; untracked ones are removed.
    fn discard(&self, worktree: &Path, paths: &[String]) -> AppResult<()>;

    // ------------------------------------------------------------- worktrees

    /// Create `branch` from `base` (or HEAD) and check it out as a linked
    /// worktree named `wt_name` at `wt_path`.
    fn create_worktree(
        &self,
        repo_path: &Path,
        wt_name: &str,
        branch: &str,
        base: Option<&str>,
        wt_path: &Path,
    ) -> AppResult<()>;
    /// Deregister a linked worktree (its folder's fate is `disposal`). An
    /// unknown name is not an error.
    fn remove_worktree(
        &self,
        repo_path: &Path,
        wt_name: &str,
        disposal: WorktreeDisposal,
    ) -> AppResult<()>;

    // --------------------------------------------------------- merge session

    /// Merge `branch` into the checkout at `repo_path`; a conflicted result
    /// leaves the session open (MERGE_HEAD, staged conflicts) for the UI.
    fn merge(&self, repo_path: &Path, branch: &str) -> AppResult<MergeSession>;
    /// The conflicted paths with their base/ours/theirs and working text.
    fn conflict_files(&self, repo_path: &Path) -> AppResult<Vec<ConflictFile>>;
    /// Write the resolved content for `rel` and stage it.
    fn conflict_resolve(&self, repo_path: &Path, rel: &str, content: &str) -> AppResult<()>;
    /// Abandon the session: hard reset to HEAD.
    fn merge_abort(&self, repo_path: &Path) -> AppResult<()>;
    /// Commit the merge once every conflict is resolved; returns the short sha.
    fn merge_continue(&self, repo_path: &Path, message: Option<&str>) -> AppResult<String>;
    /// Is a merge in progress, and how many conflicts remain.
    fn session_state(&self, repo_path: &Path) -> AppResult<SessionState>;

    // ------------------------------------------------- network (system git)

    /// Clone `url` into `target` (`depth` = shallow). CLI so the credential
    /// helper can authenticate private remotes.
    fn clone_repo(&self, url: &str, target: &Path, depth: Option<u32>) -> AppResult<()> {
        let mut cmd = crate::core::proc::cmd("git");
        cmd.arg("clone");
        if let Some(d) = depth {
            cmd.arg("--depth").arg(d.to_string());
        }
        cmd.arg(url).arg(target);
        run_git(cmd, "clone")
    }

    /// `git push -u <remote> <branch>` from `worktree`.
    fn push(&self, worktree: &Path, remote: &str, branch: &str) -> AppResult<()> {
        let mut cmd = crate::core::proc::cmd("git");
        cmd.current_dir(worktree).args(["push", "-u", remote, branch]);
        run_git(cmd, "push")
    }

    /// Fetch (with prune) one remote, or every remote when `remote` is None.
    fn fetch(&self, repo_path: &Path, remote: Option<&str>) -> AppResult<()> {
        let mut cmd = crate::core::proc::cmd("git");
        cmd.current_dir(repo_path);
        match remote {
            Some(r) => cmd.args(["fetch", "--prune", r]),
            None => cmd.args(["fetch", "--all", "--prune"]),
        };
        run_git(cmd, "fetch")
    }

    /// Fast-forward pull in `checkout`. A diverged branch errors with git's
    /// own message — the UI directs the user to "Merge in".
    fn pull_ff(&self, checkout: &Path) -> AppResult<()> {
        let mut cmd = crate::core::proc::cmd("git");
        cmd.current_dir(checkout).args(["pull", "--ff-only"]);
        run_git(cmd, "pull")
    }
}

/// Run a prepared `git` command; a non-zero exit surfaces git's stderr.
fn run_git(mut cmd: std::process::Command, what: &str) -> AppResult<()> {
    let out = cmd
        .output()
        .map_err(|e| AppError::Other(format!("git {what}: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "git {what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}
