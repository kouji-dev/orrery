use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Persisted shape — mirrors a row in the `projects` table exactly.
/// Only these fields live in SQLite. Everything else on `Project` is transient.
#[derive(Debug, Clone)]
pub struct ProjectRecord {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub icon: String,
    pub color: String,
}

/// View model sent to the frontend: the persisted record enriched with
/// transient attributes computed fresh at read time (never stored).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    // --- persisted ---
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub icon: String,
    pub color: String,
    // --- transient (computed in ProjectService::enrich) ---
    /// the project directory still exists on disk at `path`
    pub folder_exists: bool,
    /// `path` is (inside) a git repository right now
    pub has_git: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub branches: Vec<String>,
    /// The repo's default branch (origin/HEAD → main/master → current HEAD).
    /// Used as the pre-selected base when spawning an agent; `None` → callers
    /// fall back to the first entry in `branches`.
    pub default_branch: Option<String>,
}

/// A commit shaped for the frontend feed (matches the FE `Commit` model).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitView {
    pub agent: String,
    pub project_id: Uuid,
    pub sha: String,
    pub msg: String,
    pub when: String,
    pub ts: i64,
    pub files: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCreateRequest {
    pub name: String,
    pub path: String,
    pub icon: String,
    pub color: String,
    /// init a repo at create time if the path isn't already one (request-only, never persisted)
    pub with_git: bool,
    /// clone source: when set, the project is created by cloning this remote
    /// instead of registering a local folder — `path` becomes the clone
    /// destination, interpreted per `source_mode`
    #[serde(default)]
    pub source_url: Option<String>,
    /// how `path` is read when cloning (absent → `Root`)
    #[serde(default)]
    pub source_mode: Option<ClonePathMode>,
    /// shallow-clone depth; `Some(n)` fetches only the default branch at depth
    /// n (`--depth` implies `--single-branch`), `None` clones full history
    #[serde(default)]
    pub depth: Option<u32>,
}

/// How the destination path of a clone is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClonePathMode {
    /// `path` is a parent folder — the clone lands in `path/<repo-name>`.
    Root,
    /// `path` IS the project folder — the repo content lands directly in it
    /// (the `git clone <url> .` shape).
    Project,
}

/// Partial update — only the provided fields are written.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUpdateRequest {
    pub name: Option<String>,
    pub path: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
}
