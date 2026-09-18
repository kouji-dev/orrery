//! The gitoxide half: open a repository, and answer five questions about it.
//!
//! Everything here is **read-only**. Nothing in this module writes an object, a
//! ref or an index entry, which is why the manifest asks for no `write` grant
//! at all — see the crate docs for what happens when the write verbs land.
//!
//! The knowledge is `ade/src-tauri/src/git/gix_backend.rs`'s, reused rather
//! than rediscovered: the same `repo.status(..)` platform with the same
//! rename-detection setup, the same `rev_walk` sorting, the same `blame_file`
//! call. What is not reused is the ADE's types, because those are Tauri
//! commands and this is a tool bundle.

use std::path::{Path, PathBuf};

use gix::bstr::{BStr, BString, ByteSlice};

/// A repository could not answer.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// There is no repository there.
    #[error("no git repository at {path}: {message}")]
    NotARepository {
        /// Where we looked.
        path: PathBuf,
        /// What gitoxide said.
        message: String,
    },
    /// The repository is bare, and the question needs a worktree.
    #[error("this repository has no worktree, so `{what}` has nothing to report on")]
    NoWorktree {
        /// Which verb.
        what: &'static str,
    },
    /// A revision that names nothing.
    #[error("no such revision: {0}")]
    NoSuchRevision(String),
    /// Anything else gitoxide refused.
    #[error("{what}: {message}")]
    Failed {
        /// What was being done.
        what: &'static str,
        /// What went wrong.
        message: String,
    },
}

/// Wrap whatever gitoxide returned.
fn fail<E: std::fmt::Display>(what: &'static str) -> impl FnOnce(E) -> GitError {
    move |e| GitError::Failed {
        what,
        message: e.to_string(),
    }
}

/// One changed path, as `status` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Relative to the worktree root, with forward slashes on every platform.
    pub path: String,
    /// `M`, `A`, `D`, `R` or `?`.
    pub state: String,
    /// Where it came from, when this is a rename.
    pub old_path: Option<String>,
}

/// One commit, as `log` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The abbreviated id.
    pub sha: String,
    /// The subject line.
    pub subject: String,
    /// Who wrote it.
    pub author: String,
    /// When, in unix seconds.
    pub time: i64,
}

/// One line of a blame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameLine {
    /// One-based, in the file as it is now.
    pub line: u32,
    /// The commit that last touched it.
    pub sha: String,
    /// Who wrote that commit.
    pub author: String,
}

/// Open a repository at or above `path`.
///
/// # Errors
///
/// [`GitError::NotARepository`] when there is none.
pub fn open(path: &Path) -> Result<gix::Repository, GitError> {
    gix::discover(path).map_err(|e| GitError::NotARepository {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

/// A path relative to the worktree, always with forward slashes.
///
/// Backslashes would make every snapshot and every model-visible answer
/// platform-dependent, and git itself has used forward slashes since before
/// Windows was a supported host.
fn rel(p: &BStr) -> String {
    p.to_str_lossy().replace('\\', "/")
}

/// What is different between HEAD, the index and the worktree.
///
/// # Errors
///
/// [`GitError`] when the repository is bare or gitoxide refuses the walk.
pub fn status(repo: &gix::Repository) -> Result<Vec<Change>, GitError> {
    use gix::status::index_worktree::Item as IwItem;
    use gix::status::plumbing::index_as_worktree::EntryStatus;
    use gix::status::Item;

    if repo.workdir().is_none() {
        return Err(GitError::NoWorktree { what: "status" });
    }
    let rewrites = gix::diff::Rewrites::default();
    let platform = repo
        .status(gix::progress::Discard)
        .map_err(fail("open the status platform"))?;
    let iter = platform
        .untracked_files(gix::status::UntrackedFiles::Files)
        .index_worktree_rewrites(rewrites)
        .tree_index_track_renames(gix::status::tree_index::TrackRenames::Given(rewrites))
        .into_iter(Vec::<BString>::new())
        .map_err(fail("walk the status"))?;

    let mut out: Vec<Change> = Vec::new();
    let mut push = |path: String, state: &str, old_path: Option<String>| {
        // The same path can be reported by both the tree↔index and the
        // index↔worktree halves. One row per path, with the first state that
        // claimed it, is what a person expects to read.
        if let Some(existing) = out.iter_mut().find(|c| c.path == path) {
            if existing.old_path.is_none() {
                existing.old_path = old_path;
            }
            return;
        }
        out.push(Change {
            path,
            state: state.to_owned(),
            old_path,
        });
    };

    for item in iter.flatten() {
        match item {
            Item::IndexWorktree(iw) => match iw {
                IwItem::Modification {
                    rela_path, status, ..
                } => {
                    // `NeedsUpdate` is "the stat cache is stale", not "the file
                    // changed". Reporting it would show clean files as dirty.
                    if matches!(status, EntryStatus::NeedsUpdate(_)) {
                        continue;
                    }
                    // A file that is gone from the worktree is a deletion, not a
                    // modification: the model's next move differs completely.
                    let state = match status {
                        EntryStatus::Change(
                            gix::status::plumbing::index_as_worktree::Change::Removed,
                        ) => "D",
                        _ => "M",
                    };
                    push(rel(rela_path.as_ref()), state, None);
                }
                IwItem::DirectoryContents { entry, .. } => {
                    let is_file = entry
                        .disk_kind
                        .is_some_and(|k| !matches!(k, gix::dir::entry::Kind::Directory));
                    if matches!(entry.status, gix::dir::entry::Status::Untracked) && is_file {
                        push(rel(entry.rela_path.as_ref()), "?", None);
                    }
                }
                IwItem::Rewrite {
                    source,
                    dirwalk_entry,
                    ..
                } => {
                    let old = match &source {
                        gix::status::index_worktree::RewriteSource::RewriteFromIndex {
                            source_rela_path,
                            ..
                        } => Some(rel(source_rela_path.as_ref())),
                        gix::status::index_worktree::RewriteSource::CopyFromDirectoryEntry {
                            ..
                        } => None,
                    };
                    push(rel(dirwalk_entry.rela_path.as_ref()), "R", old);
                }
            },
            Item::TreeIndex(change) => {
                let (path, state, old) = match &change {
                    gix::diff::index::Change::Addition { location, .. } => {
                        (rel(location.as_ref()), "A", None)
                    }
                    gix::diff::index::Change::Deletion { location, .. } => {
                        (rel(location.as_ref()), "D", None)
                    }
                    gix::diff::index::Change::Modification { location, .. } => {
                        (rel(location.as_ref()), "M", None)
                    }
                    gix::diff::index::Change::Rewrite {
                        location,
                        source_location,
                        ..
                    } => (
                        rel(location.as_ref()),
                        "R",
                        Some(rel(source_location.as_ref())),
                    ),
                };
                push(path, state, old);
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The history behind a revision, newest first.
///
/// # Errors
///
/// [`GitError`] when the revision names nothing, or the repository has no
/// commits at all.
pub fn log(repo: &gix::Repository, rev: Option<&str>, limit: usize) -> Result<Vec<Entry>, GitError> {
    let start = resolve(repo, rev)?;
    let walk = repo
        .rev_walk([start])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map_err(fail("walk the history"))?;

    Ok(walk
        .flatten()
        .take(limit)
        .filter_map(|info| info.object().ok())
        .map(|commit| Entry {
            sha: short(&commit.id),
            subject: first_line(commit.message_raw_sloppy()),
            author: commit
                .author()
                .map(|a| a.name.to_string())
                .unwrap_or_else(|_| "unknown".to_owned()),
            time: commit.time().map(|t| t.seconds).unwrap_or(0),
        })
        .collect())
}

/// One commit: who, when, what it said, and which paths it touched.
///
/// # Errors
///
/// [`GitError`] when the revision names nothing.
pub fn show(repo: &gix::Repository, rev: Option<&str>) -> Result<(Entry, Vec<Change>), GitError> {
    let id = resolve(repo, rev)?;
    let commit = repo.find_commit(id).map_err(fail("find the commit"))?;
    let entry = Entry {
        sha: short(&commit.id),
        subject: first_line(commit.message_raw_sloppy()),
        author: commit
            .author()
            .map(|a| a.name.to_string())
            .unwrap_or_else(|_| "unknown".to_owned()),
        time: commit.time().map(|t| t.seconds).unwrap_or(0),
    };
    let tree = commit.tree().map_err(fail("read the commit's tree"))?;
    let parent = commit
        .parent_ids()
        .next()
        .and_then(|p| repo.find_commit(p).ok())
        .and_then(|p| p.tree().ok());
    let changes = tree_diff(repo, parent.as_ref(), Some(&tree))?;
    Ok((entry, changes))
}

/// What changed between two revisions, or between one and the worktree's HEAD.
///
/// # Errors
///
/// [`GitError`] when either revision names nothing.
pub fn diff(
    repo: &gix::Repository,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<Change>, GitError> {
    let tree_of = |rev: Option<&str>| -> Result<gix::Tree<'_>, GitError> {
        let id = resolve(repo, rev)?;
        repo.find_commit(id)
            .map_err(fail("find the commit"))?
            .tree()
            .map_err(fail("read the commit's tree"))
    };
    let to_tree = tree_of(to)?;
    let from_tree = match from {
        Some(rev) => Some(tree_of(Some(rev))?),
        // No `from` means "against this commit's parent", which is what a
        // person means by `diff <rev>` far more often than "against nothing".
        None => {
            let id = resolve(repo, to)?;
            repo.find_commit(id)
                .ok()
                .and_then(|c| c.parent_ids().next())
                .and_then(|p| repo.find_commit(p).ok())
                .and_then(|p| p.tree().ok())
        }
    };
    tree_diff(repo, from_tree.as_ref(), Some(&to_tree))
}

/// Who last touched each line of a file.
///
/// # Errors
///
/// [`GitError`] when the revision names nothing or the path is not tracked.
pub fn blame(
    repo: &gix::Repository,
    path: &str,
    rev: Option<&str>,
) -> Result<Vec<BlameLine>, GitError> {
    let suspect = resolve(repo, rev)?;
    let out = repo
        .blame_file(
            BStr::new(path),
            suspect,
            gix::repository::blame_file::Options::default(),
        )
        .map_err(fail("blame"))?;

    let mut entries: Vec<&gix::blame::BlameEntry> = out.entries.iter().collect();
    entries.sort_by_key(|e| e.start_in_blamed_file);

    let mut lines = Vec::new();
    for entry in entries {
        let (sha, author) = match repo.find_commit(entry.commit_id) {
            Ok(commit) => (
                short(&commit.id),
                commit
                    .author()
                    .map(|a| a.name.to_string())
                    .unwrap_or_else(|_| "unknown".to_owned()),
            ),
            Err(_) => (short(&entry.commit_id), "unknown".to_owned()),
        };
        for offset in 0..entry.len.get() {
            lines.push(BlameLine {
                line: entry.start_in_blamed_file + offset + 1,
                sha: sha.clone(),
                author: author.clone(),
            });
        }
    }
    Ok(lines)
}

/// `rev`, or HEAD when there is none.
fn resolve(repo: &gix::Repository, rev: Option<&str>) -> Result<gix::ObjectId, GitError> {
    match rev {
        Some(rev) => repo
            .rev_parse_single(rev)
            .map(gix::Id::detach)
            .map_err(|_| GitError::NoSuchRevision(rev.to_owned())),
        None => repo
            .head_id()
            .map(gix::Id::detach)
            // A repository with no commits has no HEAD, and that is what this
            // says rather than "no such revision HEAD", which reads like a typo.
            .map_err(|e| GitError::Failed {
                what: "read HEAD",
                message: format!("{e} (a repository with no commits has no HEAD)"),
            }),
    }
}

fn tree_diff(
    repo: &gix::Repository,
    from: Option<&gix::Tree<'_>>,
    to: Option<&gix::Tree<'_>>,
) -> Result<Vec<Change>, GitError> {
    let changes = repo
        .diff_tree_to_tree(from, to, gix::diff::Options::default())
        .map_err(fail("diff the trees"))?;
    let mut out: Vec<Change> = changes
        .iter()
        .map(|change| {
            let (path, state, old) = match change {
                gix::object::tree::diff::ChangeDetached::Addition { location, .. } => {
                    (rel(location.as_ref()), "A", None)
                }
                gix::object::tree::diff::ChangeDetached::Deletion { location, .. } => {
                    (rel(location.as_ref()), "D", None)
                }
                gix::object::tree::diff::ChangeDetached::Modification { location, .. } => {
                    (rel(location.as_ref()), "M", None)
                }
                gix::object::tree::diff::ChangeDetached::Rewrite {
                    location,
                    source_location,
                    ..
                } => (
                    rel(location.as_ref()),
                    "R",
                    Some(rel(source_location.as_ref())),
                ),
            };
            Change {
                path,
                state: state.to_owned(),
                old_path: old,
            }
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Seven characters, which is what git itself abbreviates to by default.
fn short(id: &gix::oid) -> String {
    id.to_hex_with_len(7).to_string()
}

fn first_line(message: &BStr) -> String {
    message
        .to_str_lossy()
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}
