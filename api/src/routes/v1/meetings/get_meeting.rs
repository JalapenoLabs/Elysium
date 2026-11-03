// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings/{id}`: one of the signed-in person's meetings, which its page loads
//! when `meeting.changed` names it.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::MeetingResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::meeting;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let found = meeting::find_owned(&mut connection, id, current.id()).await?;
    Ok(Json(json!({ "meeting": MeetingResponse::new(found) })))
}
