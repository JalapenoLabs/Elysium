// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/github-credentials/{id}`: rename a credential, or give it a new token.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{
    GithubCredentialResponse, GithubToken, matches_kind, token_shape_message, validate_not_blank,
};
use crate::errors::ApiError;
use crate::models::github_credential::{
    self, GithubCredentialChanges, GithubTokenKind, VerifiedToken,
};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Absent fields stay as they are. A token is never edited in place: a new one is checked
/// with GitHub and replaces the account, scopes, and expiry along with it.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: Option<String>,
    kind: Option<GithubTokenKind>,
    token: Option<GithubToken>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let stored = github_credential::find(&mut connection, id).await?;

    // A stored token is one kind of token, so changing the kind means bringing the token
    // that goes with it.
    let verified = match body.token {
        Some(token) => {
            let kind = body.kind.unwrap_or(stored.kind);
            if !matches_kind(kind, &token.0) {
                return Err(ApiError::BadRequest(token_shape_message(kind).to_owned()));
            }
            let account = state.github.verify(&token.0).await?;
            Some(VerifiedToken {
                kind,
                token: token.0,
                account,
            })
        }
        None if body.kind.is_some_and(|kind| kind != stored.kind) => {
            return Err(ApiError::BadRequest(
                "a different kind of token needs the token itself".to_owned(),
            ));
        }
        None => None,
    };

    let changes = GithubCredentialChanges {
        name: body.name,
        verified,
    };

    // A kind that matches the stored one changes nothing, so a body of only that leaves
    // nothing to write.
    if changes.is_empty() {
        return Err(ApiError::BadRequest(
            "request body contains no fields to update".to_owned(),
        ));
    }
    let credential =
        github_credential::update(&mut connection, &state.cipher, id, &changes).await?;
    drop(connection);

    state.events.publish(&ServerEvent::GithubCredentialUpserted(
        GithubCredentialResponse::new(credential.clone()),
    ));

    Ok(Json(
        json!({ "credential": GithubCredentialResponse::new(credential) }),
    ))
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
        assert!(empty.name.is_none() && empty.kind.is_none() && empty.token.is_none());

        let blank: RequestBody = serde_json::from_value(json!({ "name": " " })).expect("parses");
        assert!(
            blank
                .validate()
                .expect_err("a blank name")
                .field_errors()
                .contains_key("name")
        );
    }

    #[test]
    fn a_kind_alone_parses_so_the_handler_can_refuse_it_with_a_reason() {
        let body: RequestBody =
            serde_json::from_value(json!({ "kind": "classic" })).expect("parses");

        assert_eq!(body.kind, Some(GithubTokenKind::Classic));
        assert!(body.token.is_none());
    }
}
