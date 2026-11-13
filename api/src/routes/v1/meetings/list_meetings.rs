// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings`: the meetings the signed-in person may read, newest first, a page at a
//! time.
//!
//! A page holds up to `limit` meetings, and `nextCursor` names where the next one starts, as
//! `afterStartedAt` and `afterId`: the last meeting answered, so a page never skips or repeats
//! one, however many started together. It is `null` on the last page. The knowledge base's
//! Meetings page and its tree read every page today, then search and filter them in the browser
//! (`docs/knowledge.md`, The tree). Each answers the projects its topics are on, `projectIds`,
//! which both read.
//!
//! `reviewItemId` keeps the meeting whose corrected transcript an inbox item asks to review, and
//! `keptPrivateItemId` the one an item said its correction kept private, which each item's page
//! links to; either answers one page, the meeting or none.
//!
//! `deleted=true` pages through the deleted meetings the person may restore instead, their own or,
//! for an admin, everyone's, in the same order, which the Meetings page's Deleted filter restores
//! from; no other listing holds them.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::responses;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::meeting::{self, Cursor, Reader};
use crate::state::AppState;

/// A page of meetings unless the request asks for fewer: what a busy month holds.
const DEFAULT_LIMIT: i64 = 100;

/// The most meetings one page answers. Each is read with its capture, correction, run, projects,
/// and shares in a query apiece for the whole page, so a page this size stays one quick answer.
const MAX_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    review_item_id: Option<Uuid>,
    kept_private_item_id: Option<Uuid>,
    #[serde(default)]
    deleted: bool,
    limit: Option<i64>,
    /// The `startedAt` of the last meeting of the page before, with an offset.
    after_started_at: Option<DateTime<Utc>>,
    /// The `id` of the last meeting of the page before.
    after_id: Option<Uuid>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    query: Result<Query<Filters>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(filters) = query?;
    let limit = filters.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::BadRequest(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    let after = match (filters.after_started_at, filters.after_id) {
        (Some(started_at), Some(id)) => Some(Cursor { started_at, id }),
        (None, None) => None,
        _ => {
            return Err(ApiError::BadRequest(
                "afterStartedAt and afterId name a page together".to_owned(),
            ));
        }
    };
    let reader = Reader::of(&current.0);
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (meetings, next_cursor) = match (filters.review_item_id, filters.kept_private_item_id) {
        (Some(item_id), _) => (
            meeting::reviewed_by(&mut connection, item_id, reader).await?,
            None,
        ),
        (None, Some(item_id)) => (
            meeting::kept_private_by(&mut connection, item_id, reader).await?,
            None,
        ),
        (None, None) => {
            let page = if filters.deleted {
                meeting::list_deleted(&mut connection, reader, after, limit).await?
            } else {
                meeting::list_readable(&mut connection, reader, after, limit).await?
            };
            // A full page may have more after it; a shorter one is the last.
            let next_cursor = page
                .last()
                .filter(|_| i64::try_from(page.len()).unwrap_or(i64::MAX) == limit)
                .map(|last| json!({ "afterStartedAt": last.started_at, "afterId": last.id }));
            (page, next_cursor)
        }
    };
    let answered = responses(&mut connection, meetings, reader).await?;
    Ok(Json(
        json!({ "meetings": answered, "nextCursor": next_cursor }),
    ))
}
