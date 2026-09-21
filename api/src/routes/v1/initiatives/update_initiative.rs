// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/initiatives/{id}`: rename, redescribe, retarget, or change the state of
//! an initiative: `active`, `achieved`, or `abandoned`, in any direction.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::publish_initiative_write;
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::initiative::{self, InitiativeChanges, InitiativeState};
use crate::routes::v1::action_items::validate_not_blank;
use crate::state::AppState;

/// Absent fields stay as they are; `targetAt: null` removes the target.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 200), custom(function = "validate_not_blank"))]
    name: Option<String>,
    #[validate(length(max = 20000))]
    description: Option<String>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and a date are three distinct requests"
    )]
    target_at: Option<Option<DateTime<Utc>>>,
    state: Option<InitiativeState>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let changes = InitiativeChanges {
        name: body.name,
        description: body.description,
        target_at: body.target_at,
        state: body.state,
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
    let updated = initiative::update(&mut connection, id, changes, Actor::User, now).await?;
    let initiative = publish_initiative_write(&state.events, &mut connection, updated, now).await?;

    Ok(Json(json!({ "initiative": initiative })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_at_distinguishes_absent_from_null_and_states_must_be_known() {
        let absent: RequestBody =
            serde_json::from_value(json!({ "state": "achieved" })).expect("parses");
        assert_eq!(absent.target_at, None);
        assert_eq!(absent.state, Some(InitiativeState::Achieved));

        let cleared: RequestBody =
            serde_json::from_value(json!({ "targetAt": null })).expect("parses");
        assert_eq!(cleared.target_at, Some(None));

        serde_json::from_value::<RequestBody>(json!({ "state": "paused" }))
            .expect_err("unknown state");
    }
}
