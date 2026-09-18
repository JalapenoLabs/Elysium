// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/action-items/{id}/snooze`: hide an item from Next until a moment passes.
//!
//! `until` must be in the future; `null` wakes the item now. Snoozing sits beside the
//! state, so the item keeps its place when the snooze ends.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::publish_item_write;
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item::{self, ActionItemChanges};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// Required, and `null` to end a snooze. Naming the deserializer keeps serde from
    /// treating an absent field as `null`, so a body that forgot it is refused.
    #[serde(deserialize_with = "Option::deserialize")]
    until: Option<DateTime<Utc>>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let now = Utc::now();

    if body.until.is_some_and(|until| until <= now) {
        return Err(ApiError::BadRequest(
            "until must be in the future".to_owned(),
        ));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let changes = ActionItemChanges {
        snoozed_until: Some(body.until),
        ..ActionItemChanges::default()
    };
    let snoozed = action_item::update(&mut connection, id, changes, Actor::User, now).await?;
    let item = publish_item_write(&state, &mut connection, snoozed, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn until_is_a_moment_with_an_offset_or_null() {
        let cleared: RequestBody =
            serde_json::from_value(json!({ "until": null })).expect("parses");
        assert_eq!(cleared.until, None);

        let set: RequestBody =
            serde_json::from_value(json!({ "until": "2026-10-01T09:00:00Z" })).expect("parses");
        assert!(set.until.is_some());

        serde_json::from_value::<RequestBody>(json!({})).expect_err("until is required");
        serde_json::from_value::<RequestBody>(json!({ "until": "2026-10-01T09:00:00" }))
            .expect_err("no offset");
    }
}
