// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/action-items/{id}`: edit an item's title, notes, priority, due date, or
//! owner. State, snooze, and waiting have routes of their own.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{publish_item_write, validate_not_blank, validate_owner};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item::{self, ActionItemChanges, ActionItemPriority, Owner};
use crate::state::AppState;

/// Absent fields stay as they are; `dueAt: null` removes the due date.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 500), custom(function = "validate_not_blank"))]
    title: Option<String>,
    #[validate(length(max = 20000))]
    notes: Option<String>,
    priority: Option<ActionItemPriority>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and a date are three distinct requests"
    )]
    due_at: Option<Option<DateTime<Utc>>>,
    #[validate(custom(function = "validate_owner"))]
    owner: Option<Owner>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let changes = ActionItemChanges {
        title: body.title,
        notes: body.notes,
        priority: body.priority,
        due_at: body.due_at,
        owner: body.owner,
        ..ActionItemChanges::default()
    };
    if changes.is_empty() {
        return Err(ApiError::BadRequest(
            "request body contains no fields to update".to_owned(),
        ));
    }
    let now = Utc::now();

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let updated = action_item::update(&mut connection, id, changes, Actor::User, now).await?;
    let item = publish_item_write(&state, &mut connection, updated, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_at_distinguishes_absent_from_null() {
        let absent: RequestBody =
            serde_json::from_value(json!({ "title": "Renamed" })).expect("parses");
        assert_eq!(absent.due_at, None);

        let cleared: RequestBody =
            serde_json::from_value(json!({ "dueAt": null })).expect("parses");
        assert_eq!(cleared.due_at, Some(None));
    }

    #[test]
    fn state_snooze_and_waiting_are_not_edited_here() {
        for field in ["state", "snoozedUntil", "waitingOn"] {
            serde_json::from_value::<RequestBody>(json!({ field: null }))
                .expect_err("each has a route of its own");
        }
    }
}
