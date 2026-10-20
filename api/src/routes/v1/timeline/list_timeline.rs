// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/timeline`: the signed-in person's records, newest first.
//!
//! `projectId` keeps the records attributed to one project. `before` pages back: pass the
//! `occurredAt` of the last record of the previous page.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::TimelineRecordResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::timeline_record::{self, TimelineQuery};
use crate::state::AppState;

/// A page of the timeline, unless the request asks for fewer.
const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    project_id: Option<Uuid>,
    before: Option<DateTime<Utc>>,
    limit: Option<i64>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    query: Result<Query<Filters>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(filters) = query?;
    let limit = filters.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::BadRequest(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let records = timeline_record::list(
        &mut connection,
        TimelineQuery {
            owner: current.id(),
            project_id: filters.project_id,
            before: filters.before,
            limit,
        },
    )
    .await?;

    let responses: Vec<TimelineRecordResponse> = records
        .into_iter()
        .map(|(record, attributions)| TimelineRecordResponse::new(record, attributions))
        .collect();
    Ok(Json(json!({ "records": responses })))
}
