// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings/{id}/history` and `POST /api/v1/meetings/{id}/history/{entryId}/undo`:
//! every change to a meeting, newest first, for anyone who may read it, and undoing one, which
//! takes what making the change took (`crate::meetings::access::need_to_undo`)
//! (`docs/meetings.md`, History).
//!
//! Each entry names the one it undid, when it is an undo, so the page can say so and offer Undo
//! only where it still applies. An undo answers its own entry and what it did to the transcript,
//! as an edit does.
//!
//! `404` for a meeting they may not read or an entry that is not the meeting's; `403` for an
//! undo that takes more than they may; `409`
//! `meeting_change_not_undoable` for a build or an approval, `meeting_changed_since` when what it
//! changed moved on since, and `meeting_corrected` for a hold once the transcript is corrected.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use diesel::OptionalExtension;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    MeetingEventResponse, announce_lined_up, lines_up_again, merged_refusal, open,
    transcript_change,
};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::{self, Need};
use crate::meetings::history::{self, UndoError};
use crate::models::agent::Author;
use crate::models::meeting;
use crate::models::meeting_event::{self, MeetingEvent};
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
    let (found, _access) = open(&mut connection, id, &current, Need::Read).await?;
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
    let (found, _access) = open(&mut connection, id, &current, Need::Read).await?;
    // What undoing takes is what making the change took, so the entry is read first; one the
    // meeting does not have is refused by the undo itself.
    let undone = meeting_event::find(&mut connection, found.id, entry_id)
        .await
        .optional()?;
    let need = access::need_to_undo(undone.as_ref().and_then(MeetingEvent::kind));
    open(&mut connection, found.id, &current, need).await?;
    let author = Author {
        user_id: current.id(),
        agent: None,
        at: Utc::now(),
    };
    let undone = history::undo(&mut connection, found.id, entry_id, author)
        .await
        .map_err(refusal)?;
    let entry = undone.entry;
    let change = transcript_change(&mut connection, found.id, &entry).await?;
    if lines_up_again(&entry) {
        announce_lined_up(&state.events, &mut connection, &entry).await?;
    } else {
        // Read again: undoing a hold changes the meeting itself.
        let changed = meeting::find(&mut connection, found.id).await?;
        // With the meeting, the inbox items an undone resolve or reopen raised.
        undone
            .announcement
            .publish(&state.events, &mut connection, &changed, Some(entry.id))
            .await;
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
        UndoError::Merged => merged_refusal(),
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
        UndoError::Thread(error) => {
            anyhow::anyhow!("undoing a thread's change could not tell its people: {error}").into()
        }
        UndoError::Database(database) => database.into(),
        UndoError::Transcript(other) => other.into(),
    }
}
