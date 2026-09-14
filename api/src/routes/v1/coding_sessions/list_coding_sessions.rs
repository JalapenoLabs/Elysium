// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/coding-sessions`: every session, newest first, with its thread state.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::CodingSessionResponse;
use crate::errors::ApiError;
use crate::models::coding_session;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let sessions: Vec<CodingSessionResponse> = coding_session::list(&mut connection)
        .await?
        .into_iter()
        .map(|session| {
            let thread = state.fleet.thread_status(session.id);
            CodingSessionResponse::new(session, thread)
        })
        .collect();

    Ok(Json(json!({ "sessions": sessions })))
}
