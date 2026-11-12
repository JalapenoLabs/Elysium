// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/{id}/captures/{captureId}/split`: the signed-in person splits a capture
//! merged into their meeting back out, undoing the merge that brought it: every capture that merge
//! moved goes back to its own meeting, which is aligned and processed again, and both transcripts
//! are lined up again. Answers the `split` entry, which the page's Undo undoes by merging again,
//! and the meeting as it now stands. `404` for a meeting that is not theirs; `409`
//! `capture_not_merged` for a capture the meeting does not hold or no merge brought, such as one
//! split out already, and `meeting_changed` once a capture the merge moved was moved by hand since.
//! See `crate::meetings::duplicates`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{MeetingEventResponse, announce_lined_up, merged_refusal, response};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::duplicates;
use crate::meetings::history::{self, UndoError};
use crate::models::agent::Author;
use crate::models::meeting_event::{self, EventKind};
use crate::models::{meeting, meeting_capture};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((meeting_id, capture_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let owned = meeting::find_owned(&mut connection, meeting_id, current.id()).await?;
    let not_merged = ApiError::CodedConflict {
        code: "capture_not_merged",
        message: "No merge brought this capture into the meeting.",
    };
    // A merge's entry stays in the history once split, so only a capture the meeting still holds
    // is looked for among its merges: one split out already, elsewhere or a moment ago, is one no
    // standing merge brought, not one that moved on since.
    match meeting_capture::find(&mut connection, capture_id).await {
        Ok(capture) if capture.meeting_id == owned.id => {}
        Ok(_) | Err(diesel::result::Error::NotFound) => return Err(not_merged),
        Err(error) => return Err(error.into()),
    }
    let merges = meeting_event::of_kind(&mut connection, owned.id, EventKind::Merged).await?;
    let Some(brought_it) = duplicates::merges_bringing(&merges, owned.id)
        .get(&capture_id)
        .copied()
    else {
        return Err(not_merged);
    };
    let author = Author {
        user_id: current.id(),
        agent: None,
        at: Utc::now(),
    };
    let split = history::undo(&mut connection, owned.id, brought_it.entry_id, author)
        .await
        .map_err(|error| match error {
            UndoError::NotFound => ApiError::NotFound,
            UndoError::Merged => merged_refusal(),
            UndoError::NotUndoable | UndoError::ChangedSince | UndoError::Corrected => {
                ApiError::CodedConflict {
                    code: "meeting_changed",
                    message: "What the merge moved has moved on since.",
                }
            }
            UndoError::Database(database) => database.into(),
            UndoError::Transcript(other) => other.into(),
            UndoError::Thread(error) => {
                anyhow::anyhow!("undoing the merge could not tell a thread's people: {error}")
                    .into()
            }
        })?
        .entry;

    announce_lined_up(&state.events, &mut connection, &split).await?;
    let kept = meeting::find(&mut connection, owned.id).await?;
    let answered = response(&mut connection, kept).await?;
    Ok(Json(json!({
        "entry": MeetingEventResponse::from(split),
        "meeting": answered,
    })))
}
