// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/initiatives/{id}/history`: everything that happened to an initiative,
//! including items joining and leaving it, oldest first.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::{action_item_event, initiative};
use crate::routes::v1::action_items::HistoryEntryResponse;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    initiative::find(&mut connection, id).await?;
    let history: Vec<HistoryEntryResponse> =
        action_item_event::list_for_initiative(&mut connection, id)
            .await?
            .into_iter()
            .map(HistoryEntryResponse::from)
            .collect();

    Ok(Json(json!({ "history": history })))
}
