//! Git result types — the frontend contract. Library-agnostic: every
//! backend (gitoxide) produces exactly these shapes, and the
//! Tauri commands serialize them unchanged.

use std::path::Path;

use serde::Serialize;

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

pub const MAIN_CHECKOUT: &str = "";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    /// Checked out in the project's main checkout (HEAD there).
    pub current: bool,
    /// Worktree NAME holding this branch ("" = the main checkout, None = free).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_out_in: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}


/// One changed region of the working file vs HEAD (context 0 — exact lines).
/// `new_lines == 0` marks a pure deletion AFTER `new_start`; `old_lines == 0`
/// a pure insertion. 1-based starts, git hunk-header convention.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
}

