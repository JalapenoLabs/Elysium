// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings/{id}`: one meeting the signed-in person may read, which its page loads
//! and every client loads when `meeting.changed` names it. `404` for one they may not read.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{open, response};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::access::Need;
use crate::models::meeting::Reader;
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
    let (found, _access) = open(&mut connection, id, &current, Need::Read).await?;
    let answered = response(&mut connection, found, Reader::of(&current.0)).await?;
    Ok(Json(json!({ "meeting": answered })))
}
