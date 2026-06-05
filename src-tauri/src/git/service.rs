use std::path::Path;

use git2::Repository;

use crate::core::errors::{AppError, AppResult};

/// One entry from a repository's history (raw — the command layer formats it).
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub sha: String,
    pub message: String,
    pub author: String,
    pub time: i64,
    pub files: usize,
}

/// A changed file in the working tree (vs HEAD), shaped for the frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub add: i64,
    pub del: i64,
    pub state: String, // "A" | "M" | "D"
}

/// Old (HEAD) vs new (working-tree) content of a file, for a diff view.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub old: String,
    pub new: String,
    pub lang: String,
}

fn lang_from_path(rel: &str) -> &'static str {
    match Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("") {
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "json" => "json",
        "css" | "scss" | "less" => "css",
        "html" | "htm" => "html",
        "md" | "markdown" => "markdown",
        "rs" => "rust",
        "py" => "python",
        _ => "",
    }
}

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

    /// Most recent commits (newest first), or empty for a repo with no commits / no repo.
    pub fn log(&self, path: &Path, limit: usize) -> Vec<LogEntry> {
        let Ok(repo) = Repository::open(path) else {
            return Vec::new();
        };
        let Ok(mut walk) = repo.revwalk() else {
            return Vec::new();
        };
        if walk.push_head().is_err() {
            return Vec::new(); // no HEAD (no commits yet)
        }
        walk.flatten()
            .take(limit)
            .filter_map(|oid| repo.find_commit(oid).ok())
            .map(|commit| {
                let files = commit
                    .tree()
                    .ok()
                    .map(|tree| {
                        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
                        repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)
                            .ok()
                            .and_then(|d| d.stats().ok())
                            .map(|s| s.files_changed())
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                LogEntry {
                    sha: commit.id().to_string().chars().take(7).collect(),
                    message: commit
                        .message()
                        .unwrap_or("")
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string(),
                    author: commit.author().name().unwrap_or("unknown").to_string(),
                    time: commit.time().seconds(),
                    files,
                }
            })
            .collect()
    }

    /// Create a git worktree for an agent: ensure the repo has a commit, create
    /// `branch` from `base` (or HEAD), and check it out at `wt_path`.
    pub fn create_worktree(
        &self,
        repo_path: &Path,
        wt_name: &str,
        branch: &str,
        base: Option<&str>,
        wt_path: &Path,
    ) -> AppResult<()> {
        let repo =
            Repository::open(repo_path).map_err(|e| AppError::Other(format!("open repo: {e}")))?;
        Self::ensure_commit(&repo)?;

        let base_commit =
            Self::base_commit(&repo, base).map_err(|e| AppError::Other(format!("base: {e}")))?;

        let reference = match repo.find_branch(branch, git2::BranchType::Local) {
            Ok(b) => b.into_reference(),
            Err(_) => repo
                .branch(branch, &base_commit, false)
                .map_err(|e| AppError::Other(format!("branch: {e}")))?
                .into_reference(),
        };

        if let Some(parent) = wt_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(wt_name, wt_path, Some(&opts))
            .map_err(|e| AppError::Other(format!("worktree add: {e}")))?;
        Ok(())
    }

    /// Remove a worktree's working directory and prune its metadata. Best-effort.
    pub fn remove_worktree(&self, repo_path: &Path, wt_name: &str) -> AppResult<()> {
        let repo =
            Repository::open(repo_path).map_err(|e| AppError::Other(format!("open repo: {e}")))?;
        if let Ok(wt) = repo.find_worktree(wt_name) {
            let _ = std::fs::remove_dir_all(wt.path());
            let mut opts = git2::WorktreePruneOptions::new();
            opts.valid(true).working_tree(true);
            let _ = wt.prune(Some(&mut opts));
        }
        Ok(())
    }

    fn ensure_commit(repo: &Repository) -> AppResult<()> {
        if repo.head().is_ok() {
            return Ok(()); // already has commits
        }
        let sig = repo
            .signature()
            .or_else(|_| git2::Signature::now("katrix", "katrix@local"))
            .map_err(|e| AppError::Other(e.to_string()))?;
        let tree_oid = {
            let mut index = repo.index().map_err(|e| AppError::Other(e.to_string()))?;
            index.write_tree().map_err(|e| AppError::Other(e.to_string()))?
        };
        let tree = repo
            .find_tree(tree_oid)
            .map_err(|e| AppError::Other(e.to_string()))?;
        repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
            .map_err(|e| AppError::Other(e.to_string()))?;
        Ok(())
    }

    /// Guarantee the repo has at least one branch. If HEAD is unborn (freshly
    /// `git init`-ed, no commits), point it at `main` and make the initial
    /// commit so agents have a base to branch a worktree from. A repo that
    /// already has commits keeps whatever branch it's on.
    pub fn ensure_main_branch(&self, path: &Path) -> AppResult<()> {
        let repo =
            Repository::open(path).map_err(|e| AppError::Other(format!("open repo: {e}")))?;
        if repo.head().is_ok() {
            return Ok(()); // already has a commit → already has a branch
        }
        repo.set_head("refs/heads/main")
            .map_err(|e| AppError::Other(format!("set head main: {e}")))?;
        Self::ensure_commit(&repo)
    }

    fn base_commit<'a>(
        repo: &'a Repository,
        base: Option<&str>,
    ) -> Result<git2::Commit<'a>, git2::Error> {
        let head_oid = || {
            repo.head()?
                .target()
                .ok_or_else(|| git2::Error::from_str("no head"))
        };
        let oid = match base {
            Some(b) if !b.is_empty() => match repo.revparse_single(b) {
                Ok(o) => o.id(),
                Err(_) => head_oid()?,
            },
            _ => head_oid()?,
        };
        repo.find_commit(oid)
    }

    /// Stage the given paths (or everything when empty) and commit. Returns the short sha.
    pub fn commit(&self, worktree: &Path, message: &str, paths: &[String]) -> AppResult<String> {
        let repo = Repository::open(worktree).map_err(|e| AppError::Other(format!("open: {e}")))?;
        Self::ensure_commit(&repo)?;
        let mut index = repo.index().map_err(|e| AppError::Other(e.to_string()))?;
        let specs: Vec<&str> = if paths.is_empty() {
            vec!["*"]
        } else {
            paths.iter().map(|s| s.as_str()).collect()
        };
        index
            .add_all(specs.iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| AppError::Other(e.to_string()))?;
        index.write().map_err(|e| AppError::Other(e.to_string()))?;
        let tree = repo
            .find_tree(index.write_tree().map_err(|e| AppError::Other(e.to_string()))?)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let sig = repo
            .signature()
            .or_else(|_| git2::Signature::now("katrix", "katrix@local"))
            .map_err(|e| AppError::Other(e.to_string()))?;
        let parent = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| AppError::Other(e.to_string()))?;
        let msg = if message.trim().is_empty() { "wip" } else { message };
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, msg, &tree, &[&parent])
            .map_err(|e| AppError::Other(e.to_string()))?;
        Ok(oid.to_string().chars().take(7).collect())
    }

    /// Discard working-tree changes for the given paths (or all when empty):
    /// reset tracked to HEAD + remove untracked.
    pub fn discard(&self, worktree: &Path, paths: &[String]) -> AppResult<()> {
        let repo = Repository::open(worktree).map_err(|e| AppError::Other(format!("open: {e}")))?;
        let mut co = git2::build::CheckoutBuilder::new();
        co.force().remove_untracked(true);
        for p in paths {
            co.path(p.as_str());
        }
        repo.checkout_head(Some(&mut co))
            .map_err(|e| AppError::Other(e.to_string()))?;
        Ok(())
    }

    /// Merge `branch` into the repo's current HEAD (fast-forward when possible).
    pub fn merge(&self, repo_path: &Path, branch: &str) -> AppResult<()> {
        let repo =
            Repository::open(repo_path).map_err(|e| AppError::Other(format!("open: {e}")))?;
        let their = repo
            .find_branch(branch, git2::BranchType::Local)
            .map_err(|_| AppError::Other(format!("branch '{branch}' not found")))?
            .into_reference()
            .peel_to_commit()
            .map_err(|e| AppError::Other(e.to_string()))?;
        let annotated = repo
            .find_annotated_commit(their.id())
            .map_err(|e| AppError::Other(e.to_string()))?;
        let (analysis, _) = repo
            .merge_analysis(&[&annotated])
            .map_err(|e| AppError::Other(e.to_string()))?;

        if analysis.is_up_to_date() {
            return Ok(());
        }
        let head_name = repo
            .head()
            .map_err(|e| AppError::Other(e.to_string()))?
            .name()
            .unwrap_or("HEAD")
            .to_string();

        if analysis.is_fast_forward() {
            let mut head_ref = repo
                .find_reference(&head_name)
                .map_err(|e| AppError::Other(e.to_string()))?;
            head_ref
                .set_target(their.id(), "fast-forward merge")
                .map_err(|e| AppError::Other(e.to_string()))?;
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            repo.checkout_head(Some(&mut co))
                .map_err(|e| AppError::Other(e.to_string()))?;
            return Ok(());
        }

        // true merge → create a merge commit (bail on conflicts)
        repo.merge(&[&annotated], None, None)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let mut index = repo.index().map_err(|e| AppError::Other(e.to_string()))?;
        if index.has_conflicts() {
            let _ = repo.cleanup_state();
            return Err(AppError::Other("merge conflicts — resolve manually".into()));
        }
        let tree = repo
            .find_tree(index.write_tree().map_err(|e| AppError::Other(e.to_string()))?)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let sig = repo
            .signature()
            .or_else(|_| git2::Signature::now("katrix", "katrix@local"))
            .map_err(|e| AppError::Other(e.to_string()))?;
        let head_commit = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| AppError::Other(e.to_string()))?;
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &format!("merge {branch}"),
            &tree,
            &[&head_commit, &their],
        )
        .map_err(|e| AppError::Other(e.to_string()))?;
        let _ = repo.cleanup_state();
        Ok(())
    }

    /// Old (HEAD) vs new (working tree) content for a single file, for the diff view.
    pub fn file_diff(&self, worktree: &Path, rel: &str) -> FileDiff {
        let old = Repository::open(worktree)
            .ok()
            .and_then(|repo| {
                let tree = repo.head().ok()?.peel_to_tree().ok()?;
                let entry = tree.get_path(Path::new(rel)).ok()?;
                let blob = repo.find_blob(entry.id()).ok()?;
                Some(String::from_utf8_lossy(blob.content()).to_string())
            })
            .unwrap_or_default();
        let new = std::fs::read_to_string(worktree.join(rel)).unwrap_or_default();
        FileDiff { old, new, lang: lang_from_path(rel).to_string() }
    }

    /// Working-tree changes vs HEAD (staged + unstaged + untracked), with line counts.
    pub fn status(&self, path: &Path) -> Vec<FileChange> {
        use std::cell::RefCell;
        use std::collections::BTreeMap;

        let Ok(repo) = Repository::open(path) else {
            return Vec::new();
        };
        let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let mut opts = git2::DiffOptions::new();
        opts.include_untracked(true).recurse_untracked_dirs(true);
        let diff = match repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts)) {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };

        let path_of = |d: &git2::DiffDelta| {
            d.new_file()
                .path()
                .or_else(|| d.old_file().path())
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default()
        };
        // (add, del, state) per path; RefCell so both diff callbacks can mutate it
        let acc: RefCell<BTreeMap<String, (i64, i64, char)>> = RefCell::new(BTreeMap::new());
        let _ = diff.foreach(
            &mut |delta, _| {
                let st = match delta.status() {
                    git2::Delta::Added | git2::Delta::Untracked | git2::Delta::Copied => 'A',
                    git2::Delta::Deleted => 'D',
                    _ => 'M',
                };
                acc.borrow_mut().entry(path_of(&delta)).or_insert((0, 0, st)).2 = st;
                true
            },
            None,
            None,
            Some(&mut |delta, _hunk, line| {
                let mut m = acc.borrow_mut();
                let e = m.entry(path_of(&delta)).or_insert((0, 0, 'M'));
                match line.origin() {
                    '+' => e.0 += 1,
                    '-' => e.1 += 1,
                    _ => {}
                }
                true
            }),
        );

        acc.into_inner()
            .into_iter()
            .map(|(path, (add, del, state))| FileChange { path, add, del, state: state.to_string() })
            .collect()
    }

    /// Local branch names, or empty for a non-repo.
    pub fn branches(&self, path: &Path) -> Vec<String> {
        let Ok(repo) = Repository::open(path) else {
            return Vec::new();
        };
        let Ok(branches) = repo.branches(Some(git2::BranchType::Local)) else {
            return Vec::new();
        };
        branches
            .flatten()
            .filter_map(|(b, _)| b.name().ok().flatten().map(|s| s.to_string()))
            .collect()
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

    fn commit_file(repo_path: &Path, name: &str, msg: &str) {
        use git2::Signature;
        std::fs::write(repo_path.join(name), "x").unwrap();
        let repo = Repository::open(repo_path).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("Test", "t@t").unwrap();
        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .and_then(|oid| repo.find_commit(oid).ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents).unwrap();
    }

    #[test]
    fn log_is_empty_without_commits() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        assert!(svc.log(dir.path(), 10).is_empty());
    }

    #[test]
    fn commit_stages_all_and_records() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        std::fs::write(dir.path().join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.path().join("new.txt"), "n\n").unwrap();
        svc.commit(dir.path(), "wip changes", &[]).unwrap();
        assert!(svc.status(dir.path()).is_empty(), "clean after commit");
        assert_eq!(svc.log(dir.path(), 10)[0].message, "wip changes");
    }

    #[test]
    fn commit_only_selected_paths() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        std::fs::write(dir.path().join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "new\n").unwrap();
        svc.commit(dir.path(), "only a", &["a.txt".to_string()]).unwrap();
        let st = svc.status(dir.path());
        assert!(!st.iter().any(|c| c.path == "a.txt"), "a.txt committed: {st:?}");
        assert!(st.iter().any(|c| c.path == "b.txt"), "b.txt still pending: {st:?}");
    }

    #[test]
    fn discard_resets_and_cleans() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first"); // committed content = "x"
        std::fs::write(dir.path().join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.path().join("untracked.txt"), "x\n").unwrap();
        svc.discard(dir.path(), &[]).unwrap();
        assert!(svc.status(dir.path()).is_empty(), "clean after discard");
        assert!(!dir.path().join("untracked.txt").exists(), "untracked removed");
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "x");
    }

    #[test]
    fn merge_fast_forward() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = Repository::open(dir.path()).unwrap();
        let main = repo.head().unwrap().shorthand().unwrap().to_string();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &base, false).unwrap();

        let ff = |to: &str| {
            repo.set_head(to).unwrap();
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            repo.checkout_head(Some(&mut co)).unwrap();
        };
        ff("refs/heads/feature");
        commit_file(dir.path(), "b.txt", "second");
        let feat_tip = repo.head().unwrap().peel_to_commit().unwrap().id();
        ff(&format!("refs/heads/{main}"));

        svc.merge(dir.path(), "feature").unwrap();
        let tip = Repository::open(dir.path()).unwrap().head().unwrap().peel_to_commit().unwrap().id();
        assert_eq!(tip, feat_tip, "main fast-forwarded to feature");
        assert!(dir.path().join("b.txt").exists());
    }

    #[test]
    fn file_diff_reports_old_and_new() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.ts", "first"); // committed content = "x"
        std::fs::write(dir.path().join("a.ts"), "const y = 2;\n").unwrap();
        let d = svc.file_diff(dir.path(), "a.ts");
        assert_eq!(d.old, "x");
        assert_eq!(d.new, "const y = 2;\n");
        assert_eq!(d.lang, "javascript");
    }

    #[test]
    fn status_reports_modified_and_added() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        std::fs::write(dir.path().join("a.txt"), "x\ny\n").unwrap(); // modify
        std::fs::write(dir.path().join("new.txt"), "n\n").unwrap(); // add (untracked)
        let st = svc.status(dir.path());
        assert!(st.iter().any(|c| c.path == "a.txt" && c.state == "M"), "{st:?}");
        assert!(st.iter().any(|c| c.path == "new.txt" && c.state == "A"), "{st:?}");
    }

    #[test]
    fn lists_local_branches() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = Repository::open(dir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &head, false).unwrap();
        let bs = svc.branches(dir.path());
        assert!(bs.contains(&"feature".to_string()), "branches: {bs:?}");
        assert!(bs.len() >= 2, "default branch + feature: {bs:?}");
    }

    #[test]
    fn create_and_remove_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");

        let wt_root = tempfile::tempdir().unwrap();
        let wt_path = wt_root.path().join("my_task");
        svc.create_worktree(dir.path(), "my_task", "agent/my_task", None, &wt_path)
            .unwrap();

        assert!(wt_path.join("a.txt").exists(), "worktree checked out the files");
        let repo = Repository::open(dir.path()).unwrap();
        assert!(repo.find_branch("agent/my_task", git2::BranchType::Local).is_ok());

        svc.remove_worktree(dir.path(), "my_task").unwrap();
        assert!(!wt_path.exists(), "worktree dir removed");
    }

    #[test]
    fn worktree_on_empty_repo_creates_initial_commit() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        assert!(Repository::open(dir.path()).unwrap().head().is_err());

        let wt_root = tempfile::tempdir().unwrap();
        let wt_path = wt_root.path().join("t");
        svc.create_worktree(dir.path(), "t", "agent/t", None, &wt_path)
            .unwrap();

        assert!(wt_path.exists());
        assert!(Repository::open(dir.path()).unwrap().head().is_ok(), "now has a HEAD");
    }

    #[test]
    fn ensure_main_branch_creates_main_in_empty_repo() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        assert!(Repository::open(dir.path()).unwrap().head().is_err(), "starts unborn");

        svc.ensure_main_branch(dir.path()).unwrap();

        let repo = Repository::open(dir.path()).unwrap();
        assert_eq!(repo.head().unwrap().shorthand().unwrap(), "main");
        assert!(svc.branches(dir.path()).contains(&"main".to_string()));
        svc.ensure_main_branch(dir.path()).unwrap(); // idempotent on a repo with commits
    }

    #[test]
    fn log_returns_commits_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        commit_file(dir.path(), "b.txt", "second");
        let log = svc.log(dir.path(), 10);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].message, "second");
        assert_eq!(log[1].message, "first");
        assert!(log[0].files >= 1);
    }
}
