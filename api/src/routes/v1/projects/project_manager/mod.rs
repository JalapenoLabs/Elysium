// Copyright © 2026 Jalapeno Labs

//! `/api/v1/projects/{id}/project-manager`: a project's Project Manager, as its page shows it:
//! what it is doing, its latest turns with what woke each, and its batches; one turn with its
//! log; and a person writing to it. Its turns quote what was ingested, so these answer the
//! company's members alone, as the knowledge base does. See `docs/engine.md`, Project Managers.

pub mod get_project_manager;
pub mod get_turn;
pub mod send_message;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::models::project_manager::{Turn, TurnStatus, Wake, WakeKind};
use crate::models::work_batch::{BatchState, Priority, WorkBatch};
use crate::project_managers::Snapshot;
use crate::project_managers::status::Status;

/// A project's Project Manager, as its page shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManagerResponse {
    project_id: Uuid,
    status: Status,
    /// How many wakes wait for its next turn.
    waiting: i64,
    /// When a usage limit lets it start again, while one holds it.
    usage_paused_until: Option<DateTime<Utc>>,
    /// Its latest turns, newest first.
    turns: Vec<TurnResponse>,
    /// Its latest batches, newest first.
    batches: Vec<BatchResponse>,
}

impl From<Snapshot> for ProjectManagerResponse {
    fn from(snapshot: Snapshot) -> Self {
        let now = Utc::now();
        Self {
            project_id: snapshot.project_id,
            status: snapshot.status,
            waiting: snapshot.waiting,
            usage_paused_until: snapshot
                .state
                .and_then(|state| state.usage_paused_until)
                .filter(|until| *until > now),
            turns: snapshot
                .turns
                .into_iter()
                .map(|(turn, wakes)| TurnResponse::new(turn, wakes))
                .collect(),
            batches: snapshot
                .batches
                .into_iter()
                .map(BatchResponse::from)
                .collect(),
        }
    }
}

/// One turn, collapsed to its summary, with what woke it and what it answered.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnResponse {
    id: Uuid,
    status: TurnStatus,
    summary: Option<String>,
    reply: Option<String>,
    error: Option<String>,
    /// The record it put on the project's timeline.
    timeline_record_id: Option<Uuid>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    /// What woke it, oldest first.
    wakes: Vec<WakeResponse>,
}

impl TurnResponse {
    pub fn new(turn: Turn, wakes: Vec<Wake>) -> Self {
        Self {
            id: turn.id,
            status: turn.status,
            summary: turn.summary,
            reply: turn.reply,
            error: turn.error,
            timeline_record_id: turn.timeline_record_id,
            started_at: turn.started_at,
            finished_at: turn.finished_at,
            wakes: wakes.into_iter().map(WakeResponse::from).collect(),
        }
    }
}

/// One thing that woke a turn: a message with its text, a record, or a batch, by id.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeResponse {
    id: Uuid,
    kind: WakeKind,
    timeline_record_id: Option<Uuid>,
    work_batch_id: Option<Uuid>,
    body: Option<String>,
    /// Who caused it: the person or Elysia who wrote, or whoever attributed the record.
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl From<Wake> for WakeResponse {
    fn from(wake: Wake) -> Self {
        Self {
            id: wake.id,
            kind: wake.kind,
            timeline_record_id: wake.timeline_record_id,
            work_batch_id: wake.work_batch_id,
            body: wake.body,
            created_by: wake.created_by,
            created_at: wake.created_at,
        }
    }
}

/// One work batch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResponse {
    id: Uuid,
    title: String,
    goal: String,
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
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

impl From<WorkBatch> for BatchResponse {
    fn from(batch: WorkBatch) -> Self {
        Self {
            id: batch.id,
            title: batch.title,
            goal: batch.goal,
            repositories: batch.repositories,
            priority: batch.priority,
            scheduled_for: batch.scheduled_for,
            state: batch.state,
            result: batch.result,
            turn_id: batch.turn_id,
            source_record_ids: batch.source_record_ids,
            action_item_ids: batch.action_item_ids,
            timeline_record_id: batch.timeline_record_id,
            created_at: batch.created_at,
            started_at: batch.started_at,
            finished_at: batch.finished_at,
        }
    }
}
