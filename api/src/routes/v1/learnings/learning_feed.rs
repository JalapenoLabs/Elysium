// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/learnings/feed`: for admins, every change to every learning, newest first, so
//! they see what people and their agents are saving as it arrives.
//!
//! Answers at most `limit` entries (50 unless asked, up to 200), each with the learning it is
//! about as it stands now. `before`, an entry id, reads on past the last entry seen, and
//! `nextBefore` is the value to pass for the page after this one, `null` at the end.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{HistoryEntryResponse, LearningResponse};
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::learning;
use crate::state::AppState;

/// Entries answered when a client does not say how many: a screenful.
const DEFAULT_LIMIT: i64 = 50;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedQuery {
    before: Option<Uuid>,
    #[validate(range(min = 1, max = 200))]
    limit: Option<i64>,
}

/// One change and the learning it is about.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FeedEntry {
    entry: HistoryEntryResponse,
    learning: LearningResponse,
}

pub async fn handle(
    State(state): State<AppState>,
    _admin: AdminUser,
    query: Result<Query<FeedQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    query.validate()?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let page = learning::feed(&mut connection, query.before, limit).await?;

    // A short page is the last one.
    let is_full = usize::try_from(limit).is_ok_and(|limit| page.len() == limit);
    let next_before = page.last().filter(|_| is_full).map(|(entry, _)| entry.id);
    let entries: Vec<FeedEntry> = page
        .into_iter()
        .map(|(entry, learning)| FeedEntry {
            entry: HistoryEntryResponse::from(entry),
            learning: LearningResponse::from(learning),
        })
        .collect();

    Ok(Json(
        json!({ "entries": entries, "nextBefore": next_before }),
    ))
}
