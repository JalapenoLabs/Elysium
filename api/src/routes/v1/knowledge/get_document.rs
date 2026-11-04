// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/knowledge/documents/{id}`: one document of the knowledge index whole, as a
//! citation opens it, whoever's record it came from.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::knowledge_document;
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
    let document = knowledge_document::get(&mut connection, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(json!({ "document": document })))
}
