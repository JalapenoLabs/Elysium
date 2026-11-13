// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/work-batches/{id}`: one batch, whole, with its history, oldest first.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::Value;
use uuid::Uuid;

use super::with_history;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::meeting::Reader;
use crate::models::work_batch;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    MemberUser(member): MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let reader = Reader::of(&member);
    // A batch written from a meeting the person may not read answers 404 (`models::meeting_reads`).
    let batch = work_batch::find_readable(&mut connection, id, reader).await?;
    Ok(Json(with_history(&mut connection, batch, reader).await?))
}
