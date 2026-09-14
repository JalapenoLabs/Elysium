// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/satellites`: register a satellite, sealing its secret, and start
//! watching it.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::{SatelliteResponse, SatelliteSecret, validate_http_scheme, validate_not_blank};
use crate::errors::ApiError;
use crate::models::satellite::{self, NewSatellite};
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
    #[validate(url, length(max = 2048), custom(function = "validate_http_scheme"))]
    url: String,
    secret: SatelliteSecret,
    #[serde(default = "default_is_active")]
    is_active: bool,
}

const fn default_is_active() -> bool {
    true
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let new_satellite = NewSatellite {
        name: body.name,
        description: body.description,
        url: body.url,
        secret: body.secret.0,
        is_active: body.is_active,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let satellite = satellite::create(&mut connection, &state.cipher, &new_satellite).await?;
    drop(connection);

    state.fleet.reload_satellite(&satellite).await?;
    state
        .events
        .publish(&ServerEvent::SatelliteUpserted(SatelliteResponse::new(
            satellite.clone(),
            None,
        )));

    Ok((
        StatusCode::CREATED,
        Json(json!({ "satellite": SatelliteResponse::new(satellite, None) })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: Value) -> Result<RequestBody, serde_json::Error> {
        serde_json::from_value(body)
    }

    #[test]
    fn minimal_body_applies_defaults() {
        let body = parse(json!({ "name": "Orbit", "url": "http://arsox:8080", "secret": "abc" }))
            .expect("minimal body parses");

        body.validate().expect("minimal body is valid");
        assert!(body.is_active);
        assert_eq!(body.description, "");
    }

    #[test]
    fn urls_must_be_http_and_well_formed() {
        let schemeless =
            parse(json!({ "name": "A", "url": "ftp://arsox", "secret": "x" })).expect("parses");
        assert!(
            schemeless
                .validate()
                .expect_err("ftp is refused")
                .field_errors()
                .contains_key("url")
        );

        let garbage =
            parse(json!({ "name": "A", "url": "not a url", "secret": "x" })).expect("parses");
        assert!(garbage.validate().is_err(), "a non-URL is refused");

        let https = parse(json!({ "name": "A", "url": "https://sat.example.com", "secret": "x" }))
            .expect("parses");
        https.validate().expect("https is accepted");
    }

    #[test]
    fn blank_and_oversized_secrets_are_rejected_while_parsing() {
        let blank = parse(json!({ "name": "A", "url": "http://a", "secret": " " }));
        assert!(
            blank
                .expect_err("blank secret")
                .to_string()
                .contains("must not be blank")
        );

        let oversized =
            parse(json!({ "name": "A", "url": "http://a", "secret": "x".repeat(1025) }));
        assert!(
            oversized
                .expect_err("oversized secret")
                .to_string()
                .contains("exceeds 1 KiB")
        );
    }

    #[test]
    fn debug_output_never_reveals_the_secret() {
        let body = parse(json!({ "name": "A", "url": "http://a", "secret": "hunter2-bearer" }))
            .expect("parses");
        assert!(!format!("{body:?}").contains("hunter2-bearer"));
    }
}
