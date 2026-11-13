// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/timeline`: the signed-in person's records, newest first.
//!
//! `projectId` keeps the records attributed to one project, `actionItemId` the record that
//! raised one action item, which its page answers from, and `meetingId` one meeting's standing
//! excerpts. `before` pages back: pass the
//! `occurredAt` of the last record of the previous page. `total` counts every record the
//! filters match across all pages, so a project's Timeline tab can name its size.
//!
//! A record naming a meeting the person may not read is left out of both, as if it were not there
//! (`models::meeting_reads`).

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
use crate::models::meeting::Reader;
use crate::models::timeline_record::{self, TimelineQuery};
use crate::state::AppState;

/// A page of the timeline, unless the request asks for fewer.
const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    project_id: Option<Uuid>,
    action_item_id: Option<Uuid>,
    meeting_id: Option<Uuid>,
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
    let query = TimelineQuery {
        reader: Reader::of(&current.0),
        shared: current.0.is_company_member(),
        project_id: filters.project_id,
        action_item_id: filters.action_item_id,
        meeting_id: filters.meeting_id,
        before: filters.before,
        limit,
    };
    let total = timeline_record::count(&mut connection, &query).await?;
    let records = timeline_record::list(&mut connection, query).await?;

    let responses: Vec<TimelineRecordResponse> = records
        .into_iter()
        .map(|(record, attributions)| TimelineRecordResponse::new(record, attributions))
        .collect();
    Ok(Json(json!({ "records": responses, "total": total })))
}
