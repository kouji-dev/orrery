use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Persisted shape — mirrors a row in the `agents` table.
#[derive(Debug, Clone)]
pub struct AgentRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub tool: String,
    pub model: String,
    pub effort: Option<String>,
    pub name: String,
    pub task: String,
    pub status: String,
    pub branch: String,
    pub worktree: String,
    pub base: String,
    /// True once the agent has been launched at least once — gates the one-time
    /// delivery of the initial task prompt (a restart/resume must not re-run it).
    pub started: bool,
    /// The tool's own CLI session id (captured from a hook's `session_id`), used to
    /// relaunch with `claude --resume <id>`. `None` until a hook reports it.
    pub session_id: Option<String>,
    /// The ticket this agent is working on, if any. Set at spawn time; drives
    /// lifecycle: attach_agent on spawn, complete_for_agent on PTY exit.
    pub ticket_id: Option<Uuid>,
}

/// View model sent to the frontend: persisted record + transient runtime fields.
/// The runtime fields stay defaulted until the agent runtime exists (task #7).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    // --- persisted ---
    pub id: Uuid,
    pub project_id: Uuid,
    pub tool: String,
    pub model: String,
    pub effort: Option<String>,
    pub name: String,
    pub task: String,
    pub status: String,
    pub branch: String,
    pub worktree: String,
    pub base: String,
    pub started: bool,
    pub session_id: Option<String>,
    /// The ticket this agent is working on (`ticketId` in camelCase for the frontend).
    pub ticket_id: Option<Uuid>,
    // --- transient runtime (defaulted; owned by the runtime layer later) ---
    pub commits: i64,
    pub elapsed: i64,
    pub progress: f64,
    pub pending: Vec<Value>,
    pub block_reason: Option<String>,
    pub wait_reason: Option<String>,
}

/// Spawn request — the runtime-derived fields (branch, worktree, status) are set server-side.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSpawnRequest {
    /// Client-chosen id (optional). The sidebar shows a placeholder row under
    /// this id the moment Create is clicked, so the real row that arrives on
    /// `agent://created` replaces it in place instead of appearing next to it.
    #[serde(default)]
    pub id: Option<Uuid>,
    pub project_id: Uuid,
    pub tool: String,
    pub model: String,
    pub effort: Option<String>,
    pub name: String,
    pub task: String,
    pub base: String,
    /// Optional ticket to attach this agent to. When Some, attach_agent is called
    /// after spawn so the ticket flips to InProgress and the board updates.
    pub ticket_id: Option<Uuid>,
}

/// Partial update — only provided fields are written.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUpdateRequest {
    pub status: Option<String>,
    pub task: Option<String>,
    pub model: Option<String>,
    pub name: Option<String>,
}
