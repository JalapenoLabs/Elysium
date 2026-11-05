// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/pages/{id}/versions`: a page's versions, newest first, each naming who published it
//! and the agent acting for them, without what it says.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::VersionSummaryResponse;
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
    let versions = read_versions(&state, id).await?;
    Ok(Json(json!({ "versions": versions })))
}

/// Page `id`'s versions: what this route and an agent's tool both read.
///
/// # Errors
/// `404` for an unknown page.
pub async fn read_versions(
    state: &AppState,
    id: Uuid,
) -> Result<Vec<VersionSummaryResponse>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    page::find(&mut connection, id).await?;
    let versions = page::versions(&mut connection, id)
        .await?
        .into_iter()
        .map(VersionSummaryResponse::from)
        .collect();
    Ok(versions)
}
