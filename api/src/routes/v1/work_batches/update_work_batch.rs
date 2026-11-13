// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/work-batches/{id}`: change a waiting batch: its `priority` (with
//! `scheduledFor` for a scheduled one), whether it is `paused`, and whether it runs next
//! (`runNext`). Answers it as `GET` does. `409` (`work_batch_moved_on`) once its state no longer
//! allows the change, such as a worker having started on it.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use super::with_history;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::meeting::Reader;
use crate::models::work_batch::{self, Changes, Priority};
use crate::state::AppState;
use crate::worker_pool::actions;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[serde(default)]
    priority: Option<Priority>,
    /// When a scheduled batch is due, with its offset; only beside `priority: "scheduled"`.
    #[serde(default)]
    scheduled_for: Option<DateTime<Utc>>,
    #[serde(default)]
    paused: Option<bool>,
    #[serde(default)]
    run_next: Option<bool>,
}

pub async fn handle(
    State(state): State<AppState>,
    MemberUser(member): MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    if body.priority.is_none() && body.scheduled_for.is_some() {
        return Err(ApiError::BadRequest(
            "scheduledFor goes with priority \"scheduled\"".to_owned(),
        ));
    }
    let changes = Changes {
        priority: body.priority.map(|priority| (priority, body.scheduled_for)),
        paused: body.paused,
        run_next: body.run_next,
    };
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let reader = Reader::of(&member);
    // A batch written from a meeting the person may not read answers 404, as its `GET` does
    // (`models::meeting_reads`).
    work_batch::find_readable(&mut connection, id, reader).await?;
    let batch = actions::change(&mut connection, &state.events, id, changes, member.id).await?;
    Ok(Json(with_history(&mut connection, batch, reader).await?))
}
