use std::path::Path;

use git2::Repository;

use crate::core::errors::{AppError, AppResult};

/// What happens to a worktree's working directory when its agent is removed.
///
/// The default is [`KeepFolder`](WorktreeDisposal::KeepFolder): git forgets the
/// worktree but the files stay put, so a delete can never cost someone work
/// they had not committed. [`DeleteFolder`](WorktreeDisposal::DeleteFolder) is
/// the opt-in "hard delete" the confirm modal asks for explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeDisposal {
    /// Deregister from git, leave the directory on disk.
    KeepFolder,
    /// Deregister and delete the directory, uncommitted changes included.
    DeleteFolder,
}

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

/// One conflicted file inside a merge/rebase/cherry-pick session, read from
/// index stages 1/2/3 (base/ours/theirs). `merged` is the working-tree content
/// with diff3 conflict markers — the frontend parses it into per-conflict
/// segments (ctx / ours / base / theirs) for the 3-way view.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFile {
    pub path: String,
    /// Full stage-2 (ours) content; empty when the side deleted the file.
    pub ours: String,
    /// Full stage-3 (theirs) content; empty when the side deleted the file.
    pub theirs: String,
    /// Full stage-1 (common ancestor) content; empty for add/add conflicts.
    pub base: String,
    /// Working-tree content with diff3 conflict markers.
    pub merged: String,
    /// Always false in listings (a resolved file leaves the conflict index);
    /// flipped by the frontend store after `conflict_resolve`.
    pub resolved: bool,
    pub lang: String,
}

/// Result of a native merge: empty `conflicts` = merged cleanly (or was
/// already up to date / fast-forwarded); non-empty = a merge session is now
/// in progress and must be finished via `merge_continue` or `merge_abort`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeSession {
    /// HEAD branch shorthand ("ours" label in the 3-way view).
    pub ours: String,
    /// The branch that was merged in ("theirs" label).
    pub theirs: String,
    pub conflicts: Vec<ConflictFile>,
}

/// Whether a merge / rebase / cherry-pick is in progress and how far along.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    /// "none" | "merge" | "rebase" | "cherrypick" | "revert" | "other".
    pub state: String,
    /// Files still conflicted in the index.
    pub conflicts: usize,
    /// HEAD branch shorthand, when resolvable.
    pub ours: String,
}

/// One commit referenced by a blame — interned ONCE in `Blame::commits` and
/// indexed by `BlameLine::c` (A0.6 blame interning: a 50k-line file must not
/// duplicate author/sha/summary strings per line).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameCommit {
    pub sha: String,
    pub author: String,
    pub when: i64,
    pub summary: String,
}

/// One line in a blame result: `c` indexes into the owning `Blame::commits`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameLine {
    pub n: usize,
    pub c: u32,
    pub line: String,
}

/// Interned blame payload: small commit table + per-line u32 indices.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blame {
    pub commits: Vec<BlameCommit>,
    pub lines: Vec<BlameLine>,
}

pub(crate) fn lang_from_path(rel: &str) -> &'static str {
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

/// Cache key for `status()` (A2.3): a scan whose key is unchanged returns the
/// cached result without touching libgit2 (no index load, no content diffing).
#[derive(Clone, PartialEq, Eq)]
struct StatusKey {
    /// `.git/index` (mtime as unix-nanos, len) — moves on stage/commit/reset.
    index: Option<(u128, u64)>,
    /// HEAD oid — moves on commit/checkout/reset.
    head: Option<String>,
    /// Stat-level fingerprint of the worktree (path/mtime/len of every
    /// non-ignored file) — catches edits, creates and deletes. Same-second
    /// same-length rewrites can alias; the fs watcher's settle window makes
    /// that window practically unobservable for the scan path.
    worktree_fp: u64,
}

struct StatusCacheEntry {
    key: StatusKey,
    /// Whether `changes` carries per-file line counts. A full entry also
    /// serves counts-only requests; a counts-only entry never serves a full one.
    full: bool,
    changes: Vec<FileChange>,
    last_used: std::time::Instant,
}

/// Why 16: one live entry per open repo/worktree is plenty; the tiny cap keeps
/// abandoned worktrees from accumulating entries over a long session.
const STATUS_CACHE_REPOS: usize = 16;

#[derive(Clone, Default)]
pub struct GitService {
    /// Per-repo status cache, shared across clones (Arc). Bounded to
    /// [`STATUS_CACHE_REPOS`] entries, one per repo path; an entry is replaced
    /// whenever its key changes.
    status_cache:
        std::sync::Arc<std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, StatusCacheEntry>>>,
}

impl GitService {
    pub fn new() -> Self {
        Self::default()
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

    /// Prune a worktree's git metadata, optionally deleting its working
    /// directory too. Best-effort: an unknown worktree name is not an error,
    /// because the folder is torn down separately by the caller.
    pub fn remove_worktree(
        &self,
        repo_path: &Path,
        wt_name: &str,
        disposal: WorktreeDisposal,
    ) -> AppResult<()> {
        let repo =
            Repository::open(repo_path).map_err(|e| AppError::Other(format!("open repo: {e}")))?;
        if let Ok(wt) = repo.find_worktree(wt_name) {
            if disposal == WorktreeDisposal::DeleteFolder {
                let _ = remove_dir_all_retry(wt.path());
            }
            let mut opts = git2::WorktreePruneOptions::new();
            // valid(true): prune even though the worktree is healthy — the agent
            // is going away either way. working_tree only when we're allowed to
            // touch the folder; with it false git drops .git/worktrees/<name>
            // and leaves the directory sitting on disk.
            opts.valid(true)
                .working_tree(disposal == WorktreeDisposal::DeleteFolder);
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

    /// HEAD branch shorthand of the checkout at `path` (None when the dir is
    /// not a repo, or HEAD is detached/unborn). Used by the v2 project pseudo
    /// record so a project tab reports the branch main is actually on.
    pub fn head_branch(&self, path: &Path) -> Option<String> {
        let repo = Repository::open(path).ok()?;
        repo.head()
            .ok()
            .and_then(|h| h.shorthand().ok().map(|s| s.to_string()))
    }

    /// Merge `branch` into the repo's current HEAD (fast-forward when possible).
    ///
    /// On conflict the merge state is KEPT (conflicted index + working tree
    /// with diff3 markers) and the returned session carries the conflicts —
    /// the caller resolves via `conflict_resolve` then `merge_continue`, or
    /// discards via `merge_abort`. `cleanup_state` only runs on those paths.
    pub fn merge(&self, repo_path: &Path, branch: &str) -> AppResult<MergeSession> {
        let repo =
            Repository::open(repo_path).map_err(|e| AppError::Other(format!("open: {e}")))?;
        let ours_label = repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().ok().map(|s| s.to_string()))
            .unwrap_or_else(|| "HEAD".into());
        let clean = |repo_ref: &Repository| MergeSession {
            ours: repo_ref
                .head()
                .ok()
                .and_then(|h| h.shorthand().ok().map(|s| s.to_string()))
                .unwrap_or_else(|| "HEAD".into()),
            theirs: branch.to_string(),
            conflicts: Vec::new(),
        };
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
            return Ok(clean(&repo));
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
            return Ok(clean(&repo));
        }

        // true merge → write the (possibly conflicted) index + working tree.
        // allow_conflicts lets the checkout proceed with conflict markers;
        // diff3 style includes the base section so the 3-way UI can show the
        // common ancestor per conflict, not only whole-file.
        let mut co = git2::build::CheckoutBuilder::new();
        co.allow_conflicts(true).conflict_style_diff3(true);
        repo.merge(&[&annotated], None, Some(&mut co))
            .map_err(|e| AppError::Other(e.to_string()))?;
        let mut index = repo.index().map_err(|e| AppError::Other(e.to_string()))?;
        if index.has_conflicts() {
            // KEEP the merge state — the session is now in progress.
            return Ok(MergeSession {
                ours: ours_label,
                theirs: branch.to_string(),
                conflicts: self.conflict_files(repo_path)?,
            });
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
        Ok(clean(&repo))
    }

    // ── conflict session (A3.5 / A3.6) ─────────────────────────────────────

    /// Full text of an index-side blob, or empty when the entry is absent
    /// (deleted on that side / no common ancestor).
    fn stage_text(repo: &Repository, entry: Option<&git2::IndexEntry>) -> String {
        entry
            .and_then(|e| repo.find_blob(e.id).ok())
            .map(|b| String::from_utf8_lossy(b.content()).to_string())
            .unwrap_or_default()
    }

    /// The currently conflicted files, read from index stages 1/2/3. A file
    /// resolved + staged via `conflict_resolve` no longer appears here.
    pub fn conflict_files(&self, repo_path: &Path) -> AppResult<Vec<ConflictFile>> {
        let repo =
            Repository::open(repo_path).map_err(|e| AppError::Other(format!("open: {e}")))?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| AppError::Other("bare repo has no working tree".into()))?
            .to_path_buf();
        let index = repo.index().map_err(|e| AppError::Other(e.to_string()))?;
        let conflicts = index
            .conflicts()
            .map_err(|e| AppError::Other(e.to_string()))?;
        let mut files = Vec::new();
        for c in conflicts.flatten() {
            // Path lives on whichever stage exists (ours may be a delete).
            let path = c
                .our
                .as_ref()
                .or(c.their.as_ref())
                .or(c.ancestor.as_ref())
                .map(|e| String::from_utf8_lossy(&e.path).replace('\\', "/"))
                .unwrap_or_default();
            if path.is_empty() {
                continue;
            }
            let merged = std::fs::read_to_string(workdir.join(&path)).unwrap_or_default();
            files.push(ConflictFile {
                ours: Self::stage_text(&repo, c.our.as_ref()),
                theirs: Self::stage_text(&repo, c.their.as_ref()),
                base: Self::stage_text(&repo, c.ancestor.as_ref()),
                merged,
                resolved: false,
                lang: lang_from_path(&path).to_string(),
                path,
            });
        }
        Ok(files)
    }

    /// Write `content` as the resolution of `rel` and stage it — the path's
    /// conflict entries collapse into a single stage-0 entry.
    pub fn conflict_resolve(&self, repo_path: &Path, rel: &str, content: &str) -> AppResult<()> {
        let repo =
            Repository::open(repo_path).map_err(|e| AppError::Other(format!("open: {e}")))?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| AppError::Other("bare repo has no working tree".into()))?;
        let abs = workdir.join(rel);
        if let Some(parent) = abs.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&abs, content)
            .map_err(|e| AppError::Other(format!("write '{rel}': {e}")))?;
        let mut index = repo.index().map_err(|e| AppError::Other(e.to_string()))?;
        index
            .add_path(Path::new(rel))
            .map_err(|e| AppError::Other(format!("stage '{rel}': {e}")))?;
        index.write().map_err(|e| AppError::Other(e.to_string()))?;
        Ok(())
    }

    /// Abort the in-progress merge: drop the merge state and restore the
    /// working tree + index to HEAD. This is the ONLY place (besides a
    /// completed `merge_continue`) that calls `cleanup_state`.
    pub fn merge_abort(&self, repo_path: &Path) -> AppResult<()> {
        let repo =
            Repository::open(repo_path).map_err(|e| AppError::Other(format!("open: {e}")))?;
        repo.cleanup_state()
            .map_err(|e| AppError::Other(e.to_string()))?;
        let head_tree = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| AppError::Other(e.to_string()))?;
        // Reset index + working tree to HEAD so half-resolved files don't linger.
        repo.reset(head_tree.as_object(), git2::ResetType::Hard, None)
            .map_err(|e| AppError::Other(e.to_string()))?;
        Ok(())
    }

    /// Finish the in-progress merge once every conflict is resolved + staged:
    /// commit the index with HEAD + MERGE_HEAD(s) as parents, then clean up
    /// the merge state. Returns the short sha.
    pub fn merge_continue(&self, repo_path: &Path, message: Option<&str>) -> AppResult<String> {
        // Why mut: git2's mergehead_foreach takes &mut self.
        let mut repo =
            Repository::open(repo_path).map_err(|e| AppError::Other(format!("open: {e}")))?;
        let mut index = repo.index().map_err(|e| AppError::Other(e.to_string()))?;
        if index.has_conflicts() {
            return Err(AppError::Other(
                "unresolved conflicts remain — resolve every file first".into(),
            ));
        }
        let mut merge_heads: Vec<git2::Oid> = Vec::new();
        repo.mergehead_foreach(|oid| {
            merge_heads.push(*oid);
            true
        })
        .map_err(|_| AppError::Other("no merge in progress (MERGE_HEAD missing)".into()))?;
        if merge_heads.is_empty() {
            return Err(AppError::Other("no merge in progress".into()));
        }
        let tree = repo
            .find_tree(
                index
                    .write_tree()
                    .map_err(|e| AppError::Other(e.to_string()))?,
            )
            .map_err(|e| AppError::Other(e.to_string()))?;
        let head_commit = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| AppError::Other(e.to_string()))?;
        let their_commits: Vec<git2::Commit> = merge_heads
            .iter()
            .filter_map(|oid| repo.find_commit(*oid).ok())
            .collect();
        let mut parents: Vec<&git2::Commit> = vec![&head_commit];
        parents.extend(their_commits.iter());
        let sig = repo
            .signature()
            .or_else(|_| git2::Signature::now("orrery", "orrery@local"))
            .map_err(|e| AppError::Other(e.to_string()))?;
        // Default message: MERGE_MSG when git wrote one, else a plain header.
        let default_msg = std::fs::read_to_string(repo.path().join("MERGE_MSG"))
            .ok()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| {
                format!(
                    "merge {}",
                    merge_heads
                        .first()
                        .map(|o| o.to_string().chars().take(7).collect::<String>())
                        .unwrap_or_default()
                )
            });
        let msg = match message {
            Some(m) if !m.trim().is_empty() => m,
            _ => default_msg.as_str(),
        };
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let _ = repo.cleanup_state();
        Ok(oid.to_string().chars().take(7).collect())
    }

    /// Whether a merge / rebase / cherry-pick is in progress, and how many
    /// files are still conflicted.
    pub fn session_state(&self, repo_path: &Path) -> AppResult<SessionState> {
        let repo =
            Repository::open(repo_path).map_err(|e| AppError::Other(format!("open: {e}")))?;
        let state = match repo.state() {
            git2::RepositoryState::Clean => "none",
            git2::RepositoryState::Merge => "merge",
            git2::RepositoryState::Rebase
            | git2::RepositoryState::RebaseInteractive
            | git2::RepositoryState::RebaseMerge => "rebase",
            git2::RepositoryState::CherryPick | git2::RepositoryState::CherryPickSequence => {
                "cherrypick"
            }
            git2::RepositoryState::Revert | git2::RepositoryState::RevertSequence => "revert",
            _ => "other",
        };
        let conflicts = repo
            .index()
            .ok()
            .and_then(|i| i.conflicts().ok().map(|c| c.flatten().count()))
            .unwrap_or(0);
        Ok(SessionState {
            state: state.into(),
            conflicts,
            ours: repo
                .head()
                .ok()
                .and_then(|h| h.shorthand().ok().map(|s| s.to_string()))
                .unwrap_or_else(|| "HEAD".into()),
        })
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
        self.status_scan(path, true)
    }

    /// A2.2: like [`status`] but SKIPS per-file line-count diffing (add/del
    /// stay 0) — states and paths only, for non-focused agents' background
    /// scans. Full deltas are recomputed on reveal.
    pub fn status_counts_only(&self, path: &Path) -> Vec<FileChange> {
        self.status_scan(path, false)
    }

    /// Cache-aware status entry point (A2.3): unchanged key → cached
    /// `Vec<FileChange>` without touching libgit2.
    fn status_scan(&self, path: &Path, with_line_counts: bool) -> Vec<FileChange> {
        let key = Self::status_key(path);
        if let Some(k) = &key {
            let mut cache = self.status_cache.lock().unwrap();
            if let Some(e) = cache.get_mut(path) {
                // A full entry satisfies a counts-only request (extra counts
                // are harmless); a counts-only entry cannot satisfy a full one.
                if e.key == *k && (e.full || !with_line_counts) {
                    e.last_used = std::time::Instant::now();
                    return e.changes.clone();
                }
            }
        }
        let changes = self.status_uncached(path, with_line_counts);
        if let Some(k) = key {
            let mut cache = self.status_cache.lock().unwrap();
            if cache.len() >= STATUS_CACHE_REPOS && !cache.contains_key(path) {
                // evict the least-recently-used repo to stay bounded
                if let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, e)| e.last_used)
                    .map(|(p, _)| p.clone())
                {
                    cache.remove(&oldest);
                }
            }
            cache.insert(
                path.to_path_buf(),
                StatusCacheEntry {
                    key: k,
                    full: with_line_counts,
                    changes: changes.clone(),
                    last_used: std::time::Instant::now(),
                },
            );
        }
        changes
    }

    /// Compute the status cache key: (index mtime+len, HEAD oid, worktree
    /// dirty-fingerprint). None (→ never cache) for non-repos or when the
    /// workdir cannot be resolved.
    fn status_key(path: &Path) -> Option<StatusKey> {
        let repo = Repository::open(path).ok()?;
        let index = std::fs::metadata(repo.path().join("index"))
            .ok()
            .and_then(|m| {
                let mtime = m
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_nanos();
                Some((mtime, m.len()))
            });
        let head = repo.head().ok().and_then(|h| h.target()).map(|o| o.to_string());
        let workdir = repo.workdir()?.to_path_buf();
        Some(StatusKey {
            index,
            head,
            worktree_fp: crate::search::worktree_fingerprint(&workdir),
        })
    }

    fn status_uncached(&self, path: &Path, with_line_counts: bool) -> Vec<FileChange> {
        use std::cell::RefCell;
        use std::collections::BTreeMap;

        let Ok(repo) = Repository::open(path) else {
            return Vec::new();
        };
        let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let mut opts = git2::DiffOptions::new();
        // `show_untracked_content` is what makes the line-level diff callback fire
        // for untracked files — without it a brand-new N-line file reports +0/-0
        // because git2 only emits its delta header, never its lines. Reading
        // untracked content is also the expensive part, so counts-only scans
        // skip it (A2.2).
        opts.include_untracked(true).recurse_untracked_dirs(true);
        if with_line_counts {
            opts.show_untracked_content(true);
        }
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
        let mut file_cb = |delta: git2::DiffDelta, _progress: f32| {
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
        };
        let _ = if with_line_counts {
            diff.foreach(
                &mut file_cb,
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
            )
        } else {
            // A2.2 counts-only: no line callback → no per-file content diffing.
            diff.foreach(&mut file_cb, None, None, None)
        };

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
        // Full file content on each side (parent's blob vs this commit's blob) so
        // the diff shows the whole file and per-line blame aligns by line number.
        let new = Self::blob_text(repo, &to_tree, path);
        let old = from_tree
            .as_ref()
            .map(|t| Self::blob_text(repo, t, path))
            .unwrap_or_default();
        Ok(FileDiff {
            old,
            new,
            lang: lang_from_path(path).to_string(),
        })
    }

    /// Full UTF-8 text of `path` in `tree`, or empty when the path/blob is absent.
    fn blob_text(repo: &git2::Repository, tree: &git2::Tree, path: &str) -> String {
        tree.get_path(std::path::Path::new(path))
            .ok()
            .and_then(|e| repo.find_blob(e.id()).ok())
            .map(|b| String::from_utf8_lossy(b.content()).to_string())
            .unwrap_or_default()
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
        // Full file content on each side so the diff is whole-file and per-line
        // blame aligns by line number.
        let old = Self::blob_text(repo, &from_tree, path);
        let new = Self::blob_text(repo, &to_tree, path);
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
    ) -> AppResult<Blame> {
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

        Ok(Self::blame_to_lines(repo, &blame, &file_content))
    }

    /// Build an interned `Blame` from a git2 `Blame` and the file content it was
    /// computed over: commit metadata is stored once per distinct commit, each
    /// line carries only its index. Lines with a zero oid (from `blame_buffer`
    /// on a modified working tree) are reported as "Uncommitted".
    fn blame_to_lines(
        repo: &git2::Repository,
        blame: &git2::Blame,
        file_content: &str,
    ) -> Blame {
        let file_lines: Vec<&str> = file_content.split('\n').collect();
        let mut commits: Vec<BlameCommit> = Vec::new();
        let mut by_sha: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut lines = Vec::new();
        let mut line_num = 1usize;

        for hunk in blame.iter() {
            let oid = hunk.final_commit_id();
            let uncommitted = oid.is_zero();
            let sha: String = oid.to_string().chars().take(7).collect();
            let c = match by_sha.get(&sha) {
                Some(&i) => i,
                None => {
                    let sig = hunk.final_signature();
                    let author = if uncommitted {
                        "Uncommitted".to_string()
                    } else {
                        sig.as_ref().and_then(|s| s.name().ok()).unwrap_or("unknown").to_string()
                    };
                    let when = sig.as_ref().map(|s| s.when().seconds()).unwrap_or(0);
                    let summary = if uncommitted {
                        "Uncommitted changes".to_string()
                    } else {
                        repo.find_commit(oid)
                            .ok()
                            .and_then(|c| c.message().ok().map(|m| m.lines().next().unwrap_or("").to_string()))
                            .unwrap_or_default()
                    };
                    let i = commits.len() as u32;
                    commits.push(BlameCommit {
                        sha: sha.clone(),
                        author,
                        when,
                        summary,
                    });
                    by_sha.insert(sha, i);
                    i
                }
            };

            for _ in 0..hunk.lines_in_hunk() {
                let line_text = file_lines
                    .get(line_num.saturating_sub(1))
                    .copied()
                    .unwrap_or("")
                    .to_string();
                lines.push(BlameLine {
                    n: line_num,
                    c,
                    line: line_text,
                });
                line_num += 1;
            }
        }

        Blame { commits, lines }
    }

    /// Blame both sides of the working-tree diff for `path`:
    ///  - `old` = HEAD content blamed at HEAD,
    ///  - `new` = working-tree content blamed via `blame_buffer` (your uncommitted
    ///    edits map to "Uncommitted"; untouched lines keep their original author).
    pub fn working_blame(
        &self,
        repo: &git2::Repository,
        path: &str,
    ) -> AppResult<(Blame, Blame)> {
        let p = std::path::Path::new(path);
        let work_dir = repo
            .workdir()
            .ok_or_else(|| AppError::Other("no workdir".into()))?;
        let working = std::fs::read(work_dir.join(path)).unwrap_or_default();
        let working_str = String::from_utf8_lossy(&working).to_string();

        // A brand-new (untracked) file has no HEAD history to blame — the whole
        // file is your uncommitted work, and there is no old side.
        let Ok(blame) = repo.blame_file(p, None) else {
            return Ok((
                Blame { commits: Vec::new(), lines: Vec::new() },
                Self::uncommitted_lines(&working_str),
            ));
        };

        let head_content = repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_tree().ok())
            .and_then(|t| t.get_path(p).ok())
            .and_then(|e| repo.find_blob(e.id()).ok())
            .map(|b| String::from_utf8_lossy(b.content()).to_string())
            .unwrap_or_default();
        let old = Self::blame_to_lines(repo, &blame, &head_content);

        // New side: re-map the committed blame onto the working buffer so edited
        // lines become "Uncommitted". If that fails, treat the whole side as uncommitted.
        let new = match blame.blame_buffer(&working) {
            Ok(buf) => Self::blame_to_lines(repo, &buf, &working_str),
            Err(_) => Self::uncommitted_lines(&working_str),
        };

        Ok((old, new))
    }

    /// A whole-file "Uncommitted" blame (for new/untracked files): one commit
    /// table entry, every line indexing it.
    fn uncommitted_lines(content: &str) -> Blame {
        let commits = vec![BlameCommit {
            sha: "0000000".to_string(),
            author: "Uncommitted".to_string(),
            when: 0,
            summary: "Uncommitted changes".to_string(),
        }];
        let lines = content
            .split('\n')
            .enumerate()
            .map(|(i, line)| BlameLine {
                n: i + 1,
                c: 0,
                line: line.to_string(),
            })
            .collect();
        Blame { commits, lines }
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

    /// The repository's default branch — the sensible base for a new worktree:
    /// `origin/HEAD`'s target when a remote sets one, else a conventional local
    /// `main`/`master`, else the currently checked-out branch (when it is a real
    /// local branch). `None` for a non-repo / when nothing resolves, in which case
    /// callers fall back to the first branch in the list.
    pub fn default_branch(&self, path: &Path) -> Option<String> {
        let repo = Repository::open(path).ok()?;
        // 1. the remote's declared default: refs/remotes/origin/HEAD → "origin/<x>"
        if let Ok(reference) = repo.find_reference("refs/remotes/origin/HEAD") {
            if let Some(name) = reference
                .symbolic_target()
                .ok()
                .flatten()
                .and_then(|t| t.strip_prefix("refs/remotes/origin/"))
            {
                return Some(name.to_string());
            }
        }
        let locals = self.branches(path);
        // 2. a conventional default that exists locally
        for cand in ["main", "master"] {
            if locals.iter().any(|b| b == cand) {
                return Some(cand.to_string());
            }
        }
        // 3. the current HEAD branch, but only when it's a genuine local branch
        //    (guards against a detached HEAD shorthand that isn't a branch name)
        if let Some((b, _)) = self.head_info(path) {
            if locals.iter().any(|x| *x == b) {
                return Some(b);
            }
        }
        None
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
    /// Clone `url` into `target` (git creates the folder; it must be absent or
    /// empty). `depth: Some(n)` makes a shallow clone — git then also fetches
    /// only the remote's default branch (`--depth` implies `--single-branch`).
    /// CLI shell-out so the OS credential helper can authenticate private
    /// remotes — the same pattern as `push`.
    pub fn clone_repo(&self, url: &str, target: &Path, depth: Option<u32>) -> AppResult<()> {
        let mut cmd = crate::core::proc::cmd("git");
        cmd.arg("clone");
        if let Some(d) = depth {
            cmd.arg("--depth").arg(d.to_string());
        }
        cmd.arg(url).arg(target);
        let out = cmd
            .output()
            .map_err(|e| AppError::Other(format!("git clone: {e}")))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(AppError::Other(format!(
                "git clone failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

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
    fn default_branch_prefers_origin_head() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = Repository::open(dir.path()).unwrap();
        // simulate a clone whose remote declares `develop` as its default
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/develop",
            true,
            "test",
        )
        .unwrap();
        assert_eq!(svc.default_branch(dir.path()).as_deref(), Some("develop"));
    }

    #[test]
    fn default_branch_prefers_main_over_alphabetical_first() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = Repository::open(dir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        // the conventional default this repo actually has (main or master,
        // depending on the git2/system default)
        let default = repo.head().unwrap().shorthand().unwrap().to_string();
        assert!(matches!(default.as_str(), "main" | "master"), "{default}");
        // a branch that sorts before it alphabetically must not win
        repo.branch("aaa-feature", &head, false).unwrap();
        assert_eq!(svc.default_branch(dir.path()), Some(default));
    }

    #[test]
    fn default_branch_falls_back_to_head_branch() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = Repository::open(dir.path()).unwrap();
        // no origin/HEAD, no main/master: rename the default branch to `trunk`
        let head = repo.head().unwrap().shorthand().unwrap().to_string();
        let mut b = repo.find_branch(&head, git2::BranchType::Local).unwrap();
        b.rename("trunk", false).unwrap();
        repo.set_head("refs/heads/trunk").unwrap();
        assert_eq!(svc.default_branch(dir.path()).as_deref(), Some("trunk"));
    }

    #[test]
    fn default_branch_none_for_non_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(GitService::new().default_branch(dir.path()), None);
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

        svc.remove_worktree(dir.path(), "my_task", WorktreeDisposal::DeleteFolder)
            .unwrap();
        assert!(!wt_path.exists(), "worktree dir removed");
    }

    #[test]
    fn remove_worktree_keep_folder_deregisters_but_leaves_the_files() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");

        let wt_root = tempfile::tempdir().unwrap();
        let wt_path = wt_root.path().join("kept");
        svc.create_worktree(dir.path(), "kept", "agent/kept", None, &wt_path)
            .unwrap();
        // an uncommitted file — exactly what a soft delete must not destroy
        std::fs::write(wt_path.join("scratch.txt"), "unsaved work").unwrap();

        svc.remove_worktree(dir.path(), "kept", WorktreeDisposal::KeepFolder)
            .unwrap();

        assert!(wt_path.exists(), "folder kept");
        assert_eq!(
            std::fs::read_to_string(wt_path.join("scratch.txt")).unwrap(),
            "unsaved work",
            "uncommitted work survives"
        );
        let repo = Repository::open(dir.path()).unwrap();
        assert!(
            repo.find_worktree("kept").is_err(),
            "git no longer tracks the worktree"
        );
    }

    #[test]
    fn remove_worktree_is_a_noop_for_an_unknown_name() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        // the agent layer deletes the folder itself; a missing registration is
        // not an error here
        svc.remove_worktree(dir.path(), "never-existed", WorktreeDisposal::DeleteFolder)
            .unwrap();
    }

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

    // ── commit_file_diff tests (pathspec-limited) ──────────────────────────

    /// Helper: stage `name` with `content` and commit it onto HEAD, returning the sha.
    fn commit_content(repo_path: &Path, name: &str, content: &str, msg: &str) -> String {
        std::fs::create_dir_all(repo_path.join(name).parent().unwrap()).ok();
        std::fs::write(repo_path.join(name), content).unwrap();
        let repo = git2::Repository::open(repo_path).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("T", "t@t").unwrap();
        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .and_then(|oid| repo.find_commit(oid).ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap()
            .to_string()
    }

    #[test]
    fn commit_file_diff_returns_old_and_new_for_nested_path() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_content(dir.path(), "src/foo.txt", "line1\n", "init");
        let sha = commit_content(dir.path(), "src/foo.txt", "line1\nline2\n", "add line2");
        let repo = git2::Repository::open(dir.path()).unwrap();

        let fd = svc.commit_file_diff(&repo, &sha, "src/foo.txt").unwrap();
        assert!(fd.new.contains("line2"), "new content has line2: {fd:?}");
        assert!(fd.new.contains("line1"));
        assert!(fd.old.contains("line1"));
        assert!(!fd.old.contains("line2"), "old content lacks line2");
    }

    #[test]
    fn commit_file_diff_isolates_one_file_among_many() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_content(dir.path(), "a.txt", "aaa\n", "init a");
        // a commit that touches BOTH a.txt and b.txt
        std::fs::write(dir.path().join("a.txt"), "aaa\nAAA\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "bbb\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.add_path(Path::new("b.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("T", "t@t").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        let sha = repo
            .commit(Some("HEAD"), &sig, &sig, "touch both", &tree, &[&parent])
            .unwrap()
            .to_string();

        let fd = svc.commit_file_diff(&repo, &sha, "a.txt").unwrap();
        assert!(fd.new.contains("AAA"), "a.txt diff has its own content: {fd:?}");
        assert!(!fd.new.contains("bbb"), "a.txt diff must not bleed b.txt content");
    }

    #[test]
    fn range_file_diff_returns_content_for_path() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        let from = commit_content(dir.path(), "f.txt", "v1\n", "v1");
        commit_content(dir.path(), "f.txt", "v1\nv2\n", "v2");
        let to = commit_content(dir.path(), "f.txt", "v1\nv2\nv3\n", "v3");
        let repo = git2::Repository::open(dir.path()).unwrap();

        let fd = svc.range_file_diff(&repo, &from, &to, "f.txt").unwrap();
        assert!(fd.new.contains("v3"), "range new has latest content: {fd:?}");
        assert!(fd.old.contains("v1"));
        assert!(!fd.old.contains("v3"), "range old is the from-side");
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

        let b = svc.blame(&repo, "f.txt", None).unwrap();
        // 3 content lines + trailing empty from split
        assert!(b.lines.len() >= 3, "at least 3 blame lines: {:?}", b.lines);
        assert_eq!(b.lines[0].n, 1);
        assert_eq!(b.lines[1].n, 2);
        assert_eq!(b.commits[b.lines[0].c as usize].author, "Author");
        assert_eq!(b.lines[0].line, "line1");
        assert_eq!(b.lines[1].line, "line2");
    }

    /// The A0.6 interning contract: commit metadata appears ONCE in `commits`,
    /// per-line entries carry only a u32 index into it.
    #[test]
    fn blame_interns_commits_once_with_per_line_indices() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_content(dir.path(), "f.txt", "a\nb\nc\n", "c1");
        commit_content(dir.path(), "f.txt", "a\nb\nc\nd\ne\n", "c2");
        let repo = git2::Repository::open(dir.path()).unwrap();

        let b = svc.blame(&repo, "f.txt", None).unwrap();
        assert_eq!(
            b.commits.len(),
            2,
            "one table entry per DISTINCT commit, not per line: {:?}",
            b.commits
        );
        assert!(b.lines.len() >= 5);
        assert!(
            b.lines.iter().all(|l| (l.c as usize) < b.commits.len()),
            "every line index resolves into the commit table"
        );
        let commit_of = |n: usize| {
            let l = b.lines.iter().find(|l| l.n == n).unwrap();
            &b.commits[l.c as usize]
        };
        assert_eq!(commit_of(1).summary, "c1", "untouched lines keep c1");
        assert_eq!(commit_of(4).summary, "c2", "appended lines carry c2");
        assert_eq!(commit_of(1).sha.len(), 7, "short sha in the table");
    }

    #[test]
    fn working_blame_marks_uncommitted_lines_via_the_commit_table() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_content(dir.path(), "f.txt", "one\ntwo\n", "init");
        std::fs::write(dir.path().join("f.txt"), "one\ntwo\nthree\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();

        let (_old, new) = svc.working_blame(&repo, "f.txt").unwrap();
        let line3 = new.lines.iter().find(|l| l.n == 3).unwrap();
        assert_eq!(
            new.commits[line3.c as usize].author, "Uncommitted",
            "your fresh edit maps to the interned Uncommitted entry"
        );
        let line1 = new.lines.iter().find(|l| l.n == 1).unwrap();
        assert_eq!(
            new.commits[line1.c as usize].author, "T",
            "untouched lines keep their original author"
        );
    }

    // ── status cache tests (A2.3) ──────────────────────────────────────────

    #[test]
    fn status_cache_serves_unchanged_key_and_invalidates_on_worktree_edit() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_content(dir.path(), "a.txt", "one\n", "init");

        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        let s1 = svc.status(dir.path());
        assert_eq!(s1.len(), 1);
        assert_eq!((s1[0].path.as_str(), s1[0].add), ("a.txt", 1));

        // unchanged key → cached result (same shape, no recompute needed)
        let s2 = svc.status(dir.path());
        assert_eq!(s2.len(), 1);
        assert_eq!((s2[0].path.as_str(), s2[0].add), ("a.txt", 1));

        // a worktree edit changes the dirty fingerprint → fresh result
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree!\n").unwrap();
        let s3 = svc.status(dir.path());
        assert_eq!(s3[0].add, 2, "stale cache would still say 1: {s3:?}");

        // a brand-new untracked file also invalidates (fingerprint sees creates)
        std::fs::write(dir.path().join("new.txt"), "n\n").unwrap();
        let s4 = svc.status(dir.path());
        assert!(
            s4.iter().any(|c| c.path == "new.txt"),
            "created file must appear despite unchanged index/HEAD: {s4:?}"
        );

        // commit moves index + HEAD → key changes → clean status
        svc.commit(dir.path(), "all", &[]).unwrap();
        assert!(svc.status(dir.path()).is_empty(), "clean after commit");
    }

    #[test]
    fn status_counts_only_skips_line_counts_but_not_states() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_content(dir.path(), "a.txt", "one\n", "init");
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.path().join("new.txt"), "x\ny\n").unwrap();

        let st = svc.status_counts_only(dir.path());
        let a = st.iter().find(|c| c.path == "a.txt").unwrap();
        assert_eq!(a.state, "M");
        assert_eq!((a.add, a.del), (0, 0), "counts-only: no line diffing");
        let n = st.iter().find(|c| c.path == "new.txt").unwrap();
        assert_eq!(n.state, "A");
        assert_eq!((n.add, n.del), (0, 0));

        // a counts-only cache entry must NOT satisfy a later full request
        let full = svc.status(dir.path());
        let a = full.iter().find(|c| c.path == "a.txt").unwrap();
        assert_eq!(a.add, 1, "full scan recomputes real counts: {full:?}");
        let n = full.iter().find(|c| c.path == "new.txt").unwrap();
        assert_eq!(n.add, 2, "untracked content counted on the full scan");

        // …while a full entry DOES satisfy a counts-only request (with counts)
        let again = svc.status_counts_only(dir.path());
        let a = again.iter().find(|c| c.path == "a.txt").unwrap();
        assert_eq!(a.add, 1, "served from the full cache entry");
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

    // ── merge session tests (A3.5 / A3.6) ──────────────────────────────────

    /// Fixture: a repo where merging `feature` into the default branch
    /// conflicts on `f.txt` (both sides edited the same line since the base).
    /// Returns (tempdir, default-branch name). Built with the commit_content
    /// helper so each commit's content is explicit.
    fn conflicted_merge_repo() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_content(dir.path(), "f.txt", "shared\nbase line\ntail\n", "base");
        let repo = Repository::open(dir.path()).unwrap();
        // Why: the temp repo inherits the developer's global core.autocrlf; a
        // `true` there makes every checkout smudge LF→CRLF and byte-exact
        // working-tree assertions become platform-dependent. Pin it off.
        repo.config()
            .unwrap()
            .set_bool("core.autocrlf", false)
            .unwrap();
        let main = repo.head().unwrap().shorthand().unwrap().to_string();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &base, false).unwrap();

        let checkout = |to: &str| {
            repo.set_head(to).unwrap();
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            repo.checkout_head(Some(&mut co)).unwrap();
        };
        checkout("refs/heads/feature");
        commit_content(dir.path(), "f.txt", "shared\ntheirs line\ntail\n", "theirs edit");
        checkout(&format!("refs/heads/{main}"));
        commit_content(dir.path(), "f.txt", "shared\nours line\ntail\n", "ours edit");
        (dir, main)
    }

    #[test]
    fn merge_conflict_returns_session_and_keeps_state() {
        let (dir, main) = conflicted_merge_repo();
        let svc = GitService::new();

        let session = svc.merge(dir.path(), "feature").unwrap();
        assert_eq!(session.ours, main, "ours label = HEAD branch");
        assert_eq!(session.theirs, "feature");
        assert_eq!(session.conflicts.len(), 1, "{:?}", session.conflicts);
        let cf = &session.conflicts[0];
        assert_eq!(cf.path, "f.txt");
        assert!(cf.ours.contains("ours line"), "stage 2: {cf:?}");
        assert!(cf.theirs.contains("theirs line"), "stage 3: {cf:?}");
        assert!(cf.base.contains("base line"), "stage 1: {cf:?}");
        assert!(cf.merged.contains("<<<<<<<"), "workdir has markers: {cf:?}");
        assert!(!cf.resolved);

        // the merge state is KEPT, not thrown away
        let repo = Repository::open(dir.path()).unwrap();
        assert_eq!(repo.state(), git2::RepositoryState::Merge);
        let st = svc.session_state(dir.path()).unwrap();
        assert_eq!(st.state, "merge");
        assert_eq!(st.conflicts, 1);
    }

    #[test]
    fn merge_clean_returns_empty_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_content(dir.path(), "a.txt", "a\n", "base");
        let repo = Repository::open(dir.path()).unwrap();
        let main = repo.head().unwrap().shorthand().unwrap().to_string();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &base, false).unwrap();
        repo.set_head("refs/heads/feature").unwrap();
        let mut co = git2::build::CheckoutBuilder::new();
        co.force();
        repo.checkout_head(Some(&mut co)).unwrap();
        commit_content(dir.path(), "b.txt", "b\n", "feature adds b");
        repo.set_head(&format!("refs/heads/{main}")).unwrap();
        let mut co2 = git2::build::CheckoutBuilder::new();
        co2.force();
        repo.checkout_head(Some(&mut co2)).unwrap();
        // diverge main so it isn't a fast-forward
        commit_content(dir.path(), "c.txt", "c\n", "main adds c");

        let session = svc.merge(dir.path(), "feature").unwrap();
        assert!(session.conflicts.is_empty(), "{:?}", session.conflicts);
        assert_eq!(
            svc.session_state(dir.path()).unwrap().state,
            "none",
            "clean merge leaves no session"
        );
        assert!(dir.path().join("b.txt").exists(), "merged content present");
    }

    #[test]
    fn conflict_resolve_stages_and_merge_continue_commits() {
        let (dir, _main) = conflicted_merge_repo();
        let svc = GitService::new();
        svc.merge(dir.path(), "feature").unwrap();

        // continue before resolving must refuse
        assert!(svc.merge_continue(dir.path(), None).is_err());

        svc.conflict_resolve(dir.path(), "f.txt", "shared\nresolved line\ntail\n")
            .unwrap();
        assert!(
            svc.conflict_files(dir.path()).unwrap().is_empty(),
            "resolved file left the conflict index"
        );
        assert_eq!(svc.session_state(dir.path()).unwrap().conflicts, 0);

        let sha = svc.merge_continue(dir.path(), Some("merge feature")).unwrap();
        assert_eq!(sha.len(), 7, "short sha");
        let repo = Repository::open(dir.path()).unwrap();
        assert_eq!(repo.state(), git2::RepositoryState::Clean, "state cleaned");
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.parent_count(), 2, "a true merge commit");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "shared\nresolved line\ntail\n"
        );
    }

    #[test]
    fn merge_abort_restores_head_and_clears_state() {
        let (dir, _main) = conflicted_merge_repo();
        let svc = GitService::new();
        svc.merge(dir.path(), "feature").unwrap();
        assert!(std::fs::read_to_string(dir.path().join("f.txt"))
            .unwrap()
            .contains("<<<<<<<"));

        svc.merge_abort(dir.path()).unwrap();

        let repo = Repository::open(dir.path()).unwrap();
        assert_eq!(repo.state(), git2::RepositoryState::Clean);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "shared\nours line\ntail\n",
            "working tree restored to HEAD (ours)"
        );
        assert_eq!(svc.session_state(dir.path()).unwrap().state, "none");
    }

    #[test]
    fn session_state_none_on_clean_repo() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new();
        svc.init(dir.path()).unwrap();
        commit_content(dir.path(), "a.txt", "a\n", "init");
        let st = svc.session_state(dir.path()).unwrap();
        assert_eq!(st.state, "none");
        assert_eq!(st.conflicts, 0);
    }
}
