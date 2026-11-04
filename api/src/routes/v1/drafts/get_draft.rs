// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/drafts/{id}`: one of the signed-in person's drafts, which their pages load when
//! `draft.changed` names it.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::DraftResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::draft;
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
    let found = draft::find_owned(&mut connection, id, current.id()).await?;
    Ok(Json(json!({ "draft": DraftResponse::from(found) })))
}
