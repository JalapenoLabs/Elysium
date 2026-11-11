// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings/{id}/history` and `POST /api/v1/meetings/{id}/history/{entryId}/undo`:
//! every change to one of the signed-in person's meetings, newest first, and undoing one
//! (`docs/meetings.md`, History).
//!
//! Each entry names the one it undid, when it is an undo, so the page can say so and offer Undo
//! only where it still applies. An undo answers its own entry and what it did to the transcript,
//! as an edit does.
//!
//! `404` for a meeting that is not theirs or an entry that is not the meeting's; `409`
//! `meeting_change_not_undoable` for a build or an approval, `meeting_changed_since` when what it
//! changed moved on since, and `meeting_corrected` for a hold once the transcript is corrected.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{MeetingEventResponse, announce, announce_lined_up, lines_up_again, transcript_change};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::history::{self, UndoError};
use crate::models::agent::Author;
use crate::models::{meeting, meeting_event};
use crate::state::AppState;

pub async fn list(
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
    let entries: Vec<MeetingEventResponse> = meeting_event::history(&mut connection, found.id)
        .await?
        .into_iter()
        .rev()
        .map(MeetingEventResponse::from)
        .collect();
    Ok(Json(json!({ "entries": entries })))
}

pub async fn undo(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, entry_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
    let author = Author {
        user_id: current.id(),
        agent: None,
        at: Utc::now(),
    };
    let entry = history::undo(&mut connection, found.id, entry_id, author)
        .await
        .map_err(refusal)?;
    let change = transcript_change(&mut connection, found.id, &entry).await?;
    if lines_up_again(&entry) {
        announce_lined_up(&state.events, &mut connection, &entry).await?;
    } else {
        // Read again: undoing a hold changes the meeting itself.
        let changed = meeting::find_owned(&mut connection, found.id, current.id()).await?;
        announce(&state.events, &changed, &entry);
    }
    Ok(Json(json!({
        "entry": MeetingEventResponse::from(entry),
        "change": change,
    })))
}

/// Why an undo was refused, in the API's words.
fn refusal(error: UndoError) -> ApiError {
    match error {
        UndoError::NotFound => ApiError::NotFound,
        UndoError::NotUndoable => ApiError::CodedConflict {
            code: "meeting_change_not_undoable",
            message: "This change cannot be undone.",
        },
        UndoError::ChangedSince => ApiError::CodedConflict {
            code: "meeting_changed_since",
            message: "What this change touched has changed since, so it stays.",
        },
        UndoError::Corrected => ApiError::CodedConflict {
            code: "meeting_corrected",
            message: "The meeting's transcript is corrected already.",
        },
        UndoError::Database(database) => database.into(),
        UndoError::Transcript(other) => other.into(),
    }
}
