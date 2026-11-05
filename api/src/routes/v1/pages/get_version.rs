// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/pages/{id}/versions/{version}`: one version of a page, whole.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::VersionResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::page;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<(Uuid, i32)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, version)) = path?;
    let version = read_version(&state, id, version).await?;
    Ok(Json(json!({ "version": version })))
}

/// Version `version` of page `id`: what this route and an agent's tool both read.
///
/// # Errors
/// `404` for an unknown page or version.
pub async fn read_version(
    state: &AppState,
    id: Uuid,
    version: i32,
) -> Result<VersionResponse, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let version = page::find_version(&mut connection, id, version).await?;
    Ok(VersionResponse::from(version))
}
