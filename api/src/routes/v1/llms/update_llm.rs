// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/llms/{id}`: change some fields; a new token is sealed afresh.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{SecretToken, response_for, validate_not_blank};
use crate::errors::ApiError;
use crate::models::llm::{self, LlmChanges, LlmType};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Absent fields stay as they are. `expiresAt: null` clears the expiry, which is
/// why that field distinguishes "absent" from "null".
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: Option<String>,
    #[validate(length(max = 2000))]
    description: Option<String>,
    #[serde(rename = "type")]
    llm_type: Option<LlmType>,
    secret_token: Option<SecretToken>,
    priority: Option<i32>,
    is_active: Option<bool>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and a timestamp are three distinct requests"
    )]
    expires_at: Option<Option<DateTime<Utc>>>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let changes = LlmChanges {
        name: body.name,
        description: body.description,
        type_: body.llm_type,
        secret_token: body.secret_token.map(|token| token.0),
        priority: body.priority,
        is_active: body.is_active,
        expires_at: body.expires_at,
    };

    if changes.is_empty() {
        return Err(ApiError::BadRequest(
            "request body contains no fields to update".to_owned(),
        ));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let current = llm::find(&mut connection, id).await?;
    refuse_account_token_changes(current.type_, &changes)?;
    let llm = llm::update(&mut connection, &state.cipher, id, &changes).await?;

    state.events.publish(&ServerEvent::LlmUpserted(
        response_for(&mut connection, llm.clone()).await?,
    ));
    let response = response_for(&mut connection, llm).await?;
    Ok(Json(json!({ "llm": response })))
}

/// A Claude account's token belongs to its sign-in and the keeper: it cannot be typed over or
/// retyped, and no other credential can become one.
fn refuse_account_token_changes(current: LlmType, changes: &LlmChanges) -> Result<(), ApiError> {
    let becomes_account = changes.type_ == Some(LlmType::ClaudeAccount);
    let is_account = current == LlmType::ClaudeAccount;
    if becomes_account && !is_account {
        return Err(ApiError::BadRequest(
            "a Claude account is added by signing in, not by changing a credential's type"
                .to_owned(),
        ));
    }
    if is_account
        && (changes.secret_token.is_some() || changes.type_.is_some_and(|type_| type_ != current))
    {
        return Err(ApiError::BadRequest(
            "a Claude account's token comes from signing in; sign in again to replace it"
                .to_owned(),
        ));
    }
    if is_account && changes.expires_at.is_some() {
        return Err(ApiError::BadRequest(
            "a Claude account does not expire while Elysium keeps it signed in".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;

    #[test]
    fn a_claude_accounts_token_type_and_expiry_cannot_be_changed_by_hand() {
        let token = LlmChanges {
            secret_token: Some(SecretString::from("typed")),
            ..LlmChanges::default()
        };
        refuse_account_token_changes(LlmType::ClaudeAccount, &token)
            .expect_err("its token comes from a sign-in");

        let retype = LlmChanges {
            type_: Some(LlmType::ClaudeApiToken),
            ..LlmChanges::default()
        };
        refuse_account_token_changes(LlmType::ClaudeAccount, &retype)
            .expect_err("it cannot become another type");

        let expiry = LlmChanges {
            expires_at: Some(None),
            ..LlmChanges::default()
        };
        refuse_account_token_changes(LlmType::ClaudeAccount, &expiry)
            .expect_err("the keeper owns its lifetime");

        let rename = LlmChanges {
            name: Some("Work".to_owned()),
            priority: Some(2),
            is_active: Some(false),
            ..LlmChanges::default()
        };
        refuse_account_token_changes(LlmType::ClaudeAccount, &rename)
            .expect("its name, place, and active flag change like any credential's");
    }

    #[test]
    fn no_other_credential_can_become_a_claude_account() {
        let become_account = LlmChanges {
            type_: Some(LlmType::ClaudeAccount),
            ..LlmChanges::default()
        };
        refuse_account_token_changes(LlmType::ClaudeCodeOauth, &become_account)
            .expect_err("accounts are added by signing in");
        refuse_account_token_changes(
            LlmType::ClaudeCodeOauth,
            &LlmChanges {
                secret_token: Some(SecretString::from("new setup-token")),
                ..LlmChanges::default()
            },
        )
        .expect("a setup-token is still replaced by hand");
    }

    #[test]
    fn expires_at_distinguishes_absent_from_null() {
        let absent: RequestBody =
            serde_json::from_value(json!({ "isActive": false })).expect("parses");
        assert_eq!(absent.expires_at, None);

        let cleared: RequestBody =
            serde_json::from_value(json!({ "expiresAt": null })).expect("parses");
        assert_eq!(cleared.expires_at, Some(None));

        let set: RequestBody =
            serde_json::from_value(json!({ "expiresAt": "2027-01-01T00:00:00Z" })).expect("parses");
        assert!(matches!(set.expires_at, Some(Some(_))));
    }

    #[test]
    fn present_fields_are_validated_and_absent_ones_are_not() {
        let empty: RequestBody = serde_json::from_value(json!({})).expect("parses");
        empty
            .validate()
            .expect("an empty body has nothing to validate");

        let blank_name: RequestBody =
            serde_json::from_value(json!({ "name": "" })).expect("parses");
        assert!(
            blank_name
                .validate()
                .expect_err("empty name is invalid")
                .field_errors()
                .contains_key("name")
        );
    }
}
