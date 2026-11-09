// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/action-items/{id}/lead-project`: make one of an item's projects its lead, the one
//! whose Project Manager alone works it. An attribution step never moves a lead a person chose.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::publish_item_write;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::action_item_attribution;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// One of the item's projects.
    project_id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let led = action_item_attribution::set_lead(
        &mut connection,
        id,
        body.project_id,
        current.actor(),
        now,
    )
    .await?;
    let item = publish_item_write(&state.events, &mut connection, led, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}
