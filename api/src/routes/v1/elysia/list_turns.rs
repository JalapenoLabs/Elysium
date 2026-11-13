// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/elysia/turns`: her latest turns, newest first, each with what started it and,
//! once it ended, its summary or error. `limit` caps the list at up to 100, 50 unless given. A turn
//! that read a meeting the person may not read is left out (`models::meeting_reads`).

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::TurnResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::elysia;
use crate::models::meeting::Reader;
use crate::state::AppState;

/// How many turns the list holds unless the client asks for another number.
const DEFAULT_LIMIT: i64 = 50;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    limit: Option<i64>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    let turns: Vec<TurnResponse> =
        elysia::list_turns(&mut connection, Reader::of(&current.0), limit)
            .await?
            .into_iter()
            .map(TurnResponse::from)
            .collect();

    Ok(Json(json!({ "turns": turns })))
}
