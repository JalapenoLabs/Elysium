// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings/{id}/transcript`: one of the signed-in person's meetings' transcript as
//! it was heard, and its corrected copy once there is one, line for line, for its page's diff.
//!
//! `original` is the transcript the copy follows: the source's lines, or Whisper's segments for
//! a meeting whose source sent none (`basedOn`). `null` while nothing was heard; `corrected` is
//! `null` until the meeting is corrected.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::correction;
use crate::meetings::source::TranscriptLine;
use crate::models::meeting_correction::{self, BasedOn};
use crate::models::{meeting, meeting_transcription};
use crate::state::AppState;
use crate::whisper::WhisperTranscript;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Original {
    based_on: &'static str,
    lines: Vec<TranscriptLine>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Corrected {
    lines: Value,
    changed_lines: i32,
    approved_at: Option<DateTime<Utc>>,
    approved_by: Option<Uuid>,
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
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
    let source: Vec<TranscriptLine> = serde_json::from_value(found.transcript)
        .context("the meeting's transcript is not in the stored shape")?;
    let stored = meeting_correction::find(&mut connection, id).await?;

    // The copy follows Whisper's segments only for a meeting whose source sent no lines, so
    // Whisper's transcript, several times the source's size, is read only then.
    let follows_whisper = stored.as_ref().map_or(source.is_empty(), |stored| {
        stored.based_on() == BasedOn::Whisper
    });
    let whisper = if follows_whisper {
        meeting_transcription::find(&mut connection, id)
            .await?
            .map(|transcription| {
                serde_json::from_value::<WhisperTranscript>(transcription.transcript)
            })
            .transpose()
            .context("the meeting's Whisper transcript is not in the stored shape")?
    } else {
        None
    };
    let followed = if follows_whisper { Vec::new() } else { source };
    let original = correction::base(followed, whisper.as_ref()).map(|base| Original {
        based_on: base.based_on.as_str(),
        lines: base.lines,
    });
    let corrected = stored.map(|stored| Corrected {
        lines: stored.transcript,
        changed_lines: stored.changed_lines,
        approved_at: stored.approved_at,
        approved_by: stored.approved_by,
    });
    Ok(Json(
        json!({ "original": original, "corrected": corrected }),
    ))
}
