// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/timeline/{id}`: one of the signed-in person's records, or one of the
//! workspace's (a Project Manager's), with its attributions, which their pages load when
//! `timelineRecord.created` names it.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::TimelineRecordResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::timeline_record;
use crate::state::AppState;

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
    let (record, attributions) = timeline_record::find_readable(
        &mut connection,
        id,
        current.id(),
        current.0.is_company_member(),
    )
    .await?;
    Ok(Json(
        json!({ "record": TimelineRecordResponse::new(record, attributions) }),
    ))
}
