// Copyright © 2026 Jalapeno Labs

//! `/api/v1/projects/{id}/project-manager`: a project's Project Manager, as its page shows it:
//! what it is doing, its latest turns with what woke each, its batches, and its promises; one
//! turn with its
//! log; and a person writing to it. Its turns quote what was ingested, so these answer the
//! company's members alone, as the knowledge base does. See `docs/engine.md`, Project Managers.

pub mod get_project_manager;
pub mod get_turn;
pub mod send_message;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::models::project_manager::{Turn, TurnStatus, Wake, WakeKind};
use crate::project_managers::Snapshot;
use crate::project_managers::status::Status;
use crate::routes::v1::promises::PromiseResponse;
use crate::routes::v1::work_batches::BatchResponse;

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
    /// Its latest promises, newest first.
    promises: Vec<PromiseResponse>,
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
            promises: snapshot
                .promises
                .into_iter()
                .map(|(promise, record, draft_id)| {
                    PromiseResponse::new(promise, record.as_ref(), draft_id)
                })
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

/// One thing that woke a turn: a message with its text, a record, a batch, or a promise, by id.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeResponse {
    id: Uuid,
    kind: WakeKind,
    timeline_record_id: Option<Uuid>,
    work_batch_id: Option<Uuid>,
    promise_id: Option<Uuid>,
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
            promise_id: wake.promise_id,
            body: wake.body,
            created_by: wake.created_by,
            created_at: wake.created_at,
        }
    }
}
