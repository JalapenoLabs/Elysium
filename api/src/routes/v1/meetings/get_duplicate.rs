// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meetings/duplicates?itemId=`: the two meetings an inbox item asks the signed-in
//! person about, "Same meeting as" the other, and what they decided, for the item's page to offer
//! Merge and Not the same. `null` when the item asks about none of theirs.

use std::collections::HashMap;

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::meeting_duplicate::{self, MeetingDuplicate};
use crate::models::{meeting, meeting_capture};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    item_id: Uuid,
}

/// A pair as the item's page shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateResponse {
    id: Uuid,
    /// The meeting asked about, whose captures a merge moves.
    meeting_id: Uuid,
    meeting_title: String,
    /// Where its first capture came from, such as `fireflies`; `null` once its captures were
    /// merged into the other.
    meeting_source_kind: Option<String>,
    /// The meeting it looks like.
    same_as_id: Uuid,
    same_as_title: String,
    same_as_source_kind: Option<String>,
    /// `merged` or `not_the_same`, or `null` until its person decides.
    decision: Option<String>,
    decided_at: Option<DateTime<Utc>>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    query: Result<Query<Filters>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(filters) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Some(pair) = meeting_duplicate::asked_by(&mut connection, filters.item_id).await? else {
        return Ok(Json(json!({ "duplicate": null })));
    };
    match meeting::find_owned(&mut connection, pair.meeting_id, current.id()).await {
        Ok(_asking) => {}
        // An item about someone else's meeting asks nothing of this person.
        Err(diesel::result::Error::NotFound) => return Ok(Json(json!({ "duplicate": null }))),
        Err(error) => return Err(error.into()),
    }
    let answered = response(&mut connection, pair).await?;
    Ok(Json(json!({ "duplicate": answered })))
}

/// `pair` as the item's page shows it, with both meetings' titles and where each came from, which
/// tells two meetings of one title apart.
pub(super) async fn response(
    connection: &mut AsyncPgConnection,
    pair: MeetingDuplicate,
) -> Result<DuplicateResponse, ApiError> {
    let asking = meeting::find(connection, pair.meeting_id).await?;
    let same_as = meeting::find(connection, pair.same_as_id).await?;
    let sources: HashMap<Uuid, String> =
        meeting_capture::first_of_each(connection, &[asking.id, same_as.id])
            .await?
            .into_iter()
            .map(|capture| (capture.meeting_id, capture.source_kind))
            .collect();
    Ok(DuplicateResponse {
        id: pair.id,
        meeting_id: pair.meeting_id,
        meeting_title: asking.title,
        meeting_source_kind: sources.get(&asking.id).cloned(),
        same_as_id: pair.same_as_id,
        same_as_title: same_as.title,
        same_as_source_kind: sources.get(&same_as.id).cloned(),
        decision: pair.decision,
        decided_at: pair.decided_at,
    })
}
