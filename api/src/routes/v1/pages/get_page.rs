// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/pages/{id}`: one page whole, live or deleted, with who published it last.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{PageResponse, response};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::page;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let page = read(&state, id).await?;
    Ok(Json(json!({ "page": page })))
}

/// Page `id`: what this route and an agent's tool both read.
///
/// # Errors
/// `404` for an unknown page.
pub async fn read(state: &AppState, id: Uuid) -> Result<PageResponse, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let page = page::find(&mut connection, id).await?;
    response(&mut connection, page).await
}
