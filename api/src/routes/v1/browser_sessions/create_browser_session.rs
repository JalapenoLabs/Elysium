// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/browser-sessions`: add a browser for the signed-in person to sign in to an
//! account in.
//!
//! Answers `201` at once with the browser `starting`: pulling Neko's image the first time
//! takes a minute or more. The browser opens `startUrl` once Chromium answers, and each
//! change of status arrives as `browserSession.upserted`.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;
use validator::{Validate, ValidationError};

use super::BrowserSessionResponse;
use crate::auth::CurrentUser;
use crate::browser::CreateError;
use crate::errors::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 100), custom(function = "validate_not_blank"))]
    name: String,
    #[validate(length(max = 2048), custom(function = "validate_start_url"))]
    start_url: String,
}

/// Rejects names that are only whitespace; `length` alone would accept `"   "`.
fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}

/// A sign-in page is always https, and naming it by host keeps the browser on the service
/// the person meant.
fn validate_start_url(value: &str) -> Result<(), ValidationError> {
    let parsed = Url::parse(value).ok();
    let is_https_host = parsed
        .as_ref()
        .is_some_and(|url| url.scheme() == "https" && url.domain().is_some());
    if is_https_host {
        return Ok(());
    }
    Err(ValidationError::new("start_url")
        .with_message("must be an https:// address with a host name".into()))
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;
    let start_url = Url::parse(&body.start_url)
        .map_err(|error| ApiError::BadRequest(format!("startUrl is not a URL: {error}")))?;

    let session = state
        .browsers
        .create(current.id(), body.name.trim().to_owned(), start_url)
        .await
        .map_err(|error| match error {
            CreateError::PortsExhausted => ApiError::CodedConflict {
                code: "browser_ports_exhausted",
                message: "every browser this host can stream is in use; remove one first",
            },
            CreateError::Internal(error) => ApiError::Internal(error),
        })?;

    // Creating it already published `browserSession.upserted`, as `starting`.
    let status = state.browsers.status(session.id);
    Ok((
        StatusCode::CREATED,
        Json(json!({ "browserSession": BrowserSessionResponse::new(session, status) })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: Value) -> Result<RequestBody, serde_json::Error> {
        serde_json::from_value(body)
    }

    #[test]
    fn a_named_https_sign_in_page_is_accepted() {
        let body = parse(json!({
            "name": "Work Outlook",
            "startUrl": "https://outlook.office.com/mail/",
        }))
        .expect("parses");
        body.validate().expect("valid");
    }

    #[test]
    fn plain_http_addresses_and_blank_names_are_refused() {
        for (name, start_url) in [
            ("Outlook", "http://outlook.office.com/mail/"),
            ("Outlook", "https://203.0.113.7/"),
            ("Outlook", "file:///etc/passwd"),
            ("Outlook", "not a url"),
            ("   ", "https://mail.google.com/"),
        ] {
            let body = parse(json!({ "name": name, "startUrl": start_url })).expect("parses");
            assert!(body.validate().is_err(), "{name} {start_url}");
        }
    }

    #[test]
    fn unknown_fields_are_refused() {
        let refused = parse(json!({
            "name": "Gmail",
            "startUrl": "https://mail.google.com/",
            "port": 59000,
        }));
        assert!(
            refused
                .expect_err("a port cannot be chosen")
                .to_string()
                .contains("unknown field")
        );
    }
}
