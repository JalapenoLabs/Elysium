// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/work-batches/{id}/approve`: approve a proposed batch, so it waits for a slot,
//! first changing any of its `title`, `goal`, `repositories`, and `priority` (with `scheduledFor`
//! for a scheduled one) the person gives; what they leave out stays as proposed. Answers it as
//! `GET` does. `409` (`work_batch_moved_on`) once it is no longer proposed, and `400` for an edit
//! the rules every batch is held to refuse.

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
use crate::models::work_batch::approval::Edits;
use crate::models::work_batch::{Priority, Repository};
use crate::state::AppState;
use crate::worker_pool::actions;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    goal: Option<String>,
    /// `[{ url, baseBranch? }]`, replacing every repository it named.
    #[serde(default)]
    repositories: Option<Vec<Repository>>,
    #[serde(default)]
    priority: Option<Priority>,
    /// When a scheduled batch is due, with its offset; only beside `priority: "scheduled"`.
    #[serde(default)]
    scheduled_for: Option<DateTime<Utc>>,
}

pub async fn handle(
    State(state): State<AppState>,
    person: MemberUser,
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
    let edits = Edits {
        title: body.title,
        goal: body.goal,
        repositories: body.repositories,
        priority: body.priority.map(|priority| (priority, body.scheduled_for)),
    };
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let batch = actions::approve(&mut connection, &state.events, id, edits, &person).await?;
    Ok(Json(with_history(&mut connection, batch).await?))
}
