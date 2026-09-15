// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/projects`: create a project.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::{ProjectResponse, validate_not_blank};
use crate::errors::ApiError;
use crate::models::project::{self, NewProject};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: String,
    #[serde(default)]
    #[validate(length(max = 2000))]
    description: String,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let new_project = NewProject {
        name: body.name,
        description: body.description,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let project = project::create(&mut connection, &new_project).await?;

    state
        .events
        .publish(&ServerEvent::ProjectUpserted(ProjectResponse::from(
            project.clone(),
        )));

    Ok((
        StatusCode::CREATED,
        Json(json!({ "project": ProjectResponse::from(project) })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_required_and_descriptions_default_to_empty() {
        let minimal: RequestBody =
            serde_json::from_value(json!({ "name": "Elysium" })).expect("parses");
        minimal.validate().expect("a name alone is valid");
        assert_eq!(minimal.description, "");

        let blank: RequestBody = serde_json::from_value(json!({ "name": "   " })).expect("parses");
        assert!(
            blank
                .validate()
                .expect_err("a blank name is invalid")
                .field_errors()
                .contains_key("name")
        );

        let missing = serde_json::from_value::<RequestBody>(json!({ "description": "x" }));
        assert!(missing.is_err(), "a body without a name does not parse");
    }
}
