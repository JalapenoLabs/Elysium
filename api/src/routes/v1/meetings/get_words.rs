// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings/{id}/words`: a meeting's final transcript as it reads, for its page, to
//! anyone who may read the meeting: its standing words in order, every standing mark over them, and its
//! captures, each of which opens as it came (`captures/{captureId}/transcript`) and plays its
//! recording (`captures/{captureId}/media`).
//!
//! `words` is empty until the transcript is built, which its correction does
//! (`docs/meetings.md`, The final transcript). `404` for a meeting they may not read.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{MarkResponse, WordResponse, open};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::Need;
use crate::meetings::media::media_type_of;
use crate::models::meeting_capture::{self, CaptureSummary};
use crate::models::{meeting_mark, meeting_word};
use crate::state::AppState;

/// One capture of the meeting, as its page names it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureResponse {
    id: Uuid,
    /// The connector that read it, or `null` once that connector is removed.
    connector_id: Option<Uuid>,
    source_kind: String,
    /// Its page at its source.
    url: Option<String>,
    /// Where its clock sits on the meeting's.
    offset_ms: i64,
    has_recording: bool,
    /// What its recording is, such as `video/mp4`, which the player's switcher starts on the best
    /// video by; `null` without a recording, or one of a kind Elysium never writes.
    recording_content_type: Option<String>,
}

impl From<CaptureSummary> for CaptureResponse {
    fn from(capture: CaptureSummary) -> Self {
        // A recording kept before its type was recorded is named by its extension, as its media
        // route serves it.
        let recording_content_type = capture.recording_path.as_deref().and_then(|path| {
            capture
                .recording_content_type
                .clone()
                .or_else(|| media_type_of(path).map(ToOwned::to_owned))
        });
        Self {
            id: capture.id,
            connector_id: capture.connector_id,
            source_kind: capture.source_kind,
            url: capture.url,
            offset_ms: capture.offset_ms,
            has_recording: capture.recording_path.is_some(),
            recording_content_type,
        }
    }
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (found, _access) = open(&mut connection, id, &current, Need::Read).await?;
    // One snapshot, so every mark answered is over words answered, however an edit lands
    // meanwhile.
    let (words, marks, captures) = connection
        .build_transaction()
        .read_only()
        .repeatable_read()
        .run(async |connection| {
            let words = meeting_word::standing(connection, found.id).await?;
            let marks = meeting_mark::standing(connection, found.id).await?;
            let captures = meeting_capture::summaries_of(connection, found.id).await?;
            diesel::QueryResult::Ok((words, marks, captures))
        })
        .await?;
    let words: Vec<WordResponse> = words.into_iter().map(WordResponse::from).collect();
    let marks: Vec<MarkResponse> = marks.into_iter().map(MarkResponse::from).collect();
    let captures: Vec<CaptureResponse> = captures.into_iter().map(CaptureResponse::from).collect();
    Ok(Json(
        json!({ "words": words, "marks": marks, "captures": captures }),
    ))
}
