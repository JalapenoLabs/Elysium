// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/meetings/{id}/captures/{captureId}/offset`: a meeting's owner sets where one
//! capture of it sits on its clock, by hand, `{ "offsetMs": 12500 }`. It is recorded
//! in the meeting's history and undone from there, the transcript is lined up again at once, and
//! aligning never moves it after. `404` for a meeting they may not read or a capture that is not
//! its, `403` `owner_only` for anyone but its owner, `400` for an offset more than twelve hours
//! either way. See `crate::meetings::clock`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{announce_lined_up, open};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::Need;
use crate::meetings::clock::{self, NudgeError};
use crate::models::agent::Author;
use crate::models::meeting_capture;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    offset_ms: i64,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((meeting_id, capture_id)) = path?;
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (owned, _access) = open(&mut connection, meeting_id, &current, Need::Own).await?;
    let author = Author {
        user_id: current.id(),
        agent: None,
        at: Utc::now(),
    };
    let nudged = clock::nudge(
        &mut connection,
        owned.id,
        capture_id,
        body.offset_ms,
        author,
    )
    .await
    .map_err(|error| match error {
        NudgeError::NotFound => ApiError::NotFound,
        NudgeError::OutOfRange => ApiError::CodedBadRequest {
            code: "offset_out_of_range",
            message: "An offset is at most twelve hours either way.".to_owned(),
        },
        NudgeError::Database(database) => database.into(),
        NudgeError::Transcript(other) => other.into(),
    })?;
    announce_lined_up(&state.events, &mut connection, &nudged).await?;
    let capture = meeting_capture::find(&mut connection, capture_id).await?;
    Ok(Json(json!({
        "capture": {
            "id": capture.id,
            "offsetMs": capture.offset_ms,
            "offsetSource": capture.offset_source().as_str(),
        },
    })))
}
