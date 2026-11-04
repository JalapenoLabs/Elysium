// Copyright © 2026 Jalapeno Labs

//! `/api/v1/drafts`: the signed-in person's drafts, and sending them as themselves. Each route
//! calls `crate::drafts`, which every send goes through. See `docs/connectors.md`, Sending.

mod create_draft;
mod delete_draft;
mod get_draft;
mod list_drafts;
mod send_draft;
mod update_draft;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::models::draft::{Draft, DraftState, ReplyPlace};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_drafts::handle).post(create_draft::handle))
        .route(
            "/{id}",
            get(get_draft::handle)
                .patch(update_draft::handle)
                .delete(delete_draft::handle),
        )
        .route("/{id}/send", post(send_draft::handle))
}

/// A draft as its writer sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftResponse {
    id: Uuid,
    reply_to_record_id: Option<Uuid>,
    connector_id: Option<Uuid>,
    mail_account_id: Option<Uuid>,
    /// Where it goes, in its sender's own terms; the web app shows a mail's recipients and
    /// subject from it.
    target: Value,
    place: Option<ReplyPlace>,
    body: String,
    state: DraftState,
    last_error: Option<String>,
    attempted_at: Option<DateTime<Utc>>,
    sent_at: Option<DateTime<Utc>>,
    sent_record_id: Option<Uuid>,
    /// The promise it keeps, for a follow-up a Project Manager drafted, which its item's page
    /// sends rather than the record's reply box.
    promise_id: Option<Uuid>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Draft> for DraftResponse {
    fn from(draft: Draft) -> Self {
        Self {
            id: draft.id,
            reply_to_record_id: draft.reply_to_record_id,
            connector_id: draft.connector_id,
            mail_account_id: draft.mail_account_id,
            target: draft.target,
            place: draft.place,
            body: draft.body,
            state: draft.state,
            last_error: draft.last_error,
            attempted_at: draft.attempted_at,
            sent_at: draft.sent_at,
            sent_record_id: draft.sent_record_id,
            promise_id: draft.promise_id,
            created_by: draft.created_by,
            created_at: draft.created_at,
            updated_at: draft.updated_at,
        }
    }
}
