// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/elysia/turns`: her latest turns, newest first, each with what started it and,
//! once it ended, its summary or error. `limit` caps the list at up to 100, 50 unless given.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::TurnResponse;
use crate::errors::ApiError;
use crate::models::elysia;
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
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let turns: Vec<TurnResponse> =
        elysia::list_turns(&mut connection, query.limit.unwrap_or(DEFAULT_LIMIT))
            .await?
            .into_iter()
            .map(TurnResponse::from)
            .collect();

    Ok(Json(json!({ "turns": turns })))
}
