// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/jira-credentials/{id}/projects/{key}/done-transition`: choose which `done`
//! status resolving an item moves the project's issues into.
//!
//! The status must be one of the project's `done` statuses as Jira reports them now. The
//! watcher is woken, so closes that waited on the choice are tried at once.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{DoneTransitionChoice, open, project_key_of};
use crate::action_items::links::jira::done_statuses;
use crate::errors::ApiError;
use crate::models::jira_done_transition;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// One of the ids `GET` answered.
    #[validate(length(min = 1, max = 19))]
    status_id: String,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, project_key)) = path?;
    let project_key = project_key_of(&project_key)?;
    let Json(body) = body?;
    body.validate()?;

    let stored = open(&state, id).await?;
    let statuses = done_statuses(&state.jira, &stored, project_key).await?;
    let Some(status) = statuses
        .into_iter()
        .find(|status| status.id == body.status_id)
    else {
        return Err(ApiError::BadRequest(format!(
            "status {} is not one of project {project_key}'s done statuses",
            body.status_id
        )));
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let chosen =
        jira_done_transition::choose(&mut connection, id, project_key, &status.id, &status.name)
            .await?;
    state.links.wake_watcher();

    Ok(Json(
        json!({ "chosen": DoneTransitionChoice::from(chosen) }),
    ))
}
