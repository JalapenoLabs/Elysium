// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/projects/{id}/project-manager/turns/{turn_id}`: one turn with its log, read only
//! as the page opens it, since the list collapses every turn to its summary.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::TurnResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::project_manager;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((project_id, turn_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (turn, log) = project_manager::find_turn(&mut connection, project_id, turn_id).await?;
    let wakes = project_manager::wakes_of(&mut connection, &[turn.id]).await?;
    Ok(Json(json!({
        "turn": TurnResponse::new(turn, wakes),
        "log": log,
    })))
}
