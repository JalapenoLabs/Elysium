// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/{id}/done-reviewing`: a meeting's owner is done reviewing it, which
//! segments it again from its corrected words, keeping every excerpt and black-out a person made
//! or changed (`crate::meetings::review`). Its processing is its owner's, as approving its
//! correction is (`crate::meetings::access`). It answers the history entry and the meeting, back to
//! `recorded` while its new run segments it.
//!
//! `404` for a meeting they may not read, `403` `owner_only` for anyone but its owner; `409`
//! `meeting_processing` while a run of it is going
//! or held, `meeting_run_failed` while one failed and waits for its person,
//! `meeting_not_segmented` before its first segmenting, `meeting_merged` for one merged into
//! another, and `meeting_deleted` while it is deleted.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{MeetingEventResponse, announce, deleted_refusal, merged_refusal, open, response};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::Need;
use crate::meetings::review::{self, ReviewError};
use crate::models::meeting::Reader;
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
    let (found, _access) = open(&mut connection, id, &current, Need::Own).await?;
    let reviewed = review::done_reviewing(&mut connection, &found, current.id())
        .await
        .map_err(|error| match error {
            ReviewError::Busy(run) => meeting_busy(&run),
            ReviewError::NotSegmented => ApiError::CodedConflict {
                code: "meeting_not_segmented",
                message: "This meeting is segmented for the first time once its run gets there.",
            },
            ReviewError::Merged => merged_refusal(),
            ReviewError::Deleted => deleted_refusal(),
            ReviewError::Input(error) => ApiError::Internal(error),
            ReviewError::Database(error) => error.into(),
        })?;
    workflows::announce(&state.events, &reviewed.started.settled);
    state.events.signal(&Signal::Workflows);
    announce(&state.events, &reviewed.meeting, &reviewed.entry);
    let answered = response(&mut connection, reviewed.meeting, Reader::of(&current.0)).await?;
    Ok(Json(json!({
        "entry": MeetingEventResponse::from(reviewed.entry),
        "meeting": answered,
    })))
}
