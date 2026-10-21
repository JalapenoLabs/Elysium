// Copyright © 2026 Jalapeno Labs

//! `/api/v1/meetings`: the signed-in person's meetings and how far each one's processing got.
//! See `crate::meetings` and `docs/meetings.md`.
//!
//! A meeting belongs to whoever owns the account it was read from, and every route answers
//! them alone. Transcripts are not served here: what a meeting said reaches the timeline as
//! its segments.

mod get_meeting;
mod list_meetings;
mod retry_meeting;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::models::meeting::Meeting;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_meetings::handle))
        .route("/{id}", get(get_meeting::handle))
        .route("/{id}/retry", post(retry_meeting::handle))
}

/// A meeting as its owner sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingResponse {
    id: Uuid,
    connector_account_id: Option<Uuid>,
    source_kind: String,
    title: String,
    started_at: DateTime<Utc>,
    duration_ms: Option<i64>,
    organizer: String,
    participants: Value,
    url: Option<String>,
    /// `pending`, `recorded`, `segmented`, `done`, or `failed`.
    state: String,
    attempts: i32,
    /// Why the last attempt failed, or `null` when it succeeded.
    last_error: Option<String>,
    has_recording: bool,
    /// How many segments Elysia found, or `null` before she has read it.
    segment_count: Option<usize>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl MeetingResponse {
    pub fn new(meeting: Meeting) -> Self {
        Self {
            segment_count: meeting
                .segments
                .as_ref()
                .and_then(Value::as_array)
                .map(Vec::len),
            has_recording: meeting.recording_path.is_some(),
            id: meeting.id,
            connector_account_id: meeting.connector_account_id,
            source_kind: meeting.source_kind,
            title: meeting.title,
            started_at: meeting.started_at,
            duration_ms: meeting.duration_ms,
            organizer: meeting.organizer,
            participants: meeting.participants,
            url: meeting.url,
            state: meeting.state,
            attempts: meeting.attempts,
            last_error: meeting.last_error,
            created_at: meeting.created_at,
            updated_at: meeting.updated_at,
        }
    }
}
