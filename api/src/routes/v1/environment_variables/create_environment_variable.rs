// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/environment-variables`: add a variable, sealing its value.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::{EMPTY_SECRET_MESSAGE, EnvironmentVariableResponse, VariableValue, check_key};
use crate::errors::ApiError;
use crate::models::environment_variable::{self, NewEnvironmentVariable};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// `isSecret` has no default: whether a value may be shown again is a choice the client
/// makes out loud.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    key: String,
    value: VariableValue,
    is_secret: bool,
    #[serde(default)]
    #[validate(length(max = 500))]
    description: String,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;
    check_key(&body.key)?;

    if body.is_secret && body.value.0.expose_secret().is_empty() {
        return Err(ApiError::BadRequest(EMPTY_SECRET_MESSAGE.to_owned()));
    }

    let new_variable = NewEnvironmentVariable {
        key: body.key,
        value: body.value.0,
        is_secret: body.is_secret,
        description: body.description,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let variable =
        environment_variable::create(&mut connection, &state.cipher, &new_variable).await?;
    drop(connection);

    let response = EnvironmentVariableResponse::new(variable, &state.cipher)?;
    state
        .events
        .publish(&ServerEvent::EnvironmentVariableUpserted(response.clone()));

    Ok((StatusCode::CREATED, Json(json!({ "variable": response }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: Value) -> Result<RequestBody, serde_json::Error> {
        serde_json::from_value(body)
    }

    #[test]
    fn a_variable_parses_with_its_description_defaulting_to_empty() {
        let body = parse(json!({ "key": "NODE_ENV", "value": "production", "isSecret": false }))
            .expect("parses");

        body.validate().expect("valid");
        assert_eq!(body.key, "NODE_ENV");
        assert_eq!(body.value.0.expose_secret(), "production");
        assert!(!body.is_secret);
        assert_eq!(body.description, "");
    }

    #[test]
    fn the_secret_flag_is_required_and_descriptions_are_bounded() {
        parse(json!({ "key": "NPM_TOKEN", "value": "npm_abc" })).expect_err("no isSecret");

        let long = parse(json!({
            "key": "NPM_TOKEN",
            "value": "npm_abc",
            "isSecret": true,
            "description": "d".repeat(501),
        }))
        .expect("parses");
        assert!(
            long.validate()
                .expect_err("a 501-character description")
                .field_errors()
                .contains_key("description")
        );
    }

    #[test]
    fn values_never_reach_debug_output() {
        let body =
            parse(json!({ "key": "NPM_TOKEN", "value": "npm_secretvalue", "isSecret": true }))
                .expect("parses");

        assert!(!format!("{body:?}").contains("secretvalue"));
    }
}
