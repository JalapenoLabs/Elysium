// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/action-items/{id}/wait`: record that someone else owes the next step.
//!
//! `on` names them, by name or address; `null` stops waiting. A waiting item leaves Next
//! and lists under Waiting, keeping its state and its place for when the wait ends.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{publish_item_write, validate_person};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item::{self, ActionItemChanges};
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// Required, and `null` to stop waiting. Naming the deserializer keeps serde from
    /// treating an absent field as `null`, so a body that forgot it is refused.
    #[serde(deserialize_with = "Option::deserialize")]
    #[validate(custom(function = "validate_person"))]
    on: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;
    let now = Utc::now();

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let changes = ActionItemChanges {
        waiting_on: Some(body.on),
        ..ActionItemChanges::default()
    };
    let waiting = action_item::update(&mut connection, id, changes, Actor::User, now).await?;
    let item = publish_item_write(&state.events, &mut connection, waiting, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_names_someone_or_is_null() {
        let named: RequestBody =
            serde_json::from_value(json!({ "on": "sam@example.com" })).expect("parses");
        named.validate().expect("valid");

        let cleared: RequestBody = serde_json::from_value(json!({ "on": null })).expect("parses");
        cleared.validate().expect("null stops waiting");
        assert_eq!(cleared.on, None);

        let blank: RequestBody = serde_json::from_value(json!({ "on": "  " })).expect("parses");
        blank.validate().expect_err("blank is refused");

        serde_json::from_value::<RequestBody>(json!({})).expect_err("on is required");
    }
}
