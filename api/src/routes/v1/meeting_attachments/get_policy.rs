// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meeting-attachments`: what a meeting takes as a file or a frame, for anyone signed
//! in, since anyone who reads a meeting may attach to it.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::PolicyResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::user;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _current: CurrentUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let settings = user::workspace_settings(&mut connection).await?;
    Ok(Json(json!({ "policy": PolicyResponse::from(settings) })))
}
