// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/bulk`: delete or restore one or many of the signed-in person's meetings
//! at once, `{ "meetingIds": [...], "change": { "kind": "delete" } }` or `"restore"`, as the
//! Meetings table's selection, a row's actions, and a meeting's page ask. See
//! `crate::meetings::deletion` and `docs/meetings.md`, Deleting.
//!
//! Each meeting changes in its own savepoint of one transaction, recorded in its history with the
//! person: one refused (not theirs, merged into another, deleted already, or not deleted) is
//! answered with its reason while the rest apply, so the answer is `200` listing both, each
//! meeting changed as it now stands.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::responses;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::deletion::{self, BulkChange, MAX_MEETINGS};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// Each meeting to change, once or more; repeats count once.
    meeting_ids: Vec<Uuid>,
    change: BulkChange,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    // In id order, so two changes over the same meetings lock them in the same order.
    let mut meeting_ids = body.meeting_ids;
    meeting_ids.sort_unstable();
    meeting_ids.dedup();
    if meeting_ids.is_empty() {
        return Err(ApiError::BadRequest(
            "meetingIds must name at least one meeting".to_owned(),
        ));
    }
    if meeting_ids.len() > MAX_MEETINGS {
        return Err(ApiError::BadRequest(format!(
            "meetingIds may name at most {MAX_MEETINGS} meetings at once"
        )));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let outcome = deletion::change_many(
        &mut connection,
        &meeting_ids,
        body.change,
        current.id(),
        Utc::now(),
        None,
    )
    .await?;
    deletion::announce(&state.events, &outcome);

    let changed = outcome
        .changed
        .into_iter()
        .map(|changed| changed.meeting)
        .collect();
    let meetings = responses(&mut connection, changed).await?;
    Ok(Json(
        json!({ "meetings": meetings, "refused": outcome.refused }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_names_meetings_and_one_change() {
        let meeting_id = Uuid::now_v7();
        let body: RequestBody = serde_json::from_value(json!({
            "meetingIds": [meeting_id],
            "change": { "kind": "delete" },
        }))
        .expect("parses");
        assert_eq!(body.meeting_ids, [meeting_id]);
        assert_eq!(body.change, BulkChange::Delete {});

        let restore: RequestBody = serde_json::from_value(json!({
            "meetingIds": [meeting_id],
            "change": { "kind": "restore" },
        }))
        .expect("parses");
        assert_eq!(restore.change, BulkChange::Restore {});

        serde_json::from_value::<RequestBody>(json!({ "meetingIds": [meeting_id] }))
            .expect_err("a change is required");
        serde_json::from_value::<RequestBody>(json!({
            "meetingIds": [meeting_id],
            "change": { "kind": "delete", "forever": true },
        }))
        .expect_err("unknown fields are refused");
        serde_json::from_value::<RequestBody>(json!({
            "meetingIds": [meeting_id],
            "change": { "kind": "purge" },
        }))
        .expect_err("only delete and restore are changes");
    }
}
