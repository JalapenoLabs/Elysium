// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/elysia/turns/{id}`: one of her turns, with what started it and, once it ended, its
//! summary or error, which a client loads when `elysiaTurn.changed` names it. A turn that read a
//! meeting the person may not read answers `404`, as if it were not there
//! (`models::meeting_reads`).

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::TurnResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::elysia;
use crate::models::meeting::Reader;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let turn = elysia::find_readable_turn(&mut connection, id, Reader::of(&current.0)).await?;
    Ok(Json(json!({ "turn": TurnResponse::from(turn) })))
}
