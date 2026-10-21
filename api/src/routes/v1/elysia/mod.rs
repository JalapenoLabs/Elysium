// Copyright © 2026 Jalapeno Labs

//! `/api/v1/elysia`: talking to Elysia, answering her questions, and pausing or scheduling
//! her. See `crate::elysia` and `docs/elysia.md`.

mod answer_question;
mod cancel_turn;
mod dismiss_question;
mod get_elysia;
mod list_messages;
mod list_turns;
mod send_message;
mod update_settings;

use axum::Router;
use axum::routing::{get, patch, post};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::elysia::availability::AvailabilityWindow;
use crate::elysia::status::{Hold, Light};
use crate::elysia::{self, Snapshot};
use crate::errors::ApiError;
use crate::models::elysia::{
    Message, Question, QuestionStatus, Turn, TurnStatus, Work, WorkStatus,
};
use crate::realtime::{EventBus, ServerEvent};

pub fn router() -> Router<crate::state::AppState> {
    Router::new()
        .route("/", get(get_elysia::handle))
        .route("/settings", patch(update_settings::handle))
        .route(
            "/messages",
            get(list_messages::handle).post(send_message::handle),
        )
        .route("/turns", get(list_turns::handle))
        .route("/turns/{id}/cancel", post(cancel_turn::handle))
        .route("/questions/{id}/answer", post(answer_question::handle))
        .route("/questions/{id}/dismiss", post(dismiss_question::handle))
}

/// Elysia at a glance, as clients see her.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElysiaResponse {
    light: Light,
    hold: Option<Hold>,
    paused: bool,
    availability: Vec<AvailabilityWindow>,
    usage_paused_until: Option<DateTime<Utc>>,
    /// Queued and running turns, oldest first.
    active_turns: Vec<TurnResponse>,
    open_questions: Vec<QuestionResponse>,
    /// Everything she has taken on and not finished, most recently changed first.
    work: Vec<WorkResponse>,
    updated_by: Uuid,
    updated_at: DateTime<Utc>,
}

impl From<Snapshot> for ElysiaResponse {
    fn from(snapshot: Snapshot) -> Self {
        Self {
            light: snapshot.status.light,
            hold: snapshot.status.hold,
            paused: snapshot.state.paused,
            availability: snapshot.availability,
            usage_paused_until: snapshot.state.usage_paused_until,
            active_turns: snapshot
                .active_turns
                .into_iter()
                .map(TurnResponse::from)
                .collect(),
            open_questions: snapshot
                .open_questions
                .into_iter()
                .map(QuestionResponse::from)
                .collect(),
            work: snapshot.work.into_iter().map(WorkResponse::from).collect(),
            updated_by: snapshot.state.updated_by,
            updated_at: snapshot.state.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageResponse {
    id: Uuid,
    body: String,
    /// Whether Elysia wrote it, rather than a person.
    from_elysia: bool,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl From<Message> for MessageResponse {
    fn from(message: Message) -> Self {
        Self {
            from_elysia: message.is_from_elysia(),
            id: message.id,
            body: message.body,
            created_by: message.created_by,
            created_at: message.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnResponse {
    id: Uuid,
    trigger_kind: String,
    trigger_detail: Value,
    status: TurnStatus,
    summary: Option<String>,
    error: Option<String>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

impl From<Turn> for TurnResponse {
    fn from(turn: Turn) -> Self {
        Self {
            id: turn.id,
            trigger_kind: turn.trigger_kind,
            trigger_detail: turn.trigger_detail,
            status: turn.status,
            summary: turn.summary,
            error: turn.error,
            created_by: turn.created_by,
            created_at: turn.created_at,
            started_at: turn.started_at,
            finished_at: turn.finished_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionResponse {
    id: Uuid,
    prompt: String,
    options: Value,
    status: QuestionStatus,
    answer: Option<String>,
    action_item_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    answered_by: Option<Uuid>,
    answered_at: Option<DateTime<Utc>>,
}

impl From<Question> for QuestionResponse {
    fn from(question: Question) -> Self {
        Self {
            id: question.id,
            prompt: question.prompt,
            options: question.options,
            status: question.status,
            answer: question.answer,
            action_item_id: question.action_item_id,
            created_at: question.created_at,
            answered_by: question.answered_by,
            answered_at: question.answered_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkResponse {
    id: Uuid,
    title: String,
    detail: String,
    status: WorkStatus,
    ready_at: Option<DateTime<Utc>>,
    action_item_id: Option<Uuid>,
    revisits: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Work> for WorkResponse {
    fn from(work: Work) -> Self {
        Self {
            id: work.id,
            title: work.title,
            detail: work.detail,
            status: work.status,
            ready_at: work.ready_at,
            action_item_id: work.action_item_id,
            revisits: work.revisits,
            created_at: work.created_at,
            updated_at: work.updated_at,
        }
    }
}

/// Reads Elysia's snapshot and announces it. Called after anything that changes what the
/// interface shows at a glance: a turn queued or ending, a question asked or closed, her work
/// moving, or her settings changing.
///
/// # Errors
/// Propagates any database error.
pub async fn publish_snapshot(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
) -> Result<ElysiaResponse, ApiError> {
    let response = ElysiaResponse::from(elysia::snapshot(connection).await?);
    events.publish(&ServerEvent::ElysiaUpdated(Box::new(response.clone())));
    Ok(response)
}
