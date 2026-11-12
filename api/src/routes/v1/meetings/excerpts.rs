// Copyright © 2026 Jalapeno Labs

//! A meeting's excerpts and black-outs (`docs/meetings.md`, Excerpts and black-outs):
//! `POST /api/v1/meetings/{id}/excerpts` takes a range into a project, `POST .../black-outs`
//! blacks a range out, taking in every black-out it overlaps or touches, and
//! `DELETE .../marks/{markId}` removes an excerpt or a black-out. An excerpt's projects change
//! through its record (`/api/v1/timeline/{id}/projects`), as any record's do.
//!
//! Each answers the meeting's owner alone, as the meeting's other routes do, and each is a change
//! in its history, so it answers its entry and the marks as they now stand, for the page to apply
//! and offer Undo (`history/{entryId}/undo`).
//!
//! `404` for a meeting that is not theirs, or a word or a mark that is not the meeting's; `409`
//! `meeting_words_changed` for a range a word of which was replaced meanwhile, and
//! `meeting_mark_removed` for a mark removed meanwhile; `400` `meeting_excerpt_project` for no
//! project or one that does not exist, `meeting_mark_kind` for a mark that is neither an excerpt
//! nor a black-out, and for a range that ends before it starts.

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
use crate::meetings::excerpts::{self, ExcerptError};
use crate::meetings::segmentation::SegmentProject;
use crate::models::agent::Author;
use crate::models::attribution::PERSON_CONFIDENCE;
use crate::models::meeting;
use crate::models::meeting_mark::{MarkError, Range};
use crate::models::timeline_record;
use crate::realtime::{ServerEvent, Signal};
use crate::state::AppState;

/// A range to take into projects, from its first word to its last.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignRequest {
    start_word_id: Uuid,
    end_word_id: Uuid,
    /// The projects it is about; at least one.
    project_ids: Vec<Uuid>,
}

/// A range to black out, from its first word to its last.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlackOutRequest {
    start_word_id: Uuid,
    end_word_id: Uuid,
}

pub async fn assign(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<AssignRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
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
    let mut projects: Vec<SegmentProject> = Vec::with_capacity(body.project_ids.len());
    for project_id in body.project_ids {
        if projects.iter().all(|kept| kept.project_id != project_id) {
            projects.push(SegmentProject {
                project_id,
                confidence: PERSON_CONFIDENCE,
            });
        }
    }
    let assigned = excerpts::assign(
        &mut connection,
        &found,
        range,
        &projects,
        author_of(&current),
    )
    .await
    .map_err(refusal)?;
    let change = transcript_change(&mut connection, found.id, &assigned.entry).await?;
    state.events.publish(&ServerEvent::TimelineRecordCreated {
        id: assigned.record.id,
        source_kind: assigned.record.source_kind.clone(),
        created_by: assigned.record.created_by,
        action_item_id: None,
        conversation_id: None,
    });
    // A record newly on a project wakes its Project Manager.
    state.events.signal(&Signal::ProjectManagers);
    announce(&state.events, &found, &assigned.entry);
    Ok(Json(json!({
        "entry": MeetingEventResponse::from(assigned.entry),
        "change": change,
        "recordId": assigned.record.id,
    })))
}

pub async fn black_out(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<BlackOutRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
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
    let entry = excerpts::black_out(&mut connection, &found, range, author_of(&current))
        .await
        .map_err(refusal)?;
    let change = transcript_change(&mut connection, found.id, &entry).await?;
    announce(&state.events, &found, &entry);
    Ok(Json(json!({
        "entry": MeetingEventResponse::from(entry),
        "change": change,
    })))
}

pub async fn remove(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, mark_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
    let entry = excerpts::remove(&mut connection, &found, mark_id, author_of(&current))
        .await
        .map_err(refusal)?;
    let change = transcript_change(&mut connection, found.id, &entry).await?;
    // An excerpt's record left its projects, whose timelines follow.
    if let Some(record) = timeline_record::of_excerpt(&mut connection, mark_id).await? {
        state.events.publish(&ServerEvent::TimelineRecordChanged {
            id: record.id,
            created_by: record.created_by,
        });
    }
    announce(&state.events, &found, &entry);
    Ok(Json(json!({
        "entry": MeetingEventResponse::from(entry),
        "change": change,
    })))
}

fn author_of(current: &CurrentUser) -> Author<'static> {
    Author {
        user_id: current.id(),
        agent: None,
        at: Utc::now(),
    }
}

/// Why a change was refused, in the API's words.
fn refusal(error: ExcerptError) -> ApiError {
    match error {
        ExcerptError::Mark(MarkError::NotFound) => ApiError::NotFound,
        ExcerptError::Mark(MarkError::NotStanding) => ApiError::CodedConflict {
            code: "meeting_words_changed",
            message: "A word of that range was changed meanwhile.",
        },
        ExcerptError::Mark(MarkError::Removed) => ApiError::CodedConflict {
            code: "meeting_mark_removed",
            message: "It was removed meanwhile.",
        },
        ExcerptError::Mark(MarkError::OutOfOrder) => {
            ApiError::BadRequest("The range ends before it starts.".to_owned())
        }
        ExcerptError::Mark(MarkError::EditsOwn) | ExcerptError::NotAnExcerpt => {
            ApiError::CodedBadRequest {
                code: "meeting_mark_kind",
                message: "Only an excerpt or a black-out is removed here.".to_owned(),
            }
        }
        ExcerptError::Mark(MarkError::NotAnObject) => {
            ApiError::BadRequest("A mark's data is a JSON object.".to_owned())
        }
        ExcerptError::NoProject | ExcerptError::UnknownProject => ApiError::CodedBadRequest {
            code: "meeting_excerpt_project",
            message: "An excerpt is on at least one project that exists.".to_owned(),
        },
        ExcerptError::Mark(MarkError::Database(error)) | ExcerptError::Database(error) => {
            error.into()
        }
    }
}
