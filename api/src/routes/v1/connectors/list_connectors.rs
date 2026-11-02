// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/connectors`: every connector the signed-in person sees, oldest first.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::views;
use crate::auth::CurrentUser;
use crate::connectors::kinds::workspace_kinds;
use crate::errors::ApiError;
use crate::models::connector;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let connectors =
        connector::list_visible(&mut connection, current.id(), &workspace_kinds()).await?;
    drop(connection);
    Ok(Json(
        json!({ "connectors": views(&state, connectors).await? }),
    ))
}
