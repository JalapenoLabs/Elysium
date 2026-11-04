// Copyright © 2026 Jalapeno Labs

//! `/api/v1/promises`: the promises people made, which their projects' Project Managers keep, as
//! a follow-up item's page shows its own. A project's promises are listed with its Project Manager
//! (`/api/v1/projects/{id}/project-manager`). A promise quotes what was ingested, so these answer
//! the company's members alone, as the Project Manager's routes do. See `docs/engine.md`,
//! Follow-ups.

mod list_promises;

use axum::Router;
use axum::routing::get;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::models::promise::{Promise, PromiseState};
use crate::models::timeline_record::TimelineRecord;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_promises::handle))
}

/// A promise, with where it was made.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromiseResponse {
    id: Uuid,
    project_id: Uuid,
    state: PromiseState,
    /// Who it is for, by name.
    recipient: String,
    /// What was promised, as a short phrase.
    summary: String,
    /// The message it was made in; `null` once that message is gone.
    made_in: Option<MadeIn>,
    /// The work it waits on, at most one.
    work_batch_id: Option<Uuid>,
    work_item_id: Option<Uuid>,
    /// When its work finished.
    due_at: Option<DateTime<Utc>>,
    /// The item its follow-up is, once drafted.
    follow_up_item_id: Option<Uuid>,
    /// Its follow-up's draft, once drafted, which only its person may read and send.
    draft_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Where a promise was made: the message, its conversation as its title names it, and whose
/// timeline it is on, the person who made it and sends its follow-up.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MadeIn {
    record_id: Uuid,
    source_kind: String,
    title: String,
    url: Option<String>,
    occurred_at: DateTime<Utc>,
    person_id: Uuid,
}

impl PromiseResponse {
    /// `promise` as clients see it, with `record`, the message it was made in, when it was read,
    /// and the id of its follow-up's draft, once drafted.
    pub fn new(promise: Promise, record: Option<&TimelineRecord>, draft_id: Option<Uuid>) -> Self {
        Self {
            id: promise.id,
            project_id: promise.project_id,
            state: promise.state,
            recipient: promise.recipient,
            summary: promise.summary,
            made_in: record.map(|record| MadeIn {
                record_id: record.id,
                source_kind: record.source_kind.clone(),
                title: record.title.clone(),
                url: record.url.clone(),
                occurred_at: record.occurred_at,
                person_id: record.created_by,
            }),
            work_batch_id: promise.work_batch_id,
            work_item_id: promise.work_item_id,
            due_at: promise.due_at,
            follow_up_item_id: promise.follow_up_item_id,
            draft_id,
            created_at: promise.created_at,
        }
    }
}
