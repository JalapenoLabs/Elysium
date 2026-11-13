// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/work-batches/{id}/pull-requests`: the pull requests the batch's worker recorded,
//! in order, each with where it stands, who merged it, and why the last look left it open. They
//! name the batch's work, so it answers the company's members alone.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::meeting::Reader;
use crate::models::work_batch::{self, pull_request};
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
    // An unknown batch answers 404, rather than a batch with none, and so does one written from a
    // meeting the person may not read (`models::meeting_reads`).
    work_batch::find_readable(&mut connection, id, Reader::of(&member)).await?;
    let pull_requests = pull_request::of_batch(&mut connection, id).await?;
    Ok(Json(json!({ "pullRequests": pull_requests })))
}
