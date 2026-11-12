// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/meetings/{id}/shares/{personId}` and `DELETE`: a meeting's owner or an admin
//! shares it with a person, or stops sharing it with them. A private meeting is read by its owner
//! and the people it is shared with; a guest reads only a meeting shared with them, public or not
//! (`docs/meeting-review.md`, Who sees a meeting). Each applies at once and is in the meeting's
//! history, so the page offers Undo.
//!
//! Each answers the meeting as it now reads and the history entry. `404` for a meeting they may
//! not read, `403` `owner_or_admin` for anyone else, `400` `meeting_share_person` for anyone but an
//! active person other than its owner, and `409` `meeting_visibility_unchanged` when it is shared
//! with them already, or was not.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::set_visibility::refusal;
use super::{MeetingEventResponse, announce, open, response};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::Need;
use crate::meetings::visibility;
use crate::models::agent::Author;
use crate::models::meeting::Reader;
use crate::state::AppState;

pub async fn share(
    state: State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    change(state, current, path, true).await
}

pub async fn unshare(
    state: State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    change(state, current, path, false).await
}

async fn change(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    sharing: bool,
) -> Result<Json<Value>, ApiError> {
    let Path((id, person_id)) = path?;
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
    let changed = if sharing {
        visibility::share(&mut connection, found.id, person_id, author).await
    } else {
        visibility::unshare(&mut connection, found.id, person_id, author).await
    }
    .map_err(refusal)?;
    announce(&state.events, &changed.meeting, &changed.entry);
    let answered = response(&mut connection, changed.meeting, Reader::of(&current.0)).await?;
    Ok(Json(json!({
        "meeting": answered,
        "entry": MeetingEventResponse::from(changed.entry),
    })))
}
