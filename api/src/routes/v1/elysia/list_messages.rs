// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/elysia/messages`: the conversation, newest first, a page at a time.
//!
//! `before` takes a message id and returns the messages sent before it; `limit` caps the page
//! at up to 100 messages, 50 unless given.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::MessageResponse;
use crate::errors::ApiError;
use crate::models::elysia;
use crate::state::AppState;

/// How many messages a page holds unless the client asks for another number.
const DEFAULT_PAGE: i64 = 50;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    before: Option<Uuid>,
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
    let messages: Vec<MessageResponse> = elysia::list_messages(
        &mut connection,
        query.before,
        query.limit.unwrap_or(DEFAULT_PAGE),
    )
    .await?
    .into_iter()
    .map(MessageResponse::from)
    .collect();

    Ok(Json(json!({ "messages": messages })))
}
