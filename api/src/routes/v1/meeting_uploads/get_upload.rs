// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meeting-uploads/{id}`: one of the signed-in person's uploads, with how much of
//! each file arrived, which is where a resumed upload carries on from.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::UploadResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::meeting_upload;
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
    let staged = meeting_upload::find_owned(&mut connection, id, current.id()).await?;
    Ok(Json(json!({ "upload": UploadResponse::from(staged) })))
}
