// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/jira-credentials/discover`: what a token can reach, before anything is
//! stored.
//!
//! This is step one of adding a credential. Nothing is written: the token is checked, and
//! the projects and boards it can see come back so the user picks from what exists rather
//! than typing a key. Step two, `POST /api/v1/jira-credentials`, checks the picks again.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::{JiraToken, reach, validate_email_address};
use crate::errors::ApiError;
use crate::jira::{Site, normalize_site_url};
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    site_url: String,
    #[validate(custom(function = "validate_email_address"))]
    account_email: String,
    token: JiraToken,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let site_url = normalize_site_url(&body.site_url)
        .map_err(|advice| ApiError::BadRequest(advice.to_owned()))?;
    let site = Site {
        url: &site_url,
        email: body.account_email.trim(),
        token: &body.token.0,
    };
    let reachable = reach(&state.jira, &site).await?;

    Ok(Json(json!({
        "account": reachable.account,
        "projects": reachable.projects,
        "boards": reachable.boards,
        "projectsTruncated": reachable.projects_truncated,
        "boardsTruncated": reachable.boards_truncated,
    })))
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    #[test]
    fn a_site_an_address_and_a_token_parse_together() {
        let body: RequestBody = serde_json::from_value(json!({
            "siteUrl": "https://acme.atlassian.net",
            "accountEmail": "alex@example.com",
            "token": "ATATTsecret",
        }))
        .expect("parses");

        body.validate().expect("valid");
        assert_eq!(body.token.0.expose_secret(), "ATATTsecret");
        assert!(!format!("{body:?}").contains("secret"));
    }

    #[test]
    fn an_address_that_is_not_one_is_refused_before_jira_is_called() {
        let body: RequestBody = serde_json::from_value(json!({
            "siteUrl": "https://acme.atlassian.net",
            "accountEmail": "alex",
            "token": "ATATTsecret",
        }))
        .expect("parses");

        assert!(
            body.validate()
                .expect_err("not an address")
                .field_errors()
                .contains_key("account_email")
        );
    }
}
