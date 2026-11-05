// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/pages/{id}/history`: everything that happened to a page, oldest first, each entry
//! naming who did it and the agent acting for them.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::HistoryEntryResponse;
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
    let history = read_history(&state, id).await?;
    Ok(Json(json!({ "history": history })))
}

/// Page `id`'s history: what this route and an agent's tool both read.
///
/// # Errors
/// `404` for an unknown page.
pub async fn read_history(
    state: &AppState,
    id: Uuid,
) -> Result<Vec<HistoryEntryResponse>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    page::find(&mut connection, id).await?;
    let history = page::history(&mut connection, id)
        .await?
        .into_iter()
        .map(HistoryEntryResponse::from)
        .collect();
    Ok(history)
}
