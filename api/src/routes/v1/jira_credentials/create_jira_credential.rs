// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/jira-credentials`: add a credential, sealing its token once Jira accepts
//! it and the picks it was given.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::{
    JiraCredentialResponse, JiraToken, Selection, reach, validate_email_address, validate_not_blank,
};
use crate::errors::ApiError;
use crate::jira::{Site, normalize_site_url};
use crate::models::jira_credential::{self, Allowed, NewJiraCredential, VerifiedToken};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: String,
    site_url: String,
    #[validate(custom(function = "validate_email_address"))]
    account_email: String,
    token: JiraToken,
    /// `"*"`, or the projects to allow, each named by its Jira id or its key.
    projects: Selection<String>,
    /// `"*"`, or the boards to allow, each named by its Jira id.
    boards: Selection<i64>,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let site_url = normalize_site_url(&body.site_url)
        .map_err(|advice| ApiError::BadRequest(advice.to_owned()))?;
    let account_email = body.account_email.trim().to_owned();

    // The token is stored only once Jira has accepted it, and the picks only once Jira has
    // reported them, so no row ever promises access that was never proven.
    let reachable = reach(
        &state.jira,
        &Site {
            url: &site_url,
            email: &account_email,
            token: &body.token.0,
        },
    )
    .await?;
    let allowed = Allowed {
        projects: reachable.choose_projects(&body.projects)?,
        boards: reachable.choose_boards(&body.boards)?,
    };

    let new_credential = NewJiraCredential {
        name: body.name,
        verified: VerifiedToken {
            site_url,
            account_email,
            token: body.token.0,
            account: reachable.account,
        },
        allowed: allowed.clone(),
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credential =
        jira_credential::create(&mut connection, &state.cipher, &new_credential).await?;
    drop(connection);

    let response = JiraCredentialResponse::new(credential.clone(), allowed.clone());
    state
        .events
        .publish(&ServerEvent::JiraCredentialUpserted(response));

    Ok((
        StatusCode::CREATED,
        Json(json!({ "credential": JiraCredentialResponse::new(credential, allowed) })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: Value) -> Result<RequestBody, serde_json::Error> {
        serde_json::from_value(body)
    }

    #[test]
    fn a_credential_parses_with_its_picks() {
        let body = parse(json!({
            "name": "Work",
            "siteUrl": "https://acme.atlassian.net",
            "accountEmail": "alex@example.com",
            "token": "ATATTsecret",
            "projects": ["ELY"],
            "boards": "*",
        }))
        .expect("parses");

        body.validate().expect("valid");
        assert_eq!(body.projects, Selection::Only(vec!["ELY".to_owned()]));
        assert_eq!(body.boards, Selection::All);
        assert!(!format!("{body:?}").contains("secret"));
    }

    #[test]
    fn picks_are_required_and_unknown_fields_are_refused() {
        parse(json!({
            "name": "Work",
            "siteUrl": "https://acme.atlassian.net",
            "accountEmail": "alex@example.com",
            "token": "ATATTsecret",
        }))
        .expect_err("a credential says what it may touch");

        parse(json!({
            "name": "Work",
            "siteUrl": "https://acme.atlassian.net",
            "accountEmail": "alex@example.com",
            "token": "ATATTsecret",
            "projects": "*",
            "boards": "*",
            "isDefault": true,
        }))
        .expect_err("an unknown field");
    }

    #[test]
    fn a_blank_name_is_refused() {
        let body = parse(json!({
            "name": "   ",
            "siteUrl": "https://acme.atlassian.net",
            "accountEmail": "alex@example.com",
            "token": "ATATTsecret",
            "projects": "*",
            "boards": "*",
        }))
        .expect("parses");

        assert!(
            body.validate()
                .expect_err("a blank name")
                .field_errors()
                .contains_key("name")
        );
    }
}
