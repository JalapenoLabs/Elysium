// Copyright © 2026 Jalapeno Labs

//! `/api/v1/work-batches`: one work batch with why it exists, its history, its worker's log, and
//! the pull requests its worker recorded, and what people do to it while it waits: approve it,
//! edit and approve it, or reject it while it is proposed, then run it next, change its priority,
//! pause or resume it, and cancel it. A batch's goal quotes what was ingested, so every route
//! answers the company's members alone, as a project's Project Manager does; those same members
//! are the only ones who decide on a proposal, since no machine user is one. See
//! `docs/engine.md`, Workers and the pool.
//!
//! Each change goes through `crate::worker_pool::actions`, as Elysia's and a Project Manager's
//! do, so it is recorded in the batch's history with who made it, announced, and signalled to
//! the engine's pool.

mod approve_work_batch;
mod cancel_work_batch;
mod get_pull_requests;
mod get_work_batch;
mod get_worker_log;
mod reject_work_batch;
#[cfg(test)]
mod tests;
mod update_work_batch;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::models::action_item::ActionItemState;
use crate::models::work_batch::why::{self, Why};
use crate::models::work_batch::worker::Wait;
use crate::models::work_batch::{self, BatchEvent, BatchState, Priority, WorkBatch};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/{id}",
            get(get_work_batch::handle).patch(update_work_batch::handle),
        )
        .route("/{id}/approve", post(approve_work_batch::handle))
        .route("/{id}/reject", post(reject_work_batch::handle))
        .route("/{id}/cancel", post(cancel_work_batch::handle))
        .route("/{id}/worker-log", get(get_worker_log::handle))
        .route("/{id}/pull-requests", get(get_pull_requests::handle))
}

/// One work batch, whole.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResponse {
    id: Uuid,
    project_id: Uuid,
    title: String,
    goal: String,
    /// The goal in its project's brief it serves, as the brief spelled it; `null` for a batch from
    /// before briefs.
    serves: Option<String>,
    /// `[{ url, baseBranch? }]`.
    repositories: Value,
    priority: Priority,
    scheduled_for: Option<DateTime<Utc>>,
    state: BatchState,
    result: Option<String>,
    /// The turn that submitted it.
    turn_id: Option<Uuid>,
    source_record_ids: Value,
    action_item_ids: Value,
    timeline_record_id: Option<Uuid>,
    /// While someone asked for it to run next.
    pinned_at: Option<DateTime<Utc>>,
    /// When the pool gave it a slot.
    assigned_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    /// What its worker waits on while it keeps its container: `review`, `ci`, or `question`.
    waiting_on: Option<Wait>,
    /// What it waits for, in its worker's words.
    waiting_detail: Option<String>,
    /// When its worker looks again, unless a question is answered first.
    waiting_until: Option<DateTime<Utc>>,
}

impl From<WorkBatch> for BatchResponse {
    fn from(batch: WorkBatch) -> Self {
        Self {
            id: batch.id,
            project_id: batch.project_id,
            title: batch.title,
            goal: batch.goal,
            serves: batch.serves,
            repositories: batch.repositories,
            priority: batch.priority,
            scheduled_for: batch.scheduled_for,
            state: batch.state,
            result: batch.result,
            turn_id: batch.turn_id,
            source_record_ids: batch.source_record_ids,
            action_item_ids: batch.action_item_ids,
            timeline_record_id: batch.timeline_record_id,
            pinned_at: batch.pinned_at,
            assigned_at: batch.assigned_at,
            created_at: batch.created_at,
            started_at: batch.started_at,
            finished_at: batch.finished_at,
            waiting_on: batch.waiting_on,
            waiting_detail: batch.waiting_detail,
            waiting_until: batch.waiting_until,
        }
    }
}

/// One entry of a batch's history.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchEventResponse {
    id: Uuid,
    /// `submitted`, `held_for_approval`, `edited`, `approved`, `rejected`, `pinned`,
    /// `unpinned`, `reprioritized`, `paused`, `resumed`, `cancelled`, `assigned`, `unassigned`,
    /// `started`, `waiting`, `answered`, or `finished`.
    kind: String,
    data: Value,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl From<BatchEvent> for BatchEventResponse {
    fn from(event: BatchEvent) -> Self {
        Self {
            id: event.id,
            kind: event.kind,
            data: event.data,
            created_by: event.created_by,
            created_at: event.created_at,
        }
    }
}

/// What called for a batch: the timeline records named as its cause, each with the knowledge
/// base's document a member opens it by, and the action items it works on.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhyResponse {
    records: Vec<CauseResponse>,
    action_items: Vec<ItemResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CauseResponse {
    id: Uuid,
    source_kind: String,
    title: String,
    occurred_at: DateTime<Utc>,
    /// Null until the knowledge base has indexed it.
    document_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemResponse {
    id: Uuid,
    title: String,
    state: ActionItemState,
}

impl From<Why> for WhyResponse {
    fn from(why: Why) -> Self {
        Self {
            records: why
                .causes
                .into_iter()
                .map(|cause| CauseResponse {
                    id: cause.record_id,
                    source_kind: cause.source_kind,
                    title: cause.title,
                    occurred_at: cause.occurred_at,
                    document_id: cause.document_id,
                })
                .collect(),
            action_items: why
                .items
                .into_iter()
                .map(|item| ItemResponse {
                    id: item.id,
                    title: item.title,
                    state: item.state,
                })
                .collect(),
        }
    }
}

/// Batch `batch` with why it exists and its history, as every route here answers it.
async fn with_history(
    connection: &mut AsyncPgConnection,
    batch: WorkBatch,
) -> diesel::QueryResult<Value> {
    let history: Vec<BatchEventResponse> = work_batch::history(connection, batch.id)
        .await?
        .into_iter()
        .map(BatchEventResponse::from)
        .collect();
    let why = WhyResponse::from(why::read(connection, &batch).await?);
    Ok(json!({ "batch": BatchResponse::from(batch), "why": why, "history": history }))
}
