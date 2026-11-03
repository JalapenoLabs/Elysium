// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings`: the signed-in person's meetings, newest first.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::MeetingResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::meeting;
use crate::state::AppState;

/// The most meetings one listing answers: the recent ones a person checks on.
const LIMIT: i64 = 100;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let meetings = meeting::list_owned(&mut connection, current.id(), LIMIT).await?;
    let responses: Vec<MeetingResponse> = meetings.into_iter().map(MeetingResponse::new).collect();
    Ok(Json(json!({ "meetings": responses })))
}
