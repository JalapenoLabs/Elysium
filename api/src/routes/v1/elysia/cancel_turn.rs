// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/elysia/turns/{id}/cancel`: withdraws a turn that has not started. A running
//! turn cannot be cancelled here; it finishes, or fails, on its own.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{TurnResponse, publish_snapshot};
use crate::errors::ApiError;
use crate::models::elysia;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Some(turn) = elysia::cancel_turn(&mut connection, id).await? else {
        return Err(ApiError::CodedConflict {
            code: "turn_not_queued",
            message: "only a turn that has not started can be cancelled",
        });
    };

    let turn = TurnResponse::from(turn);
    state
        .events
        .publish(&ServerEvent::ElysiaTurnUpserted(turn.clone()));
    publish_snapshot(&state.events, &mut connection).await?;

    Ok(Json(json!({ "turn": turn })))
}
