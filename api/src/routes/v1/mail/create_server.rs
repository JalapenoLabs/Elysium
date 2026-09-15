// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/mail/server`: create the mail server, with its first domain.
//!
//! Answers `202` at once with the server `creating`; progress and the outcome arrive as
//! `mailServer.updated` events. See [`crate::mail::hosting`].

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::validate_domain;
use crate::errors::ApiError;
use crate::mail::hosting::StartRefused;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// The name the server answers as, such as `mail.example.com`.
    #[validate(length(min = 3, max = 253), custom(function = "validate_domain"))]
    hostname: String,
    /// The first domain the server hosts, such as `example.com`.
    #[validate(length(min = 3, max = 253), custom(function = "validate_domain"))]
    domain: String,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let hosting = &state.mail.hosting;
    hosting
        .start(body.hostname.to_lowercase(), body.domain.to_lowercase())
        .await
        .map_err(|refused| match refused {
            StartRefused::AlreadyExists => ApiError::Conflict("a mail server already exists"),
            StartRefused::InProgress => {
                ApiError::Conflict("the mail server is already being created")
            }
            StartRefused::Database(error) => ApiError::Internal(error),
        })?;

    let server = hosting.status().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "server": server }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_and_domain_must_be_domain_names() {
        let valid: RequestBody = serde_json::from_value(
            json!({ "hostname": "mail.example.com", "domain": "example.com" }),
        )
        .expect("parses");
        valid.validate().expect("valid");

        let invalid: RequestBody =
            serde_json::from_value(json!({ "hostname": "localhost", "domain": "exa mple.com" }))
                .expect("parses");
        let errors = invalid.validate().expect_err("invalid");
        assert!(errors.field_errors().contains_key("hostname"));
        assert!(errors.field_errors().contains_key("domain"));
    }
}
