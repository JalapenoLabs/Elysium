// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/environment-variables/{id}`: change a variable's key, value, secrecy, or
//! description.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{EMPTY_SECRET_MESSAGE, EnvironmentVariableResponse, VariableValue, check_key};
use crate::errors::ApiError;
use crate::models::environment_variable::{self, EnvironmentVariableChanges};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Absent fields stay as they are. A value is never returned for editing, so an absent
/// value keeps the sealed one.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    key: Option<String>,
    value: Option<VariableValue>,
    is_secret: Option<bool>,
    #[validate(length(max = 500))]
    description: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;
    if let Some(key) = &body.key {
        check_key(key)?;
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let stored = environment_variable::find(&mut connection, id).await?;
    let is_secret = body.is_secret.unwrap_or(stored.is_secret);

    match &body.value {
        Some(value) if is_secret && value.0.expose_secret().is_empty() => {
            return Err(ApiError::BadRequest(EMPTY_SECRET_MESSAGE.to_owned()));
        }
        Some(_value) => {}
        // A secret was never shown, and flipping the flag must not be a way to show it. The
        // value has to be sent again, which proves the client already knows it.
        None if stored.is_secret && !is_secret => {
            return Err(ApiError::BadRequest(
                "a secret variable becomes visible only with its value sent again".to_owned(),
            ));
        }
        // Becoming secret keeps the stored value, which is only allowed when there is one.
        None if is_secret && !stored.is_secret => {
            let stored_value = stored.value(&state.cipher).with_context(|| {
                format!("environment variable {} could not be decrypted", stored.key)
            })?;
            if stored_value.expose_secret().is_empty() {
                return Err(ApiError::BadRequest(EMPTY_SECRET_MESSAGE.to_owned()));
            }
        }
        None => {}
    }

    let changes = EnvironmentVariableChanges {
        key: body.key,
        value: body.value.map(|value| value.0),
        // A flag that matches the stored one changes nothing.
        is_secret: body.is_secret.filter(|flag| *flag != stored.is_secret),
        description: body.description,
    };

    if changes.is_empty() {
        return Err(ApiError::BadRequest(
            "request body contains no fields to update".to_owned(),
        ));
    }
    let variable =
        environment_variable::update(&mut connection, &state.cipher, id, &changes).await?;
    drop(connection);

    let response = EnvironmentVariableResponse::new(variable, &state.cipher)?;
    state
        .events
        .publish(&ServerEvent::EnvironmentVariableUpserted(response.clone()));

    Ok(Json(json!({ "variable": response })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_is_optional_and_present_ones_are_validated() {
        let empty: RequestBody = serde_json::from_value(json!({})).expect("parses");
        empty
            .validate()
            .expect("an empty body has nothing to validate");
        assert!(
            empty.key.is_none()
                && empty.value.is_none()
                && empty.is_secret.is_none()
                && empty.description.is_none()
        );

        let long: RequestBody =
            serde_json::from_value(json!({ "description": "d".repeat(501) })).expect("parses");
        assert!(
            long.validate()
                .expect_err("a 501-character description")
                .field_errors()
                .contains_key("description")
        );
    }

    #[test]
    fn a_flag_alone_parses_so_the_handler_can_decide_whether_it_needs_the_value() {
        let body: RequestBody =
            serde_json::from_value(json!({ "isSecret": false })).expect("parses");

        assert_eq!(body.is_secret, Some(false));
        assert!(body.value.is_none());
    }
}
