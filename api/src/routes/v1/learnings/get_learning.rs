// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/learnings/{id}`: one learning, live or deleted.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::LearningResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::learning;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let learning = learning::find(&mut connection, id).await?;

    Ok(Json(
        json!({ "learning": LearningResponse::from(learning) }),
    ))
}
