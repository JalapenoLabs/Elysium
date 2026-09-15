// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/mail/server`: the mail server's status, asked live.

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let server = state.mail.hosting.status().await?;
    Ok(Json(json!({ "server": server })))
}
