// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/initiatives`: start an initiative. It starts `active`, with no items.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::publish_initiative_write;
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::initiative::{self, NewInitiative};
use crate::routes::v1::action_items::{refuse_unknown_projects, unique_ids, validate_not_blank};
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 200), custom(function = "validate_not_blank"))]
    name: String,
    #[serde(default)]
    #[validate(length(max = 20000))]
    description: String,
    target_at: Option<DateTime<Utc>>,
    #[serde(default)]
    project_ids: Vec<Uuid>,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let new_initiative = NewInitiative {
        name: body.name,
        description: body.description,
        target_at: body.target_at,
        project_ids: unique_ids(body.project_ids),
    };
    let now = Utc::now();

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let created = initiative::create(&mut connection, new_initiative, Actor::User, now)
        .await
        .map_err(refuse_unknown_projects)?;
    let initiative = publish_initiative_write(&state, &mut connection, created, now).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "initiative": initiative })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_alone_makes_an_initiative_and_blank_names_are_refused() {
        let body: RequestBody =
            serde_json::from_value(json!({ "name": "Ship the storage page" })).expect("parses");
        body.validate().expect("valid");
        assert_eq!(body.target_at, None);

        let blank: RequestBody = serde_json::from_value(json!({ "name": "  " })).expect("parses");
        blank.validate().expect_err("blank");

        serde_json::from_value::<RequestBody>(
            json!({ "name": "A", "targetAt": "2026-12-01T00:00:00" }),
        )
        .expect_err("a target without an offset is ambiguous");
        serde_json::from_value::<RequestBody>(json!({ "name": "A", "state": "achieved" }))
            .expect_err("new initiatives start active");
    }
}
