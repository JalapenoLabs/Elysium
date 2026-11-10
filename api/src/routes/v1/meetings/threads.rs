// Copyright © 2026 Jalapeno Labs

//! A meeting's comment threads (`docs/meetings.md`, Comments): `GET /api/v1/meetings/{id}/threads`
//! lists what is written in them, `POST /api/v1/meetings/{id}/threads` starts one on a range,
//! `POST .../threads/{threadId}/comments` replies, `POST` and `DELETE .../threads/{threadId}/resolved`
//! resolve and reopen one, and `DELETE .../threads/{threadId}` removes one, comments and all. A
//! thread is named by its comment mark's id.
//!
//! Every route answers the meeting's owner alone, as the meeting's other routes do. Starting,
//! resolving, reopening, and removing a thread are changes in the meeting's history, so each
//! answers its entry and what it did to the transcript's marks, for the page to apply and offer
//! Undo; a reply answers its comment.
//!
//! `404` for a meeting that is not theirs or a thread that is not the meeting's; `409`
//! `meeting_thread_removed` for a thread removed meanwhile, `meeting_thread_unchanged` for one
//! resolved or open already, and `meeting_words_changed` for a range a word of which was replaced
//! meanwhile; `400` `meeting_comment_empty`, `meeting_comment_too_long` past
//! [`BODY_MAX_CHARACTERS`], `meeting_comment_mention` for a mention of anyone but an active person
//! or Elysia, and for a range that ends before it starts.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{CommentResponse, MeetingEventResponse, transcript_change};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::comments::{self, CommentError, Outcome, Writing};
use crate::models::agent::Author;
use crate::models::meeting::{self, Meeting};
use crate::models::meeting_comment::{self, BODY_MAX_CHARACTERS};
use crate::models::meeting_mark::{MarkError, Range};
use crate::state::AppState;

/// A new thread: the range it is on, from its first word to its last, and its first comment.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartRequest {
    start_word_id: Uuid,
    end_word_id: Uuid,
    body: String,
    /// Whom it mentions, by user id.
    #[serde(default)]
    mentions: Vec<Uuid>,
}

/// A reply in a thread.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplyRequest {
    body: String,
    #[serde(default)]
    mentions: Vec<Uuid>,
}

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
    let comments: Vec<CommentResponse> = meeting_comment::of_meeting(&mut connection, found.id)
        .await?
        .into_iter()
        .map(CommentResponse::from)
        .collect();
    Ok(Json(json!({ "comments": comments })))
}

pub async fn start(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<StartRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
    let range = Range {
        start_word_id: body.start_word_id,
        end_word_id: body.end_word_id,
    };
    let writing = Writing {
        body: &body.body,
        mentions: &body.mentions,
    };
    let outcome = comments::start(&mut connection, &found, range, writing, author(&current))
        .await
        .map_err(refusal)?;
    let answered = answer(&state, &mut connection, &found, outcome).await?;
    Ok((StatusCode::CREATED, Json(answered)))
}

pub async fn reply(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    body: Result<Json<ReplyRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path((id, thread_id)) = path?;
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
    let writing = Writing {
        body: &body.body,
        mentions: &body.mentions,
    };
    let outcome = comments::reply(
        &mut connection,
        &found,
        thread_id,
        writing,
        author(&current),
    )
    .await
    .map_err(refusal)?;
    let answered = answer(&state, &mut connection, &found, outcome).await?;
    Ok((StatusCode::CREATED, Json(answered)))
}

pub async fn resolve(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    set_resolved(state, current, path, true).await
}

pub async fn reopen(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    set_resolved(state, current, path, false).await
}

async fn set_resolved(
    state: AppState,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    resolved: bool,
) -> Result<Json<Value>, ApiError> {
    let Path((id, thread_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
    let outcome = comments::set_resolved(
        &mut connection,
        &found,
        thread_id,
        resolved,
        author(&current),
    )
    .await
    .map_err(refusal)?;
    Ok(Json(
        answer(&state, &mut connection, &found, outcome).await?,
    ))
}

pub async fn remove(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, thread_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
    let outcome = comments::remove(&mut connection, &found, thread_id, author(&current))
        .await
        .map_err(refusal)?;
    Ok(Json(
        answer(&state, &mut connection, &found, outcome).await?,
    ))
}

/// `GET /api/v1/meetings/thread-items/{itemId}`: the thread an inbox item was raised by, as its
/// meeting and its id, for the item's page to open it. `404` for an item no thread of the
/// signed-in person's meetings raised.
pub async fn of_item(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(item_id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (meeting_id, thread_id) = meeting_comment::thread_of_item(&mut connection, item_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let found = meeting::find_owned(&mut connection, meeting_id, current.id()).await?;
    Ok(Json(
        json!({ "meetingId": found.id, "threadId": thread_id }),
    ))
}

/// The person signed in, writing now.
fn author(current: &CurrentUser) -> Author<'static> {
    Author {
        user_id: current.id(),
        agent: None,
        at: Utc::now(),
    }
}

/// What a change answers once it committed, announced: its history entry and what it did to the
/// transcript's marks when it is a change in the meeting's history, and its comment when it
/// wrote one.
async fn answer(
    state: &AppState,
    connection: &mut diesel_async::AsyncPgConnection,
    meeting: &Meeting,
    outcome: Outcome,
) -> Result<Value, ApiError> {
    let Outcome {
        entry,
        comment,
        announcement,
    } = outcome;
    let mut answered = json!({});
    if let Some(entry) = entry {
        let change = transcript_change(connection, meeting.id, &entry).await?;
        answered["entry"] = json!(MeetingEventResponse::from(entry));
        answered["change"] = json!(change);
    }
    if let Some(comment) = comment {
        answered["comment"] = json!(CommentResponse::from(comment));
    }
    announcement
        .publish(&state.events, connection, meeting)
        .await;
    Ok(answered)
}

/// Why a thread could not be written to or changed, in the API's words.
fn refusal(error: CommentError) -> ApiError {
    match error {
        CommentError::NotFound | CommentError::Mark(MarkError::NotFound) => ApiError::NotFound,
        CommentError::Removed => ApiError::CodedConflict {
            code: "meeting_thread_removed",
            message: "That thread was removed.",
        },
        CommentError::Unchanged => ApiError::CodedConflict {
            code: "meeting_thread_unchanged",
            message: "That thread is already so.",
        },
        CommentError::Empty => ApiError::CodedBadRequest {
            code: "meeting_comment_empty",
            message: "A comment says something.".to_owned(),
        },
        CommentError::TooLong => ApiError::CodedBadRequest {
            code: "meeting_comment_too_long",
            message: format!("A comment is at most {BODY_MAX_CHARACTERS} characters."),
        },
        CommentError::Unmentionable => ApiError::CodedBadRequest {
            code: "meeting_comment_mention",
            message: "Only active people and Elysia can be mentioned.".to_owned(),
        },
        CommentError::Mark(MarkError::NotStanding) => ApiError::CodedConflict {
            code: "meeting_words_changed",
            message: "A word of that range was changed meanwhile.",
        },
        CommentError::Mark(MarkError::OutOfOrder) => {
            ApiError::BadRequest("The range ends before it starts.".to_owned())
        }
        CommentError::Mark(error) => anyhow::anyhow!("a thread's mark was refused: {error}").into(),
        CommentError::Item(error) => {
            anyhow::anyhow!("a thread's inbox item was refused: {error}").into()
        }
        CommentError::Database(database) => database.into(),
    }
}
