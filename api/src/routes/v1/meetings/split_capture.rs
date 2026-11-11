// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/{id}/captures/{captureId}/split`: the signed-in person splits a capture
//! merged into their meeting back out, undoing the merge that brought it: every capture that merge
//! moved goes back to its own meeting, which is aligned and processed again, and both transcripts
//! are lined up again. `404` for a capture that is not theirs, `409` for one no merge brought, or
//! once what the merge moved has moved on. See `crate::meetings::duplicates`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{announce_lined_up, response};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::duplicates::Moved;
use crate::meetings::history::{self, UndoError};
use crate::models::agent::Author;
use crate::models::meeting;
use crate::models::meeting_event::{self, EventKind};
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
    // The newest merge into the meeting that brought the capture; an older one was split since.
    let history = meeting_event::history(&mut connection, owned.id).await?;
    let brought_it = history.iter().rev().find(|entry| {
        entry.kind() == Some(EventKind::Merged)
            && serde_json::from_value::<Moved>(entry.data.clone()).is_ok_and(|moved| {
                moved.into == owned.id
                    && moved
                        .captures
                        .iter()
                        .any(|moved_capture| moved_capture.capture_id == capture_id)
            })
    });
    let Some(brought_it) = brought_it else {
        return Err(ApiError::CodedConflict {
            code: "capture_not_merged",
            message: "No merge brought this capture into the meeting.",
        });
    };
    let author = Author {
        user_id: current.id(),
        agent: None,
        at: Utc::now(),
    };
    let split = history::undo(&mut connection, owned.id, brought_it.id, author)
        .await
        .map_err(|error| match error {
            UndoError::NotFound => ApiError::NotFound,
            UndoError::NotUndoable | UndoError::ChangedSince | UndoError::Corrected => {
                ApiError::CodedConflict {
                    code: "meeting_changed",
                    message: "What the merge moved has moved on since.",
                }
            }
            UndoError::Database(database) => database.into(),
            UndoError::Transcript(other) => other.into(),
        })?;

    announce_lined_up(&state.events, &mut connection, &split).await?;
    let kept = meeting::find(&mut connection, owned.id).await?;
    let answered = response(&mut connection, kept).await?;
    Ok(Json(json!({ "meeting": answered })))
}
