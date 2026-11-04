// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/learnings/{id}/restore`: bring a deleted learning back, into the knowledge
//! index again. Restoring a live learning changes nothing.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{announce, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::learning;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = learning::restore(&mut connection, id, author(&member, None)).await?;

    Ok(Json(json!({ "learning": announce(&state, recorded) })))
}
