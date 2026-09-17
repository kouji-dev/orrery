use tauri::{AppHandle, Runtime, State};
use uuid::Uuid;

use crate::core::errors::AppResult;
use crate::core::events::{emit_entity, Change};

use super::model::{Comment, CommentCreateRequest, Ticket, TicketCreateRequest, TicketStatus, TicketUpdateRequest};
use super::service::TicketService;

#[tauri::command(async)]
pub fn ticket_list(svc: State<'_, TicketService>) -> AppResult<Vec<Ticket>> {
    crate::perf::timed("ticket_list", || svc.list())
}

#[tauri::command(async)]
pub fn ticket_create<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, TicketService>,
    req: TicketCreateRequest,
) -> AppResult<Ticket> {
    crate::perf::timed("ticket_create", || {
        let ticket = svc.create(req)?;
        emit_entity(&app, "ticket", Change::Created, ticket.clone());
        Ok(ticket)
    })
}

#[tauri::command(async)]
pub fn ticket_update<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, TicketService>,
    id: Uuid,
    req: TicketUpdateRequest,
) -> AppResult<Ticket> {
    crate::perf::timed("ticket_update", || {
        let ticket = svc.update(id, req)?;
        emit_entity(&app, "ticket", Change::Updated, ticket.clone());
        Ok(ticket)
    })
}

#[tauri::command(async)]
pub fn ticket_remove<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, TicketService>,
    id: Uuid,
) -> AppResult<()> {
    crate::perf::timed("ticket_remove", || {
        svc.remove(id)?;
        emit_entity(
            &app,
            "ticket",
            Change::Deleted,
            serde_json::json!({ "id": id }),
        );
        Ok(())
    })
}

#[tauri::command(async)]
pub fn ticket_set_status<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, TicketService>,
    id: Uuid,
    status: TicketStatus,
) -> AppResult<Ticket> {
    crate::perf::timed("ticket_set_status", || {
        let ticket = svc.set_status(id, status)?;
        emit_entity(&app, "ticket", Change::Updated, ticket.clone());
        Ok(ticket)
    })
}

#[tauri::command(async)]
pub fn comment_list(
    svc: State<'_, TicketService>,
    ticket_id: Uuid,
) -> AppResult<Vec<Comment>> {
    crate::perf::timed("comment_list", || svc.comments(ticket_id))
}

#[tauri::command(async)]
pub fn comment_add<R: Runtime>(
    app: AppHandle<R>,
    svc: State<'_, TicketService>,
    ticket_id: Uuid,
    req: CommentCreateRequest,
) -> AppResult<Comment> {
    crate::perf::timed("comment_add", || {
        let comment = svc.add_comment(ticket_id, req)?;
        emit_entity(&app, "comment", Change::Created, comment.clone());
        Ok(comment)
    })
}
