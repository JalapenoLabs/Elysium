// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/learnings/{id}/history`: every change to a learning, oldest first, each naming
//! who made it and the agent acting for them.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::HistoryEntryResponse;
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
    learning::find(&mut connection, id).await?;
    let history: Vec<HistoryEntryResponse> = learning::history(&mut connection, id)
        .await?
        .into_iter()
        .map(HistoryEntryResponse::from)
        .collect();

    Ok(Json(json!({ "history": history })))
}
