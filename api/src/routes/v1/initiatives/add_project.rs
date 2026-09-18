// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/initiatives/{id}/projects/{project_id}`: put an initiative in a project. An
//! initiative already in it is answered unchanged.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::publish_initiative_write;
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::initiative;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, project_id)) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let added = initiative::add_project(&mut connection, id, project_id, Actor::User, now).await?;
    let initiative = publish_initiative_write(&state, &mut connection, added, now).await?;

    Ok(Json(json!({ "initiative": initiative })))
}
