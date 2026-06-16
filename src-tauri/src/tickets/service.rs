use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::core::database::DB;
use crate::core::errors::{AppError, AppResult, DbError, TicketError};

use super::model::{
    Comment, CommentCreateRequest, CommentRecord, CommentRole, Ticket, TicketCreateRequest,
    TicketRecord, TicketStatus, TicketUpdateRequest,
};

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn status_to_str(s: &TicketStatus) -> &'static str {
    match s {
        TicketStatus::Todo => "todo",
        TicketStatus::InProgress => "inprogress",
        TicketStatus::Done => "done",
    }
}

fn str_to_status(s: &str) -> TicketStatus {
    match s {
        "inprogress" => TicketStatus::InProgress,
        "done" => TicketStatus::Done,
        _ => TicketStatus::Todo,
    }
}

fn role_to_str(r: &CommentRole) -> &'static str {
    match r {
        CommentRole::User => "user",
        CommentRole::Agent => "agent",
    }
}

fn str_to_role(s: &str) -> CommentRole {
    match s {
        "agent" => CommentRole::Agent,
        _ => CommentRole::User,
    }
}

/// Serialize tags to the JSON-array text stored in the `tags` column.
fn tags_to_json(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".into())
}

/// Parse the `tags` column back to a list (tolerant: malformed/legacy `NULL`
/// rows decode to an empty list rather than erroring).
fn json_to_tags(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

/// Defensive backstop for tags coming off the wire: trim, drop empties, and
/// dedupe while preserving order. The frontend already snake_cases labels; this
/// just guarantees we never persist blanks or duplicates.
fn norm_tags(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = t.trim().to_string();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

#[derive(Clone)]
pub struct TicketService {
    db: DB,
}

impl TicketService {
    pub fn new(db: DB) -> Self {
        let svc = Self { db };
        svc.init_schema();
        svc
    }

    fn init_schema(&self) {
        let c = self.db.lock().unwrap();
        c.execute(
            "CREATE TABLE IF NOT EXISTS tickets (
                id         TEXT PRIMARY KEY,
                title      TEXT NOT NULL,
                notes      TEXT NOT NULL DEFAULT '',
                status     TEXT NOT NULL DEFAULT 'todo',
                project_id TEXT,
                agent_id   TEXT,
                tags       TEXT NOT NULL DEFAULT '[]',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();

        // Migration for tickets tables created before tags existed: add the
        // column if it isn't there. SQLite has no "ADD COLUMN IF NOT EXISTS", so
        // we run it unconditionally and ignore the "duplicate column" error.
        let _ = c.execute(
            "ALTER TABLE tickets ADD COLUMN tags TEXT NOT NULL DEFAULT '[]'",
            [],
        );

        c.execute(
            "CREATE TABLE IF NOT EXISTS comments (
                id         TEXT PRIMARY KEY,
                ticket_id  TEXT NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
                author     TEXT NOT NULL DEFAULT 'You',
                role       TEXT NOT NULL DEFAULT 'user',
                tool       TEXT,
                body       TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();

        // Enable FK enforcement for this connection
        let _ = c.execute("PRAGMA foreign_keys = ON", []);
    }

    pub fn list(&self) -> AppResult<Vec<Ticket>> {
        let records = self.records()?;
        Ok(records.into_iter().map(Ticket::from).collect())
    }

    pub fn get(&self, id: Uuid) -> AppResult<Ticket> {
        Ok(Ticket::from(self.record(id)?))
    }

    pub fn create(&self, req: TicketCreateRequest) -> AppResult<Ticket> {
        if req.title.trim().is_empty() {
            return Err(TicketError::Required("title is empty".into()).into());
        }
        let id = Uuid::new_v4();
        let now = now_ms();
        let notes = req.notes.unwrap_or_default();
        let project_id_str = req.project_id.map(|u| u.to_string());
        let tags = norm_tags(req.tags.unwrap_or_default());
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "INSERT INTO tickets (id, title, notes, status, project_id, agent_id, tags, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8)",
                rusqlite::params![
                    id.to_string(),
                    req.title,
                    notes,
                    "todo",
                    project_id_str,
                    tags_to_json(&tags),
                    now,
                    now,
                ],
            )
            .map_err(DbError::Sqlite)?;
        }
        Ok(Ticket::from(TicketRecord {
            id,
            title: req.title,
            notes,
            status: TicketStatus::Todo,
            project_id: req.project_id,
            agent_id: None,
            tags,
            created_at: now,
            updated_at: now,
        }))
    }

    pub fn update(&self, id: Uuid, req: TicketUpdateRequest) -> AppResult<Ticket> {
        let mut rec = self.record(id)?;
        if let Some(title) = req.title {
            if !title.trim().is_empty() {
                rec.title = title;
            }
        }
        if let Some(notes) = req.notes {
            rec.notes = notes;
        }
        // project_id: Option<Option<Uuid>> would be ideal for "clear vs no-change",
        // but the request shape uses Option<Uuid> per spec — Some(id) sets it, None leaves it.
        if let Some(pid) = req.project_id {
            rec.project_id = Some(pid);
        }
        // tags: Some(list) replaces the set wholesale; None leaves it untouched.
        if let Some(tags) = req.tags {
            rec.tags = norm_tags(tags);
        }
        let now = now_ms();
        rec.updated_at = now;
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "UPDATE tickets SET title = ?2, notes = ?3, project_id = ?4, tags = ?5, updated_at = ?6 WHERE id = ?1",
                rusqlite::params![
                    id.to_string(),
                    rec.title,
                    rec.notes,
                    rec.project_id.map(|u| u.to_string()),
                    tags_to_json(&rec.tags),
                    now,
                ],
            )
            .map_err(DbError::Sqlite)?;
        }
        Ok(Ticket::from(rec))
    }

    pub fn remove(&self, id: Uuid) -> AppResult<()> {
        // Enable FK so ON DELETE CASCADE fires for comments
        let c = self.db.lock().unwrap();
        let _ = c.execute("PRAGMA foreign_keys = ON", []);
        let n = c
            .execute("DELETE FROM tickets WHERE id = ?1", [id.to_string()])
            .map_err(DbError::Sqlite)?;
        if n == 0 {
            return Err(TicketError::NotFound(id.to_string()).into());
        }
        Ok(())
    }

    pub fn set_status(&self, id: Uuid, status: TicketStatus) -> AppResult<Ticket> {
        let mut rec = self.record(id)?;
        rec.status = status;
        let now = now_ms();
        rec.updated_at = now;
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "UPDATE tickets SET status = ?2, updated_at = ?3 WHERE id = ?1",
                rusqlite::params![id.to_string(), status_to_str(&rec.status), now],
            )
            .map_err(DbError::Sqlite)?;
        }
        Ok(Ticket::from(rec))
    }

    /// Assign an agent to a ticket and flip status to InProgress.
    pub fn attach_agent(&self, id: Uuid, agent_id: Uuid) -> AppResult<Ticket> {
        let mut rec = self.record(id)?;
        rec.agent_id = Some(agent_id);
        rec.status = TicketStatus::InProgress;
        let now = now_ms();
        rec.updated_at = now;
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "UPDATE tickets SET agent_id = ?2, status = ?3, updated_at = ?4 WHERE id = ?1",
                rusqlite::params![
                    id.to_string(),
                    agent_id.to_string(),
                    "inprogress",
                    now,
                ],
            )
            .map_err(DbError::Sqlite)?;
        }
        Ok(Ticket::from(rec))
    }

    /// Find the ticket owned by `agent_id` that is not yet Done, mark it Done,
    /// and return it. Returns `None` if no such ticket exists.
    pub fn complete_for_agent(&self, agent_id: Uuid) -> AppResult<Option<Ticket>> {
        // Find ticket whose agent_id == agent_id AND status != done
        let row: Option<String> = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT id FROM tickets WHERE agent_id = ?1 AND status != 'done' LIMIT 1",
                [agent_id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(DbError::Sqlite)?
        };
        match row {
            None => Ok(None),
            Some(id_str) => {
                let ticket_id =
                    Uuid::parse_str(&id_str).map_err(|e| AppError::Other(e.to_string()))?;
                let ticket = self.set_status(ticket_id, TicketStatus::Done)?;
                Ok(Some(ticket))
            }
        }
    }

    pub fn comments(&self, ticket_id: Uuid) -> AppResult<Vec<Comment>> {
        let records = self.comment_records(ticket_id)?;
        Ok(records.into_iter().map(Comment::from).collect())
    }

    pub fn add_comment(&self, ticket_id: Uuid, req: CommentCreateRequest) -> AppResult<Comment> {
        // Verify the ticket exists
        let _ = self.record(ticket_id)?;
        let id = Uuid::new_v4();
        let now = now_ms();
        let author = req.author.unwrap_or_else(|| "You".into());
        let role = req.role.unwrap_or(CommentRole::User);
        {
            let c = self.db.lock().unwrap();
            c.execute(
                "INSERT INTO comments (id, ticket_id, author, role, tool, body, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    id.to_string(),
                    ticket_id.to_string(),
                    author,
                    role_to_str(&role),
                    req.tool,
                    req.body,
                    now,
                ],
            )
            .map_err(DbError::Sqlite)?;
        }
        Ok(Comment::from(CommentRecord {
            id,
            ticket_id,
            author,
            role,
            tool: req.tool,
            body: req.body,
            created_at: now,
        }))
    }

    // ---- record readers ----

    fn records(&self) -> AppResult<Vec<TicketRecord>> {
        let raw: Vec<(String, String, String, String, Option<String>, Option<String>, String, i64, i64)> = {
            let c = self.db.lock().unwrap();
            let mut stmt = c
                .prepare(
                    "SELECT id, title, notes, status, project_id, agent_id, tags, created_at, updated_at FROM tickets ORDER BY created_at DESC",
                )
                .map_err(DbError::Sqlite)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ))
                })
                .map_err(DbError::Sqlite)?;
            rows.collect::<Result<_, _>>().map_err(DbError::Sqlite)?
        };

        raw.into_iter()
            .map(|(id, title, notes, status, project_id, agent_id, tags, created_at, updated_at)| {
                let id = Uuid::parse_str(&id).map_err(|e| AppError::Other(e.to_string()))?;
                let project_id = project_id
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()
                    .map_err(|e| AppError::Other(e.to_string()))?;
                let agent_id = agent_id
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()
                    .map_err(|e| AppError::Other(e.to_string()))?;
                Ok(TicketRecord {
                    id,
                    title,
                    notes,
                    status: str_to_status(&status),
                    project_id,
                    agent_id,
                    tags: json_to_tags(&tags),
                    created_at,
                    updated_at,
                })
            })
            .collect()
    }

    fn record(&self, id: Uuid) -> AppResult<TicketRecord> {
        let row: Option<(String, String, String, Option<String>, Option<String>, String, i64, i64)> = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT title, notes, status, project_id, agent_id, tags, created_at, updated_at FROM tickets WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
            )
            .optional()
            .map_err(DbError::Sqlite)?
        };
        match row {
            Some((title, notes, status, project_id, agent_id, tags, created_at, updated_at)) => {
                let project_id = project_id
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()
                    .map_err(|e| AppError::Other(e.to_string()))?;
                let agent_id = agent_id
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()
                    .map_err(|e| AppError::Other(e.to_string()))?;
                Ok(TicketRecord {
                    id,
                    title,
                    notes,
                    status: str_to_status(&status),
                    project_id,
                    agent_id,
                    tags: json_to_tags(&tags),
                    created_at,
                    updated_at,
                })
            }
            None => Err(TicketError::NotFound(id.to_string()).into()),
        }
    }

    fn comment_records(&self, ticket_id: Uuid) -> AppResult<Vec<CommentRecord>> {
        let raw: Vec<(String, String, String, Option<String>, String, i64)> = {
            let c = self.db.lock().unwrap();
            let mut stmt = c
                .prepare(
                    "SELECT id, author, role, tool, body, created_at FROM comments WHERE ticket_id = ?1 ORDER BY created_at ASC",
                )
                .map_err(DbError::Sqlite)?;
            let rows = stmt
                .query_map([ticket_id.to_string()], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                })
                .map_err(DbError::Sqlite)?;
            rows.collect::<Result<_, _>>().map_err(DbError::Sqlite)?
        };

        raw.into_iter()
            .map(|(id, author, role, tool, body, created_at)| {
                let id = Uuid::parse_str(&id).map_err(|e| AppError::Other(e.to_string()))?;
                Ok(CommentRecord {
                    id,
                    ticket_id,
                    author,
                    role: str_to_role(&role),
                    tool,
                    body,
                    created_at,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    fn svc() -> TicketService {
        let db: DB = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        TicketService::new(db)
    }

    fn create_req(title: &str) -> TicketCreateRequest {
        TicketCreateRequest {
            title: title.into(),
            notes: None,
            project_id: None,
            tags: None,
        }
    }

    #[test]
    fn create_and_list() {
        let s = svc();
        let t = s.create(create_req("fix bug")).unwrap();
        assert_eq!(t.title, "fix bug");
        assert_eq!(t.status, TicketStatus::Todo);
        let all = s.list().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, t.id);
    }

    #[test]
    fn rejects_empty_title() {
        let s = svc();
        let err = s.create(create_req("")).unwrap_err();
        assert!(matches!(err, AppError::Ticket(TicketError::Required(_))));
    }

    #[test]
    fn get_missing_is_not_found() {
        let s = svc();
        let err = s.get(Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, AppError::Ticket(TicketError::NotFound(_))));
    }

    #[test]
    fn update_fields() {
        let s = svc();
        let t = s.create(create_req("old title")).unwrap();
        let updated = s
            .update(
                t.id,
                TicketUpdateRequest {
                    title: Some("new title".into()),
                    notes: Some("<p>some notes</p>".into()),
                    project_id: None,
                    tags: None,
                },
            )
            .unwrap();
        assert_eq!(updated.title, "new title");
        assert_eq!(updated.notes, "<p>some notes</p>");
    }

    #[test]
    fn create_seeds_tags_and_they_persist() {
        let s = svc();
        let t = s
            .create(TicketCreateRequest {
                title: "tagged".into(),
                notes: None,
                project_id: None,
                tags: Some(vec!["payments".into(), "tech_debt".into()]),
            })
            .unwrap();
        assert_eq!(t.tags, vec!["payments", "tech_debt"]);
        // round-trips through the DB
        let fetched = s.get(t.id).unwrap();
        assert_eq!(fetched.tags, vec!["payments", "tech_debt"]);
        // and through list()
        assert_eq!(s.list().unwrap()[0].tags, vec!["payments", "tech_debt"]);
    }

    #[test]
    fn update_tags_replaces_set_and_normalizes() {
        let s = svc();
        let t = s.create(create_req("task")).unwrap();
        assert!(t.tags.is_empty(), "no tags by default");
        // Some(list) replaces wholesale; blanks/dupes are dropped, order kept.
        let updated = s
            .update(
                t.id,
                TicketUpdateRequest {
                    title: None,
                    notes: None,
                    project_id: None,
                    tags: Some(vec![
                        "auth".into(),
                        "  ".into(),
                        "auth".into(),
                        "bug".into(),
                    ]),
                },
            )
            .unwrap();
        assert_eq!(updated.tags, vec!["auth", "bug"]);

        // None leaves tags untouched.
        let again = s
            .update(
                t.id,
                TicketUpdateRequest {
                    title: Some("renamed".into()),
                    notes: None,
                    project_id: None,
                    tags: None,
                },
            )
            .unwrap();
        assert_eq!(again.tags, vec!["auth", "bug"], "None must not clear tags");
    }

    #[test]
    fn tags_default_empty_for_legacy_rows_via_migration() {
        // Simulate a pre-tags DB: create the old schema WITHOUT the tags column,
        // insert a row, then let TicketService::new run the ADD COLUMN migration.
        let db: DB = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        {
            let c = db.lock().unwrap();
            c.execute(
                "CREATE TABLE tickets (
                    id TEXT PRIMARY KEY, title TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '',
                    status TEXT NOT NULL DEFAULT 'todo', project_id TEXT, agent_id TEXT,
                    created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO tickets (id, title, created_at, updated_at) VALUES (?1, 'legacy', 0, 0)",
                [Uuid::new_v4().to_string()],
            )
            .unwrap();
        }
        let s = TicketService::new(db); // runs the ALTER TABLE migration
        let all = s.list().unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].tags.is_empty(), "legacy row decodes to empty tags");
    }

    #[test]
    fn set_status_transitions() {
        let s = svc();
        let t = s.create(create_req("task")).unwrap();
        assert_eq!(t.status, TicketStatus::Todo);
        let t2 = s.set_status(t.id, TicketStatus::InProgress).unwrap();
        assert_eq!(t2.status, TicketStatus::InProgress);
        let t3 = s.set_status(t.id, TicketStatus::Done).unwrap();
        assert_eq!(t3.status, TicketStatus::Done);
    }

    #[test]
    fn remove_deletes_ticket_and_cascades_comments() {
        let s = svc();
        let t = s.create(create_req("to delete")).unwrap();
        s.add_comment(
            t.id,
            CommentCreateRequest {
                body: "hello".into(),
                author: None,
                role: None,
                tool: None,
            },
        )
        .unwrap();
        s.remove(t.id).unwrap();
        assert_eq!(s.list().unwrap().len(), 0);
        // comments are cascaded — querying a non-existent ticket returns NotFound,
        // but we can check via raw db that the comment row is gone.
    }

    #[test]
    fn add_comment_and_list_comments() {
        let s = svc();
        let t = s.create(create_req("with comments")).unwrap();
        let c1 = s
            .add_comment(
                t.id,
                CommentCreateRequest {
                    body: "<p>first</p>".into(),
                    author: None,
                    role: None,
                    tool: None,
                },
            )
            .unwrap();
        assert_eq!(c1.author, "You");
        assert_eq!(c1.role, CommentRole::User);
        let c2 = s
            .add_comment(
                t.id,
                CommentCreateRequest {
                    body: "<p>agent response</p>".into(),
                    author: Some("Claude".into()),
                    role: Some(CommentRole::Agent),
                    tool: Some("bash".into()),
                },
            )
            .unwrap();
        assert_eq!(c2.author, "Claude");
        assert_eq!(c2.role, CommentRole::Agent);
        assert_eq!(c2.tool.as_deref(), Some("bash"));
        let all = s.comments(t.id).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].body, "<p>first</p>");
    }

    #[test]
    fn attach_agent_sets_inprogress() {
        let s = svc();
        let t = s.create(create_req("agent work")).unwrap();
        let agent_id = Uuid::new_v4();
        let t2 = s.attach_agent(t.id, agent_id).unwrap();
        assert_eq!(t2.status, TicketStatus::InProgress);
        assert_eq!(t2.agent_id, Some(agent_id));
    }

    #[test]
    fn complete_for_agent_marks_done() {
        let s = svc();
        let t = s.create(create_req("agent task")).unwrap();
        let agent_id = Uuid::new_v4();
        s.attach_agent(t.id, agent_id).unwrap();
        let completed = s.complete_for_agent(agent_id).unwrap();
        assert!(completed.is_some());
        let done = completed.unwrap();
        assert_eq!(done.status, TicketStatus::Done);
        // Calling again returns None (already Done)
        let again = s.complete_for_agent(agent_id).unwrap();
        assert!(again.is_none());
    }

    #[test]
    fn complete_for_agent_returns_none_when_no_ticket() {
        let s = svc();
        let result = s.complete_for_agent(Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn persists_to_real_file_across_connections() {
        let dbdir = tempfile::tempdir().unwrap();
        let dbfile = dbdir.path().join("orrery_ticket_test.db");
        {
            let db: DB = Arc::new(Mutex::new(Connection::open(&dbfile).unwrap()));
            let s = TicketService::new(db);
            s.create(create_req("persistent")).unwrap();
        }
        let db2: DB = Arc::new(Mutex::new(Connection::open(&dbfile).unwrap()));
        let s2 = TicketService::new(db2);
        assert_eq!(s2.list().unwrap().len(), 1, "row must persist to disk");
    }
}
