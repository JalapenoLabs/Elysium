// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/{id}/edits`: the signed-in person replaces a range of one of their
//! meetings' final transcript with what they typed (`docs/meetings.md`, The final transcript).
//!
//! It applies at once and is in the meeting's history, so the page offers Undo
//! (`history/{entryId}/undo`) rather than a Save. It answers the history entry and what it did to
//! the transcript, for the page to apply without reading the whole transcript again. An edit that
//! fixes a misheard name or term also teaches the Transcription doc, in the same transaction
//! (`crate::meetings::fixes`).
//!
//! `404` for a meeting that is not theirs or a word that is not the meeting's; `409`
//! `meeting_words_changed` when a word of the range was replaced meanwhile; `400`
//! `meeting_edit_empty` for text with no words, `meeting_edit_too_long` past
//! [`EDIT_TEXT_MAX_CHARACTERS`], and for a range that ends before it starts.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{MeetingEventResponse, announce, transcript_change};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::fixes;
use crate::models::agent::Author;
use crate::models::meeting;
use crate::models::meeting_mark::Range;
use crate::models::meeting_word::{EDIT_TEXT_MAX_CHARACTERS, EditError, Replacement};
use crate::state::AppState;

/// The range to replace, from its first word to its last, and what replaces it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditRequest {
    start_word_id: Uuid,
    end_word_id: Uuid,
    /// Split at whitespace into the new words; at least one.
    text: String,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<EditRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
    let replacement = Replacement {
        range: Range {
            start_word_id: body.start_word_id,
            end_word_id: body.end_word_id,
        },
        text: &body.text,
    };
    let author = Author {
        user_id: current.id(),
        agent: None,
        at: Utc::now(),
    };
    // Through `fixes`, so a fix of a misheard word teaches the Transcription doc as it is made.
    let entry = fixes::edit(
        &mut connection,
        &state.events,
        found.id,
        replacement,
        author,
    )
    .await
    .map_err(refusal)?;
    let change = transcript_change(&mut connection, found.id, &entry).await?;
    announce(&state.events, &found, &entry);
    Ok(Json(json!({
        "entry": MeetingEventResponse::from(entry),
        "change": change,
    })))
}

/// Why an edit was refused, in the API's words.
fn refusal(error: EditError) -> ApiError {
    match error {
        EditError::NotFound => ApiError::NotFound,
        EditError::NotStanding => ApiError::CodedConflict {
            code: "meeting_words_changed",
            message: "A word of that range was changed meanwhile.",
        },
        EditError::OutOfOrder => {
            ApiError::BadRequest("The range ends before it starts.".to_owned())
        }
        EditError::Empty => ApiError::CodedBadRequest {
            code: "meeting_edit_empty",
            message: "An edit writes at least one word.".to_owned(),
        },
        EditError::TooLong => ApiError::CodedBadRequest {
            code: "meeting_edit_too_long",
            message: format!("An edit writes at most {EDIT_TEXT_MAX_CHARACTERS} characters."),
        },
        EditError::Database(database) => database.into(),
    }
}
