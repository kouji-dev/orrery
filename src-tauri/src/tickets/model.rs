use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TicketStatus {
    Todo,
    #[serde(rename = "inprogress")]
    InProgress,
    Done,
}

impl Default for TicketStatus {
    fn default() -> Self {
        TicketStatus::Todo
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CommentRole {
    User,
    Agent,
}

impl Default for CommentRole {
    fn default() -> Self {
        CommentRole::User
    }
}

/// Persisted shape — mirrors a row in the `tickets` table exactly.
#[derive(Debug, Clone)]
pub struct TicketRecord {
    pub id: Uuid,
    pub title: String,
    pub notes: String,
    pub status: TicketStatus,
    pub project_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// View model sent to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticket {
    pub id: Uuid,
    pub title: String,
    pub notes: String,
    pub status: TicketStatus,
    pub project_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<TicketRecord> for Ticket {
    fn from(r: TicketRecord) -> Self {
        Ticket {
            id: r.id,
            title: r.title,
            notes: r.notes,
            status: r.status,
            project_id: r.project_id,
            agent_id: r.agent_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Persisted shape — mirrors a row in the `comments` table exactly.
#[derive(Debug, Clone)]
pub struct CommentRecord {
    pub id: Uuid,
    pub ticket_id: Uuid,
    pub author: String,
    pub role: CommentRole,
    pub tool: Option<String>,
    pub body: String,
    pub created_at: i64,
}

/// View model sent to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: Uuid,
    pub ticket_id: Uuid,
    pub author: String,
    pub role: CommentRole,
    pub tool: Option<String>,
    pub body: String,
    pub created_at: i64,
}

impl From<CommentRecord> for Comment {
    fn from(r: CommentRecord) -> Self {
        Comment {
            id: r.id,
            ticket_id: r.ticket_id,
            author: r.author,
            role: r.role,
            tool: r.tool,
            body: r.body,
            created_at: r.created_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketCreateRequest {
    pub title: String,
    pub notes: Option<String>,
    pub project_id: Option<Uuid>,
}

/// Partial update — only the provided fields are written.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketUpdateRequest {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentCreateRequest {
    pub body: String,
    pub author: Option<String>,
    pub role: Option<CommentRole>,
    pub tool: Option<String>,
}
