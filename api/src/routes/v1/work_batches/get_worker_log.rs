// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/work-batches/{id}/worker-log`: what the batch's worker said and which tools it
//! called, one line each, scrubbed of its secrets, the newest 64 KiB. Empty before its worker
//! said anything. It quotes the batch's work, so it answers the company's members alone.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::meeting::Reader;
use crate::models::work_batch::{self, worker};
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
    // The log quotes the batch, so one written from a meeting the person may not read answers 404
    // (`models::meeting_reads`).
    work_batch::find_readable(&mut connection, id, Reader::of(&member)).await?;
    let log = worker::log(&mut connection, id).await?;
    Ok(Json(json!({ "log": log })))
}
