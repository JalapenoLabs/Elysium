// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings/{id}/words`: one of the signed-in person's meetings' final transcript as
//! it reads, for its page: its standing words in order, every standing mark over them (each
//! excerpt with its record and the projects it is on), and its captures, each of which opens as it
//! came (`captures/{captureId}/transcript`) and plays its recording (`captures/{captureId}/media`),
//! saying where it sits on the meeting's clock and how that was found, and which meeting it came
//! from when a merge brought it, which splitting it makes its own again.
//!
//! `words` is empty until the transcript is built, which its correction does
//! (`docs/meetings.md`, The final transcript). `404` for a meeting that is not theirs.

use std::collections::HashMap;

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{WordResponse, mark_responses};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::duplicates;
use crate::meetings::media::media_type_of;
use crate::models::meeting_capture::{self, CaptureSummary};
use crate::models::meeting_event::{self, EventKind};
use crate::models::{meeting, meeting_mark, meeting_word};
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
    /// How that was found: `listed`, `audio`, `words`, or `hand`, which aligning never moves.
    offset_source: &'static str,
    has_recording: bool,
    /// What its recording is, such as `video/mp4`, which the player's switcher starts on the best
    /// video by; `null` without a recording, or one of a kind Elysium never writes.
    recording_content_type: Option<String>,
    /// The meeting a merge brought it from, while that merge stands; `null` for a capture of the
    /// meeting's own.
    merged_from: Option<MergedFrom>,
}

/// The meeting a capture was merged in from.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MergedFrom {
    meeting_id: Uuid,
    title: String,
}

impl CaptureResponse {
    fn new(capture: CaptureSummary, merged_from: Option<MergedFrom>) -> Self {
        // A recording kept before its type was recorded is named by its extension, as its media
        // route serves it.
        let recording_content_type = capture.recording_path.as_deref().and_then(|path| {
            capture
                .recording_content_type
                .clone()
                .or_else(|| media_type_of(path).map(ToOwned::to_owned))
        });
        Self {
            offset_source: capture.offset_source().as_str(),
            id: capture.id,
            connector_id: capture.connector_id,
            source_kind: capture.source_kind,
            url: capture.url,
            offset_ms: capture.offset_ms,
            has_recording: capture.recording_path.is_some(),
            recording_content_type,
            merged_from,
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
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
    // One snapshot, so every mark answered is over words answered, however an edit lands
    // meanwhile, and every capture is named by the merges that hold it now.
    let (words, marks, captures, merges) = connection
        .build_transaction()
        .read_only()
        .repeatable_read()
        .run(async |connection| {
            let words = meeting_word::standing(connection, found.id).await?;
            let marks = meeting_mark::standing(connection, found.id).await?;
            let captures = meeting_capture::summaries_of(connection, found.id).await?;
            let merges = meeting_event::of_kind(connection, found.id, EventKind::Merged).await?;
            diesel::QueryResult::Ok((words, marks, captures, merges))
        })
        .await?;
    let brought = duplicates::merges_bringing(&merges, found.id);
    let mut from_ids: Vec<Uuid> = captures
        .iter()
        .filter_map(|capture| brought.get(&capture.id).map(|merge| merge.from))
        .collect();
    from_ids.sort_unstable();
    from_ids.dedup();
    // A meeting merged in is the same person's, and outlives the merge, so its title is read as
    // it stands now. Most meetings had nothing merged in, and their page, read on every change,
    // asks nothing more.
    let titles: HashMap<Uuid, String> = if from_ids.is_empty() {
        HashMap::new()
    } else {
        meeting::titles_of(&mut connection, &from_ids).await?
    };

    let words: Vec<WordResponse> = words.into_iter().map(WordResponse::from).collect();
    let marks = mark_responses(&mut connection, marks).await?;
    let captures: Vec<CaptureResponse> = captures
        .into_iter()
        .map(|capture| {
            let merged_from = brought.get(&capture.id).map(|merge| {
                let title = titles.get(&merge.from).cloned().unwrap_or_else(|| {
                    // Meetings are never deleted, so only a fault leaves a merge naming none; the
                    // page then calls it untitled rather than hiding where the capture came from.
                    tracing::event!(
                        name: "meetings.words.merged_from_missing",
                        tracing::Level::WARN,
                        meeting.id = %found.id,
                        meeting_capture.id = %capture.id,
                        "a capture's merge names a meeting that is gone",
                    );
                    String::new()
                });
                MergedFrom {
                    meeting_id: merge.from,
                    title,
                }
            });
            CaptureResponse::new(capture, merged_from)
        })
        .collect();
    Ok(Json(
        json!({ "words": words, "marks": marks, "captures": captures }),
    ))
}
