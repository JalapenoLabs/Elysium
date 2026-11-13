// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings/{id}/captures/{captureId}/transcript`: one capture of a meeting the
//! signed-in person may read, read exactly as it came, for its page's switch between the final
//! transcript and each capture's own.
//!
//! A capture's own is its source's lines, or Whisper's segments for a source that sent none
//! (`basedOn`); `null` while nothing was heard. Its times are moved onto the meeting's clock by
//! the capture's offset, the clock everything else on the page is on. `404` for a meeting they may
//! not read, or a capture that is not the meeting's.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::Need;
use crate::meetings::correction;
use crate::meetings::source::TranscriptLine;
use crate::models::{meeting_capture, meeting_transcription};
use crate::routes::v1::meetings::open;
use crate::state::AppState;
use crate::whisper::WhisperTranscript;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, capture_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (found, _access) = open(&mut connection, id, &current, Need::Read).await?;
    let capture = meeting_capture::find(&mut connection, capture_id).await?;
    if capture.meeting_id != found.id {
        return Err(ApiError::NotFound);
    }
    let source: Vec<TranscriptLine> = serde_json::from_value(capture.transcript)
        .context("the capture's transcript is not in the stored shape")?;

    // Whisper's transcript, several times the source's size, is read only for a source that
    // sent no lines, as a corrected copy follows it only then.
    let whisper = if source.is_empty() {
        meeting_transcription::find(&mut connection, capture.id)
            .await?
            .map(|transcription| {
                serde_json::from_value::<WhisperTranscript>(transcription.transcript)
            })
            .transpose()
            .context("the capture's Whisper transcript is not in the stored shape")?
    } else {
        None
    };
    let Some(heard) = correction::base(source, whisper.as_ref()) else {
        return Ok(Json(json!({ "transcript": null })));
    };
    let lines: Vec<TranscriptLine> = heard
        .lines
        .into_iter()
        .map(|line| TranscriptLine {
            start_ms: line.start_ms + capture.offset_ms,
            end_ms: line.end_ms + capture.offset_ms,
            ..line
        })
        .collect();
    Ok(Json(json!({
        "transcript": { "basedOn": heard.based_on.as_str(), "lines": lines },
    })))
}
