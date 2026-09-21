// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/action-items/{id}/links`: an item's links, the primary first, each with the
//! provider writes it still owes. What the provider says about each now is
//! `GET .../links/remote`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::link_responses;
use crate::errors::ApiError;
use crate::models::{action_item, action_item_link};
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
    action_item::find(&mut connection, id).await?;
    let links = action_item_link::list_for_item(&mut connection, id).await?;
    let links = link_responses(&mut connection, links).await?;

    Ok(Json(json!({ "links": links })))
}
