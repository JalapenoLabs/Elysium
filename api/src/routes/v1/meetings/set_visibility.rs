// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/meetings/{id}/visibility`: a meeting's owner or an admin makes it private or opens
//! it to every member, `{ "visibility": "private" }` or `"public"`. It applies at once and is in
//! the meeting's history, so the page offers Undo; its correction leaves it as they chose
//! (`crate::meetings::visibility`).
//!
//! It answers the meeting as it now reads and the history entry. `404` for a meeting they may not
//! read, `403` `owner_or_admin` for anyone else, and `409` `meeting_visibility_unchanged` when it
//! is so already, and `meeting_deleted` while it is deleted.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{MeetingEventResponse, announce, deleted_refusal, open, response};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::Need;
use crate::meetings::visibility::{self, VisibilityError};
use crate::models::agent::Author;
use crate::models::meeting::{Reader, Visibility};
use crate::state::AppState;

/// Who reads it: what a person chooses. `unchecked` is a new meeting's alone, until its
/// correction decides, so no request names it.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Chosen {
    Private,
    Public,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisibilityRequest {
    visibility: Chosen,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<VisibilityRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let visibility = match body.visibility {
        Chosen::Private => Visibility::Private,
        Chosen::Public => Visibility::Public,
    };
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (found, _access) = open(&mut connection, id, &current, Need::Manage).await?;
    let author = Author {
        user_id: current.id(),
        agent: None,
        at: Utc::now(),
    };
    let changed = visibility::set(&mut connection, found.id, visibility, author)
        .await
        .map_err(refusal)?;
    announce(&state.events, &changed.meeting, &changed.entry);
    let answered = response(&mut connection, changed.meeting, Reader::of(&current.0)).await?;
    Ok(Json(json!({
        "meeting": answered,
        "entry": MeetingEventResponse::from(changed.entry),
    })))
}

/// Why a change to who sees a meeting was refused, in the API's words.
pub(super) fn refusal(error: VisibilityError) -> ApiError {
    match error {
        VisibilityError::NotFound => ApiError::NotFound,
        VisibilityError::Unchanged => ApiError::CodedConflict {
            code: "meeting_visibility_unchanged",
            message: "The meeting is so already.",
        },
        VisibilityError::NotShareable => ApiError::CodedBadRequest {
            code: "meeting_share_person",
            message: "A meeting is shared with an active person other than its owner.".to_owned(),
        },
        VisibilityError::Deleted => deleted_refusal(),
        VisibilityError::Database(database) => database.into(),
    }
}
