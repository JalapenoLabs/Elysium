// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/{id}/done-reviewing`: the signed-in person is done reviewing one of
//! their meetings, which segments it again from its corrected words, keeping every excerpt and
//! black-out a person made or changed (`crate::meetings::review`). It answers the history entry
//! and the meeting, back to `recorded` while its new run segments it.
//!
//! `404` for a meeting that is not theirs; `409` `meeting_processing` while a run of it is going
//! or held, `meeting_run_failed` while one failed and waits for its person,
//! `meeting_not_segmented` before its first segmenting, and `meeting_merged` for one merged into
//! another.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{MeetingEventResponse, announce, response};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::review::{self, ReviewError};
use crate::models::meeting;
use crate::realtime::Signal;
use crate::routes::v1::meeting_uploads::meeting_busy;
use crate::state::AppState;
use crate::workflows;

pub async fn handle(
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
    let reviewed = review::done_reviewing(&mut connection, &found, current.id())
        .await
        .map_err(|error| match error {
            ReviewError::Busy(run) => meeting_busy(&run),
            ReviewError::NotSegmented => ApiError::CodedConflict {
                code: "meeting_not_segmented",
                message: "This meeting is segmented for the first time once its run gets there.",
            },
            ReviewError::Merged => ApiError::CodedConflict {
                code: "meeting_merged",
                message: "This meeting was merged into another; review that one.",
            },
            ReviewError::Input(error) => ApiError::Internal(error),
            ReviewError::Database(error) => error.into(),
        })?;
    workflows::announce(&state.events, &reviewed.started.settled);
    state.events.signal(&Signal::Workflows);
    announce(&state.events, &reviewed.meeting, &reviewed.entry);
    let answered = response(&mut connection, reviewed.meeting).await?;
    Ok(Json(json!({
        "entry": MeetingEventResponse::from(reviewed.entry),
        "meeting": answered,
    })))
}
