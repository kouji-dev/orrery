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
    pub state: String, // "A" | "M" | "D" | "R" (renamed/moved)
    /// For "R" (renamed/moved): the pre-move path. None otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
}

/// Old (HEAD) vs new (working-tree) content of a file, for a diff view.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub old: String,
    pub new: String,
    pub lang: String,
}

/// One commit in a file's history (returned by `file_history`).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistoryEntry {
    pub sha: String,
    pub author: String,
    pub email: String,
    pub when: i64,
    pub summary: String,
    pub add: i64,
    pub del: i64,
}

/// One line in a blame result.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameLine {
    pub n: usize,
    pub sha: String,
    pub author: String,
    pub when: i64,
    pub summary: String,
    pub line: String,
}

fn lang_from_path(rel: &str) -> &'static str {
    match Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
    {
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "json" => "json",
        "css" | "scss" | "less" => "css",
        "html" | "htm" => "html",
        "md" | "markdown" => "markdown",
        "rs" => "rust",
        "py" => "python",
        "java" => "java",
        "yaml" | "yml" => "yaml",
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

    /// Most recent commits (newest first), `offset`-skipped for paging, or empty
    /// for a repo with no commits / no repo.
    pub fn log(&self, path: &Path, limit: usize, offset: usize) -> Vec<LogEntry> {
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
            .skip(offset)
            .take(limit)
            .filter_map(|oid| repo.find_commit(oid).ok())
            .map(|commit| {
                // delta count only — .stats() would load + line-diff every changed
                // blob per commit just to expose the same files-changed number
                let files = commit
                    .tree()
                    .ok()
                    .map(|tree| {
                        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
                        repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)
                            .ok()
                            .map(|d| d.deltas().len())
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
            .or_else(|_| git2::Signature::now("orrery", "orrery@local"))
            .map_err(|e| AppError::Other(e.to_string()))?;
        let tree_oid = {
            let mut index = repo.index().map_err(|e| AppError::Other(e.to_string()))?;
            index
                .write_tree()
                .map_err(|e| AppError::Other(e.to_string()))?
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
            .find_tree(
                index
                    .write_tree()
                    .map_err(|e| AppError::Other(e.to_string()))?,
            )
            .map_err(|e| AppError::Other(e.to_string()))?;
        let sig = repo
            .signature()
            .or_else(|_| git2::Signature::now("orrery", "orrery@local"))
            .map_err(|e| AppError::Other(e.to_string()))?;
        let parent = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| AppError::Other(e.to_string()))?;
        let msg = if message.trim().is_empty() {
            "wip"
        } else {
            message
        };
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
            .find_tree(
                index
                    .write_tree()
                    .map_err(|e| AppError::Other(e.to_string()))?,
            )
            .map_err(|e| AppError::Other(e.to_string()))?;
        let sig = repo
            .signature()
            .or_else(|_| git2::Signature::now("orrery", "orrery@local"))
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
    /// Old (HEAD) vs new (working-tree) content of a file. For a renamed/moved
    /// file, pass `old_rel` = the pre-move path so the OLD side reads from where
    /// the content used to live (otherwise a rename looks like a fresh add).
    pub fn file_diff(&self, worktree: &Path, rel: &str, old_rel: Option<&str>) -> FileDiff {
        let old_path = old_rel.unwrap_or(rel);
        let old = Repository::open(worktree)
            .ok()
            .and_then(|repo| {
                let tree = repo.head().ok()?.peel_to_tree().ok()?;
                let entry = tree.get_path(Path::new(old_path)).ok()?;
                let blob = repo.find_blob(entry.id()).ok()?;
                Some(String::from_utf8_lossy(blob.content()).to_string())
            })
            .unwrap_or_default();
        let new = std::fs::read_to_string(worktree.join(rel)).unwrap_or_default();
        FileDiff {
            old,
            new,
            lang: lang_from_path(rel).to_string(),
        }
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
        // `show_untracked_content` is what makes the line-level diff callback fire
        // for untracked files — without it a brand-new N-line file reports +0/-0
        // because git2 only emits its delta header, never its lines.
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true);
        let mut diff =
            match repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts)) {
                Ok(d) => d,
                Err(_) => return Vec::new(),
            };
        // Coalesce a delete + add of similar content into ONE "renamed" delta
        // (incl. untracked new files), so a move shows as a single R entry — not
        // a separate Deleted + Added pair.
        let mut find = git2::DiffFindOptions::new();
        find.renames(true).for_untracked(true);
        let _ = diff.find_similar(Some(&mut find));

        let new_path_of = |d: &git2::DiffDelta| {
            d.new_file()
                .path()
                .or_else(|| d.old_file().path())
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default()
        };
        // (add, del, state, old_path) per NEW path; RefCell so both callbacks can mutate it
        let acc: RefCell<BTreeMap<String, (i64, i64, char, Option<String>)>> =
            RefCell::new(BTreeMap::new());
        let _ = diff.foreach(
            &mut |delta, _| {
                let st = match delta.status() {
                    git2::Delta::Added | git2::Delta::Untracked | git2::Delta::Copied => 'A',
                    git2::Delta::Deleted => 'D',
                    git2::Delta::Renamed => 'R',
                    _ => 'M',
                };
                let mut m = acc.borrow_mut();
                let e = m.entry(new_path_of(&delta)).or_insert((0, 0, st, None));
                e.2 = st;
                if st == 'R' {
                    e.3 = delta
                        .old_file()
                        .path()
                        .map(|p| p.to_string_lossy().replace('\\', "/"));
                }
                true
            },
            None,
            None,
            Some(&mut |delta, _hunk, line| {
                let mut m = acc.borrow_mut();
                let e = m.entry(new_path_of(&delta)).or_insert((0, 0, 'M', None));
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
            .map(|(path, (add, del, state, old_path))| FileChange {
                path,
                add,
                del,
                state: state.to_string(),
                old_path,
            })
            .collect()
    }

    /// Diff any two tree-ish (commit trees) and return the same `FileChange`
    /// shape the frontend already consumes. Rename detection is enabled.
    ///
    /// `from` is the "old" tree (e.g. parent commit tree); `to` is the "new"
    /// tree (e.g. the commit tree). Pass `None` for either side to diff against
    /// an empty tree (useful for the very first commit).
    fn diff_trees(
        &self,
        repo: &git2::Repository,
        from: Option<&git2::Tree>,
        to: Option<&git2::Tree>,
    ) -> Vec<FileChange> {
        let mut diff = match repo.diff_tree_to_tree(from, to, None) {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };

        // Enable rename detection (same as status())
        let mut find = git2::DiffFindOptions::new();
        find.renames(true);
        let _ = diff.find_similar(Some(&mut find));

        let mut results: Vec<FileChange> = Vec::new();

        for idx in 0..diff.deltas().len() {
            let delta = diff.get_delta(idx).expect("delta index in range");
            let state = match delta.status() {
                git2::Delta::Added | git2::Delta::Copied => 'A',
                git2::Delta::Deleted => 'D',
                git2::Delta::Renamed => 'R',
                _ => 'M',
            };
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let old_path = if state == 'R' {
                delta
                    .old_file()
                    .path()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
            } else {
                None
            };

            // Count added/deleted lines via Patch for this specific delta
            let (add, del) = git2::Patch::from_diff(&diff, idx)
                .ok()
                .flatten()
                .map(|patch| {
                    let stats = patch.line_stats().unwrap_or((0, 0, 0));
                    // line_stats() → (context, additions, deletions)
                    (stats.1 as i64, stats.2 as i64)
                })
                .unwrap_or((0, 0));

            results.push(FileChange {
                path,
                add,
                del,
                state: state.to_string(),
                old_path,
            });
        }

        results
    }

    /// Diff two commit trees within a known repository and return per-file
    /// `FileChange` entries — the same shape the frontend already consumes.
    ///
    /// This is the foundation for per-commit diff, range diff, and file
    /// history. `from` is the parent/older tree; `to` is the newer tree.
    ///
    /// Requires `repo` because `git2::Tree` does not expose its owning
    /// repository. Callers that already have the repo open pass it directly;
    /// Tauri commands that work from a `&Path` can open the repo first.
    pub fn diff_treeish(
        &self,
        repo: &git2::Repository,
        from: git2::Tree,
        to: git2::Tree,
    ) -> Vec<FileChange> {
        self.diff_trees(repo, Some(&from), Some(&to))
    }

    // ── new git-inspection helpers ─────────────────────────────────────────

    /// Parse a SHA string (full or short) into an Oid, returning a descriptive
    /// error if the string is malformed or the object doesn't exist.
    fn resolve_oid(repo: &git2::Repository, sha: &str) -> AppResult<git2::Oid> {
        repo.revparse_single(sha)
            .map(|o| o.id())
            .map_err(|e| AppError::Other(format!("cannot resolve '{sha}': {e}")))
    }

    /// Per-file changes introduced by a single commit: diff against first parent
    /// (or empty tree for the root commit). Exported for Tauri commands.
    pub fn commit_files(
        &self,
        repo: &git2::Repository,
        sha: &str,
    ) -> AppResult<Vec<FileChange>> {
        let oid = Self::resolve_oid(repo, sha)?;
        let commit = repo
            .find_commit(oid)
            .map_err(|e| AppError::Other(format!("find commit: {e}")))?;
        let to_tree = commit
            .tree()
            .map_err(|e| AppError::Other(format!("commit tree: {e}")))?;
        let from_tree = commit
            .parent(0)
            .ok()
            .and_then(|p| p.tree().ok());
        Ok(self.diff_trees(repo, from_tree.as_ref(), Some(&to_tree)))
    }

    /// Build hunks from a `git2::Diff` for a single `path`.
    /// Returns (old_content, new_content) pair as strings + the hunk vec.
    /// This is a shared helper extracted from the old `file_diff` logic so both
    /// the working-tree and commit diff paths can reuse it.
    fn hunks_from_diff(
        diff: &git2::Diff,
        path: &str,
    ) -> AppResult<(String, String)> {
        // Find the delta index for `path`
        let idx = (0..diff.deltas().len())
            .find(|&i| {
                let d = diff.get_delta(i).unwrap();
                let new_match = d
                    .new_file()
                    .path()
                    .map(|p| p.to_string_lossy().replace('\\', "/") == path)
                    .unwrap_or(false);
                let old_match = d
                    .old_file()
                    .path()
                    .map(|p| p.to_string_lossy().replace('\\', "/") == path)
                    .unwrap_or(false);
                new_match || old_match
            })
            .ok_or_else(|| AppError::Other(format!("path '{path}' not in diff")))?;

        let patch = git2::Patch::from_diff(diff, idx)
            .map_err(|e| AppError::Other(format!("patch: {e}")))?
            .ok_or_else(|| AppError::Other(format!("no patch for '{path}'")))?;

        // Reconstruct old/new from the patch hunks
        let mut old = String::new();
        let mut new = String::new();
        for h in 0..patch.num_hunks() {
            let (_, line_count) = patch
                .hunk(h)
                .map_err(|e| AppError::Other(format!("hunk: {e}")))?;
            for l in 0..line_count {
                let line = patch
                    .line_in_hunk(h, l)
                    .map_err(|e| AppError::Other(format!("line: {e}")))?;
                let text = String::from_utf8_lossy(line.content()).to_string();
                match line.origin() {
                    '-' => old.push_str(&text),
                    '+' => new.push_str(&text),
                    ' ' => {
                        old.push_str(&text);
                        new.push_str(&text);
                    }
                    _ => {}
                }
            }
        }
        Ok((old, new))
    }

    /// Old/new content of `path` between the first-parent and the commit at
    /// `sha`. Returns the same `FileDiff` shape as `file_diff`.
    pub fn commit_file_diff(
        &self,
        repo: &git2::Repository,
        sha: &str,
        path: &str,
    ) -> AppResult<FileDiff> {
        let oid = Self::resolve_oid(repo, sha)?;
        let commit = repo
            .find_commit(oid)
            .map_err(|e| AppError::Other(format!("find commit: {e}")))?;
        let to_tree = commit
            .tree()
            .map_err(|e| AppError::Other(format!("commit tree: {e}")))?;
        let from_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let diff = repo
            .diff_tree_to_tree(from_tree.as_ref(), Some(&to_tree), None)
            .map_err(|e| AppError::Other(format!("diff: {e}")))?;
        let (old, new) = Self::hunks_from_diff(&diff, path)?;
        Ok(FileDiff {
            old,
            new,
            lang: lang_from_path(path).to_string(),
        })
    }

    /// Diff from `from_sha`'s tree to `to_sha`'s tree: files changed in that range.
    pub fn range_diff(
        &self,
        repo: &git2::Repository,
        from_sha: &str,
        to_sha: &str,
    ) -> AppResult<Vec<FileChange>> {
        let from_oid = Self::resolve_oid(repo, from_sha)?;
        let to_oid = Self::resolve_oid(repo, to_sha)?;
        let from_tree = repo
            .find_commit(from_oid)
            .map_err(|e| AppError::Other(format!("from commit: {e}")))?
            .tree()
            .map_err(|e| AppError::Other(format!("from tree: {e}")))?;
        let to_tree = repo
            .find_commit(to_oid)
            .map_err(|e| AppError::Other(format!("to commit: {e}")))?
            .tree()
            .map_err(|e| AppError::Other(format!("to tree: {e}")))?;
        Ok(self.diff_trees(repo, Some(&from_tree), Some(&to_tree)))
    }

    /// Old/new content of `path` between `from_sha`'s tree and `to_sha`'s tree.
    pub fn range_file_diff(
        &self,
        repo: &git2::Repository,
        from_sha: &str,
        to_sha: &str,
        path: &str,
    ) -> AppResult<FileDiff> {
        let from_oid = Self::resolve_oid(repo, from_sha)?;
        let to_oid = Self::resolve_oid(repo, to_sha)?;
        let from_tree = repo
            .find_commit(from_oid)
            .map_err(|e| AppError::Other(format!("from commit: {e}")))?
            .tree()
            .map_err(|e| AppError::Other(format!("from tree: {e}")))?;
        let to_tree = repo
            .find_commit(to_oid)
            .map_err(|e| AppError::Other(format!("to commit: {e}")))?
            .tree()
            .map_err(|e| AppError::Other(format!("to tree: {e}")))?;
        let diff = repo
            .diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)
            .map_err(|e| AppError::Other(format!("diff: {e}")))?;
        let (old, new) = Self::hunks_from_diff(&diff, path)?;
        Ok(FileDiff {
            old,
            new,
            lang: lang_from_path(path).to_string(),
        })
    }

    /// Commits that touch `path` (revwalk from HEAD, topo+time order), paged.
    pub fn file_history(
        &self,
        repo: &git2::Repository,
        path: &str,
        limit: usize,
        offset: usize,
    ) -> AppResult<Vec<FileHistoryEntry>> {
        let mut walk = repo
            .revwalk()
            .map_err(|e| AppError::Other(format!("revwalk: {e}")))?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
            .map_err(|e| AppError::Other(format!("sort: {e}")))?;
        walk.push_head()
            .map_err(|e| AppError::Other(format!("push head: {e}")))?;

        let target = std::path::Path::new(path);
        let mut results = Vec::new();
        let mut seen = 0usize;

        for oid_result in walk.flatten() {
            let commit = match repo.find_commit(oid_result) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let to_tree = match commit.tree() {
                Ok(t) => t,
                Err(_) => continue,
            };
            let from_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

            // Cheap "did this path change?" check: compare the blob OID of
            // `path` in this commit's tree vs the (first) parent's tree. No diff
            // is computed, so non-touching commits cost only two tree lookups —
            // this is what keeps history walks fast on large repos.
            let new_oid = to_tree.get_path(target).ok().map(|e| e.id());
            let old_oid = from_tree
                .as_ref()
                .and_then(|t| t.get_path(target).ok())
                .map(|e| e.id());
            if new_oid == old_oid {
                continue; // path unchanged in this commit
            }

            // Apply offset before any diff work — skipped entries cost nothing.
            if seen < offset {
                seen += 1;
                continue;
            }

            // Only for entries we actually return: run a pathspec-limited diff
            // so add/del is computed for just this path (not the whole tree).
            let (add, del) = {
                let mut opts = git2::DiffOptions::new();
                opts.pathspec(path);
                match repo.diff_tree_to_tree(from_tree.as_ref(), Some(&to_tree), Some(&mut opts)) {
                    Ok(diff) => (0..diff.deltas().len()).fold((0i64, 0i64), |acc, idx| {
                        let (a, d) = git2::Patch::from_diff(&diff, idx)
                            .ok()
                            .flatten()
                            .map(|p| {
                                let s = p.line_stats().unwrap_or((0, 0, 0));
                                (s.1 as i64, s.2 as i64)
                            })
                            .unwrap_or((0, 0));
                        (acc.0 + a, acc.1 + d)
                    }),
                    Err(_) => (0, 0),
                }
            };

            results.push(FileHistoryEntry {
                sha: commit.id().to_string().chars().take(7).collect(),
                author: commit.author().name().unwrap_or("unknown").to_string(),
                email: commit.author().email().unwrap_or("").to_string(),
                when: commit.time().seconds(),
                summary: commit
                    .message()
                    .unwrap_or("")
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string(),
                add,
                del,
            });
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    /// Blame for `path` at `rev` (or HEAD when `None`): one entry per line.
    pub fn blame(
        &self,
        repo: &git2::Repository,
        path: &str,
        rev: Option<&str>,
    ) -> AppResult<Vec<BlameLine>> {
        // Resolve the starting OID (HEAD or the given rev)
        let newest_commit = match rev {
            Some(r) => {
                let oid = Self::resolve_oid(repo, r)?;
                Some(oid)
            }
            None => None,
        };

        let mut opts = git2::BlameOptions::new();
        if let Some(oid) = newest_commit {
            opts.newest_commit(oid);
        }

        let blame = repo
            .blame_file(std::path::Path::new(path), Some(&mut opts))
            .map_err(|e| AppError::Other(format!("blame '{path}': {e}")))?;

        // Read the file content at the given rev (or HEAD) to get line text.
        // We resolve via the tree so we read the committed version, not the
        // working-tree file (matches what `blame` reports).
        let content_oid = match newest_commit {
            Some(oid) => oid,
            None => repo
                .head()
                .ok()
                .and_then(|h| h.target())
                .ok_or_else(|| AppError::Other("no HEAD".into()))?,
        };
        let file_content = repo
            .find_commit(content_oid)
            .ok()
            .and_then(|c| c.tree().ok())
            .and_then(|t| t.get_path(std::path::Path::new(path)).ok())
            .and_then(|e| repo.find_blob(e.id()).ok())
            .map(|b| String::from_utf8_lossy(b.content()).to_string())
            .unwrap_or_default();

        let file_lines: Vec<&str> = file_content.split('\n').collect();
        let mut result = Vec::new();
        let mut line_num = 1usize;

        for hunk in blame.iter() {
            let sig = hunk.final_signature();
            let sha: String = hunk.final_commit_id().to_string().chars().take(7).collect();
            let author = sig.as_ref().and_then(|s| s.name().ok()).unwrap_or("unknown").to_string();
            let when = sig.as_ref().map(|s| s.when().seconds()).unwrap_or(0);
            // summary: look up the commit message
            let summary = repo
                .find_commit(hunk.final_commit_id())
                .ok()
                .and_then(|c| c.message().ok().map(|m| m.lines().next().unwrap_or("").to_string()))
                .unwrap_or_default();

            for _ in 0..hunk.lines_in_hunk() {
                let line_text = file_lines
                    .get(line_num.saturating_sub(1))
                    .copied()
                    .unwrap_or("")
                    .to_string();
                result.push(BlameLine {
                    n: line_num,
                    sha: sha.clone(),
                    author: author.clone(),
                    when,
                    summary: summary.clone(),
                    line: line_text,
                });
                line_num += 1;
            }
        }

        Ok(result)
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
        let short = head
            .target()?
            .to_string()
            .chars()
            .take(7)
            .collect::<String>();
        Some((branch, short))
    }

    /// Full HEAD oid hex, or None for an unborn HEAD / non-repo. Cheap (one ref
    /// read) — callers use it to detect "did HEAD move" without walking history.
    pub fn head_oid(&self, path: &Path) -> Option<String> {
        Repository::open(path)
            .ok()?
            .head()
            .ok()?
            .target()
            .map(|o| o.to_string())
    }

    /// Push `branch` to `remote` using the system `git` CLI (so the OS credential
    /// helper handles auth — the one place we shell out instead of using git2,
    /// because libgit2 push needs manual credential callbacks). Errors carry git's
    /// stderr.
    pub fn push(&self, worktree: &Path, remote: &str, branch: &str) -> AppResult<()> {
        let mut cmd = crate::core::proc::cmd("git");
        cmd.current_dir(worktree)
            .args(["push", "-u", remote, branch]);
        let out = cmd
            .output()
            .map_err(|e| AppError::Other(format!("git push: {e}")))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(AppError::Other(format!(
                "git push failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_oid_none_unborn_then_moves_on_commit() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        assert!(svc.head_oid(dir.path()).is_none(), "unborn HEAD");
        commit_file(dir.path(), "a.txt", "first");
        let h1 = svc.head_oid(dir.path()).unwrap();
        commit_file(dir.path(), "b.txt", "second");
        let h2 = svc.head_oid(dir.path()).unwrap();
        assert_ne!(h1, h2, "HEAD moves on commit");
        assert_eq!(h2.len(), 40, "full oid hex, not short sha");
    }

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
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap();
    }

    #[test]
    fn log_is_empty_without_commits() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        assert!(svc.log(dir.path(), 10, 0).is_empty());
    }

    #[test]
    fn log_files_counts_changed_files_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        // one commit touching two files
        use git2::Signature;
        std::fs::write(dir.path().join("x.txt"), "x\n").unwrap();
        std::fs::write(dir.path().join("y.txt"), "y\n").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("x.txt")).unwrap();
        index.add_path(Path::new("y.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("Test", "t@t").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "two files", &tree, &[&parent])
            .unwrap();

        let log = svc.log(dir.path(), 10, 0);
        assert_eq!(log[0].files, 2, "two-file commit counts 2");
        assert_eq!(log[1].files, 1, "single-file commit counts 1");
    }

    #[test]
    fn log_offset_pages_through_history() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        for i in 0..5 {
            commit_file(dir.path(), &format!("f{i}.txt"), &format!("c{i}"));
        }
        let p1 = svc.log(dir.path(), 2, 0);
        let p2 = svc.log(dir.path(), 2, 2);
        let p3 = svc.log(dir.path(), 2, 4);
        assert_eq!(
            p1.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(),
            ["c4", "c3"]
        );
        assert_eq!(
            p2.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(),
            ["c2", "c1"]
        );
        assert_eq!(
            p3.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(),
            ["c0"]
        );
        assert!(
            svc.log(dir.path(), 2, 99).is_empty(),
            "past-the-end offset is empty"
        );
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
        assert_eq!(svc.log(dir.path(), 10, 0)[0].message, "wip changes");
    }

    #[test]
    fn commit_only_selected_paths() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        std::fs::write(dir.path().join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "new\n").unwrap();
        svc.commit(dir.path(), "only a", &["a.txt".to_string()])
            .unwrap();
        let st = svc.status(dir.path());
        assert!(
            !st.iter().any(|c| c.path == "a.txt"),
            "a.txt committed: {st:?}"
        );
        assert!(
            st.iter().any(|c| c.path == "b.txt"),
            "b.txt still pending: {st:?}"
        );
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
        assert!(
            !dir.path().join("untracked.txt").exists(),
            "untracked removed"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "x"
        );
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
        let tip = Repository::open(dir.path())
            .unwrap()
            .head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .id();
        assert_eq!(tip, feat_tip, "main fast-forwarded to feature");
        assert!(dir.path().join("b.txt").exists());
    }

    #[test]
    fn push_sends_branch_to_a_local_origin() {
        let origin = tempfile::tempdir().unwrap();
        Repository::init_bare(origin.path()).unwrap();

        let work = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(work.path()).unwrap();
        commit_file(work.path(), "a.txt", "hi");

        let repo = Repository::open(work.path()).unwrap();
        // forward-slash the path so the git CLI accepts it as a local remote on Windows
        let url = origin.path().to_str().unwrap().replace('\\', "/");
        repo.remote("origin", &url).unwrap();
        let branch = repo.head().unwrap().shorthand().unwrap().to_string();

        svc.push(work.path(), "origin", &branch).unwrap();

        let bare = Repository::open(origin.path()).unwrap();
        assert!(
            bare.find_reference(&format!("refs/heads/{branch}")).is_ok(),
            "origin should now have the pushed branch"
        );
    }

    #[test]
    fn file_diff_reports_old_and_new() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.ts", "first"); // committed content = "x"
        std::fs::write(dir.path().join("a.ts"), "const y = 2;\n").unwrap();
        let d = svc.file_diff(dir.path(), "a.ts", None);
        assert_eq!(d.old, "x");
        assert_eq!(d.new, "const y = 2;\n");
        assert_eq!(d.lang, "javascript");
    }

    #[test]
    fn status_detects_a_move_as_one_renamed_entry() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(
            dir.path(),
            "a.txt",
            "hello world\nsecond line\nthird line\n",
        );
        // move a.txt -> sub/b.txt (unstaged, like dragging it in a file explorer)
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::rename(dir.path().join("a.txt"), dir.path().join("sub/b.txt")).unwrap();

        let st = svc.status(dir.path());
        let renamed: Vec<_> = st.iter().filter(|f| f.state == "R").collect();
        assert_eq!(
            renamed.len(),
            1,
            "the move is ONE renamed entry, got: {st:?}"
        );
        assert_eq!(renamed[0].path, "sub/b.txt", "new path");
        assert_eq!(
            renamed[0].old_path.as_deref(),
            Some("a.txt"),
            "carries the old path"
        );
        assert!(
            !st.iter().any(|f| f.state == "A"),
            "no separate Added entry: {st:?}"
        );
        assert!(
            !st.iter().any(|f| f.state == "D"),
            "no separate Deleted entry: {st:?}"
        );
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
        assert!(
            st.iter().any(|c| c.path == "a.txt" && c.state == "M"),
            "{st:?}"
        );
        assert!(
            st.iter().any(|c| c.path == "new.txt" && c.state == "A"),
            "{st:?}"
        );
        // a brand-new untracked file must report its added line count, not +0
        let new = st.iter().find(|c| c.path == "new.txt").unwrap();
        assert_eq!(new.add, 1, "untracked file counts added lines: {st:?}");
        assert_eq!(new.del, 0, "untracked file has no deletions: {st:?}");
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

        assert!(
            wt_path.join("a.txt").exists(),
            "worktree checked out the files"
        );
        let repo = Repository::open(dir.path()).unwrap();
        assert!(repo
            .find_branch("agent/my_task", git2::BranchType::Local)
            .is_ok());

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
        assert!(
            Repository::open(dir.path()).unwrap().head().is_ok(),
            "now has a HEAD"
        );
    }

    // ── diff_treeish tests ──────────────────────────────────────────────────

    /// Build a minimal repo with two commits:
    ///   commit1: a.txt = "x\n"
    ///   commit2: a.txt = "x\ny\n"
    /// Returns (repo_path, parent_tree_oid, head_tree_oid).
    /// The temp directory is kept alive until the OS cleans it up on process exit.
    fn two_commit_repo() -> (std::path::PathBuf, git2::Oid, git2::Oid) {
        use git2::Signature;
        let path = tempfile::tempdir().unwrap().keep();

        let tree1_oid;
        let tree2_oid;
        {
            let repo = git2::Repository::init(&path).unwrap();
            let sig = Signature::now("Test", "t@t").unwrap();

            // commit 1: a.txt = "x\n"
            std::fs::write(path.join("a.txt"), "x\n").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
            tree1_oid = index.write_tree().unwrap();
            let tree1 = repo.find_tree(tree1_oid).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "first", &tree1, &[])
                .unwrap();

            // commit 2: a.txt = "x\ny\n"  (one line added)
            std::fs::write(path.join("a.txt"), "x\ny\n").unwrap();
            let mut index2 = repo.index().unwrap();
            index2.add_path(Path::new("a.txt")).unwrap();
            index2.write().unwrap();
            tree2_oid = index2.write_tree().unwrap();
            let tree2 = repo.find_tree(tree2_oid).unwrap();
            let parent = repo.head().unwrap().peel_to_commit().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "second", &tree2, &[&parent])
                .unwrap();
            // repo, tree1, tree2, parent all dropped here before returning
        }

        (path, tree1_oid, tree2_oid)
    }

    #[test]
    fn diff_treeish_modified_file_add_del_and_state() {
        let svc = GitService::new();
        let (path, from_oid, to_oid) = two_commit_repo();
        let repo = git2::Repository::open(&path).unwrap();
        let from_tree = repo.find_tree(from_oid).unwrap();
        let to_tree = repo.find_tree(to_oid).unwrap();

        let changes = svc.diff_treeish(&repo, from_tree, to_tree);

        assert_eq!(changes.len(), 1, "exactly one changed file: {changes:?}");
        let fc = &changes[0];
        assert_eq!(fc.path, "a.txt", "path: {changes:?}");
        assert_eq!(fc.state, "M", "state should be M (modified): {changes:?}");
        assert_eq!(fc.add, 1, "one line added: {changes:?}");
        assert_eq!(fc.del, 0, "no lines deleted: {changes:?}");
        assert!(fc.old_path.is_none(), "no old_path for M: {changes:?}");
    }

    #[test]
    fn diff_treeish_added_file_from_empty_tree() {
        let svc = GitService::new();
        let (path, from_oid, _to_oid) = two_commit_repo();
        let repo = git2::Repository::open(&path).unwrap();
        // Diff from empty (None) → first commit's tree shows a.txt as Added
        let to_tree = repo.find_tree(from_oid).unwrap(); // first commit = "x\n"
        // Use diff_trees directly with from=None
        let changes = svc.diff_trees(&repo, None, Some(&to_tree));

        assert_eq!(changes.len(), 1, "one file added from empty: {changes:?}");
        let fc = &changes[0];
        assert_eq!(fc.state, "A", "state should be A (added): {changes:?}");
        assert!(fc.add > 0, "added lines > 0: {changes:?}");
    }

    #[test]
    fn diff_treeish_rename_shows_r_state() {
        use git2::Signature;
        let path = tempfile::tempdir().unwrap().keep();

        // Build two commits: a.txt (commit1) → renamed b.txt (commit2)
        let (tree1_oid, tree2_oid) = {
            let repo = git2::Repository::init(&path).unwrap();
            let sig = Signature::now("Test", "t@t").unwrap();

            // commit 1: a.txt with enough content for rename detection
            let content = "line one\nline two\nline three\nline four\nline five\n";
            std::fs::write(path.join("a.txt"), content).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
            let t1 = index.write_tree().unwrap();
            let tree1 = repo.find_tree(t1).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "add a", &tree1, &[])
                .unwrap();

            // commit 2: rename a.txt → b.txt (same content)
            std::fs::rename(path.join("a.txt"), path.join("b.txt")).unwrap();
            let mut index2 = repo.index().unwrap();
            index2.remove_path(Path::new("a.txt")).unwrap();
            index2.add_path(Path::new("b.txt")).unwrap();
            index2.write().unwrap();
            let t2 = index2.write_tree().unwrap();
            let tree2 = repo.find_tree(t2).unwrap();
            let parent = repo.head().unwrap().peel_to_commit().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "rename to b", &tree2, &[&parent])
                .unwrap();
            // all borrows of repo dropped here
            (t1, t2)
        };

        let svc = GitService::new();
        let repo = git2::Repository::open(&path).unwrap();
        let from_tree = repo.find_tree(tree1_oid).unwrap();
        let to_tree = repo.find_tree(tree2_oid).unwrap();
        let changes = svc.diff_treeish(&repo, from_tree, to_tree);

        // Should be a single Renamed entry
        let renamed: Vec<_> = changes.iter().filter(|c| c.state == "R").collect();
        assert_eq!(renamed.len(), 1, "one rename entry: {changes:?}");
        assert_eq!(renamed[0].path, "b.txt");
        assert_eq!(renamed[0].old_path.as_deref(), Some("a.txt"));
    }

    #[test]
    fn ensure_main_branch_creates_main_in_empty_repo() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        assert!(
            Repository::open(dir.path()).unwrap().head().is_err(),
            "starts unborn"
        );

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
        let log = svc.log(dir.path(), 10, 0);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].message, "second");
        assert_eq!(log[1].message, "first");
        assert!(log[0].files >= 1);
    }

    // ── commit_files tests ─────────────────────────────────────────────────

    #[test]
    fn commit_files_reports_changed_file_with_add_del() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        // second commit modifies a.txt
        std::fs::write(dir.path().join("a.txt"), "x\ny\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("T", "t@t").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "modify a", &tree, &[&parent])
            .unwrap();

        let changes = svc.commit_files(&repo, &oid.to_string()).unwrap();
        assert_eq!(changes.len(), 1, "one file changed: {changes:?}");
        assert_eq!(changes[0].path, "a.txt");
        assert_eq!(changes[0].state, "M");
        assert!(changes[0].add > 0 || changes[0].del > 0);
    }

    #[test]
    fn commit_files_root_commit_shows_added() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        std::fs::write(dir.path().join("init.txt"), "hello\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("init.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("T", "t@t").unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "root", &tree, &[])
            .unwrap();

        let changes = svc.commit_files(&repo, &oid.to_string()).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].state, "A", "root commit file is Added");
    }

    // ── file_history tests ─────────────────────────────────────────────────

    #[test]
    fn file_history_returns_commits_that_touch_path() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "init a");
        commit_file(dir.path(), "b.txt", "init b"); // touches b, not a
        // touch a again
        std::fs::write(dir.path().join("a.txt"), "updated\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("T", "t@t").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "update a", &tree, &[&parent])
            .unwrap();

        let history = svc.file_history(&repo, "a.txt", 10, 0).unwrap();
        assert_eq!(history.len(), 2, "two commits touch a.txt: {history:?}");
        assert_eq!(history[0].summary, "update a", "newest first");
        assert_eq!(history[1].summary, "init a");
    }

    #[test]
    fn file_history_offset_pages() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        // Four commits, each changing f.txt's *content* so every commit truly
        // touches the path (the shared commit_file helper writes identical bytes,
        // which would collapse to a single touching commit).
        for i in 0..4 {
            std::fs::write(dir.path().join("f.txt"), format!("content {i}\n")).unwrap();
            let repo = git2::Repository::open(dir.path()).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("f.txt")).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let sig = git2::Signature::now("T", "t@t").unwrap();
            let parent = repo
                .head()
                .ok()
                .and_then(|h| h.target())
                .and_then(|oid| repo.find_commit(oid).ok());
            let parents: Vec<&git2::Commit> = parent.iter().collect();
            repo.commit(Some("HEAD"), &sig, &sig, &format!("c{i}"), &tree, &parents)
                .unwrap();
        }
        let repo = git2::Repository::open(dir.path()).unwrap();
        let all = svc.file_history(&repo, "f.txt", 10, 0).unwrap();
        assert_eq!(all.len(), 4);
        let page = svc.file_history(&repo, "f.txt", 2, 2).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].summary, all[2].summary);
    }

    // ── blame tests ────────────────────────────────────────────────────────

    #[test]
    fn blame_returns_one_entry_per_line() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        // write a 3-line file
        std::fs::write(dir.path().join("f.txt"), "line1\nline2\nline3\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("f.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("Author", "a@a").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add file", &tree, &[])
            .unwrap();

        let lines = svc.blame(&repo, "f.txt", None).unwrap();
        // 3 content lines + trailing empty from split
        assert!(lines.len() >= 3, "at least 3 blame lines: {lines:?}");
        assert_eq!(lines[0].n, 1);
        assert_eq!(lines[1].n, 2);
        assert_eq!(lines[0].author, "Author");
        assert_eq!(lines[0].line, "line1");
        assert_eq!(lines[1].line, "line2");
    }

    // ── range_diff tests ───────────────────────────────────────────────────

    #[test]
    fn range_diff_shows_files_between_two_commits() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = git2::Repository::open(dir.path()).unwrap();
        let from_oid = repo.head().unwrap().target().unwrap();
        commit_file(dir.path(), "b.txt", "second");
        let to_oid = repo.head().unwrap().target().unwrap();

        let changes = svc
            .range_diff(&repo, &from_oid.to_string(), &to_oid.to_string())
            .unwrap();
        assert_eq!(changes.len(), 1, "b.txt added in the range");
        assert_eq!(changes[0].path, "b.txt");
        assert_eq!(changes[0].state, "A");
    }
}
