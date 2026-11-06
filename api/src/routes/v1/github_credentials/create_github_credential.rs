// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/github-credentials`: add a token, sealing it once GitHub accepts it.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{
    GithubCredentialResponse, GithubToken, matches_kind, token_shape_message, validate_not_blank,
};
use crate::auth::CurrentUser;
use crate::connectors::kinds::github as github_kind;
use crate::errors::ApiError;
use crate::models::github_credential::{
    self, GithubCredential, GithubTokenKind, NewGithubCredential, VerifiedToken,
};
use crate::realtime::ServerEvent;
use crate::routes::v1::jira_credentials::Selection;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: String,
    kind: GithubTokenKind,
    token: GithubToken,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    // Settings, GitHub makes a connector that reads every repository the token reaches; which
    // ones is chosen on the connector's page.
    let credential = create_credential(&state, current.id(), body, &Selection::All).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "credential": GithubCredentialResponse::new(credential) })),
    ))
}

/// Creates a credential, and with it its GitHub connector reading `repositories`, for
/// `created_by`, once GitHub has accepted the token. The hub's GitHub connectors are made here
/// too (`crate::connectors::kinds::github`), so both check exactly the same things.
///
/// # Errors
/// [`ApiError::Validation`] for fields out of shape, [`ApiError::BadRequest`] for a token that
/// is not shaped like its kind or that GitHub refuses, or a repository it does not reach, and
/// [`ApiError::BadGateway`] when GitHub cannot be reached.
pub async fn create_credential(
    state: &AppState,
    created_by: Uuid,
    body: RequestBody,
    repositories: &Selection<String>,
) -> Result<GithubCredential, ApiError> {
    body.validate()?;

    if !matches_kind(body.kind, &body.token.0) {
        return Err(ApiError::BadRequest(
            token_shape_message(body.kind).to_owned(),
        ));
    }

    // A token is stored only once GitHub has accepted it, so every row names the account
    // it acts as and says when it expires.
    let account = state.github.verify(&body.token.0).await?;
    // A pick is checked against what the token reaches before anything is stored, and stored as
    // GitHub spells it.
    let reachable = match repositories {
        Selection::All => Vec::new(),
        Selection::Only(_picked) => {
            state
                .github
                .list_repositories(&body.token.0)
                .await?
                .repositories
        }
    };
    let new_credential = NewGithubCredential {
        created_by,
        name: body.name,
        connector_content: github_kind::stored_content(repositories, &reachable)?,
        verified: VerifiedToken {
            kind: body.kind,
            token: body.token.0,
            account,
        },
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credential =
        github_credential::create(&mut connection, &state.cipher, &new_credential).await?;
    drop(connection);

    state.events.publish(&ServerEvent::GithubCredentialUpserted(
        GithubCredentialResponse::new(credential.clone()),
    ));
    Ok(credential)
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    fn parse(body: Value) -> Result<RequestBody, serde_json::Error> {
        serde_json::from_value(body)
    }

    #[test]
    fn a_fine_grained_token_parses_with_its_name_and_kind() {
        let body = parse(json!({
            "name": "Work",
            "kind": "fine-grained",
            "token": "github_pat_11ABCDEFG0abcdefg",
        }))
        .expect("parses");

        body.validate().expect("valid");
        assert_eq!(body.kind, GithubTokenKind::FineGrained);
        assert_eq!(body.token.0.expose_secret(), "github_pat_11ABCDEFG0abcdefg");
    }

    #[test]
    fn unknown_kinds_and_blank_names_are_refused() {
        parse(json!({ "name": "Work", "kind": "oauth", "token": "ghp_abc" }))
            .expect_err("an unknown kind");

        let blank =
            parse(json!({ "name": "  ", "kind": "classic", "token": "ghp_abc" })).expect("parses");
        assert!(
            blank
                .validate()
                .expect_err("a blank name")
                .field_errors()
                .contains_key("name")
        );
    }

    #[test]
    fn tokens_never_reach_debug_output() {
        let body = parse(json!({
            "name": "Work",
            "kind": "classic",
            "token": "ghp_secretvalue",
        }))
        .expect("parses");

        assert!(!format!("{body:?}").contains("secretvalue"));
    }
}
