// Copyright © 2026 Jalapeno Labs

//! `/api/v1/llms`: LLM provider credentials. Tokens are write-only over HTTP.
//!
//! A `claude-account` credential is never created or retyped here: its token comes from a
//! sign-in through the official CLI (`/claude-sign-ins`), and the keeper in
//! `crate::claude_accounts` refreshes it. Its name, description, priority, and active flag
//! change like any other credential's.

mod cancel_claude_sign_in;
mod create_llm;
mod delete_llm;
mod finish_claude_sign_in;
mod get_llm;
mod list_llms;
mod start_claude_sign_in;
mod update_llm;

use std::collections::HashMap;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;
use validator::ValidationError;

use crate::claude_accounts::sign_in::SignInError;
use crate::errors::ApiError;
use crate::models::claude_account::{self, ClaudeAccount, UsageWindow};
use crate::models::llm::{self, Llm, LlmType};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_llms::handle).post(create_llm::handle))
        .route("/claude-sign-ins", post(start_claude_sign_in::handle))
        .route(
            "/claude-sign-ins/{id}",
            post(finish_claude_sign_in::handle).delete(cancel_claude_sign_in::handle),
        )
        .route(
            "/{id}",
            get(get_llm::handle)
                .patch(update_llm::handle)
                .delete(delete_llm::handle),
        )
}

/// Upper bound on a stored token. OAuth refresh bundles run a few kilobytes; this
/// leaves headroom while keeping one row from carrying an arbitrary blob.
const SECRET_TOKEN_MAX_BYTES: usize = 16 * 1024;

/// A credential as clients see it. The token is never included, sealed or not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmResponse {
    id: Uuid,
    name: String,
    description: String,
    #[serde(rename = "type")]
    llm_type: LlmType,
    priority: i32,
    is_active: bool,
    expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    /// Who created it.
    created_by: Uuid,
    updated_at: DateTime<Utc>,
    /// The signed-in account, for a `claude-account` credential; `null` for every other type,
    /// whose usage Elysium cannot read.
    account: Option<ClaudeAccountView>,
}

impl LlmResponse {
    /// `account` is the credential's Claude account, when it is one.
    pub fn new(llm: Llm, account: Option<&ClaudeAccount>) -> Self {
        Self {
            id: llm.id,
            name: llm.name,
            description: llm.description,
            llm_type: llm.type_,
            priority: llm.priority,
            is_active: llm.is_active,
            expires_at: llm.expires_at,
            created_at: llm.created_at,
            created_by: llm.created_by,
            updated_at: llm.updated_at,
            account: account.map(ClaudeAccountView::from),
        }
    }
}

/// A signed-in Claude account. No token, sealed or not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccountView {
    email: String,
    organization_name: Option<String>,
    subscription_type: Option<String>,
    /// `false` once Anthropic refused the refresh token; only signing in again fixes it.
    is_signed_in: bool,
    access_token_expires_at: DateTime<Utc>,
    last_refreshed_at: Option<DateTime<Utc>>,
    last_refresh_error: Option<String>,
    usage: UsageView,
    usage_error: Option<String>,
}

impl From<&ClaudeAccount> for ClaudeAccountView {
    fn from(account: &ClaudeAccount) -> Self {
        Self {
            email: account.email.clone(),
            organization_name: account.organization_name.clone(),
            subscription_type: account.subscription_type.clone(),
            is_signed_in: account.is_signed_in(),
            access_token_expires_at: account.access_token_expires_at,
            last_refreshed_at: account.last_refreshed_at,
            last_refresh_error: account.last_refresh_error.clone(),
            usage: UsageView::from(account),
            usage_error: account.usage_error.clone(),
        }
    }
}

/// An account's latest usage: numbers and instants only, so it can travel on the event stream.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageView {
    five_hour: Option<UsageWindowView>,
    seven_day: Option<UsageWindowView>,
    /// When Anthropic last answered; `null` before the first read.
    read_at: Option<DateTime<Utc>>,
}

/// One window, as a percentage used and when it resets.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindowView {
    utilization: f64,
    resets_at: Option<DateTime<Utc>>,
}

impl From<&ClaudeAccount> for UsageView {
    fn from(account: &ClaudeAccount) -> Self {
        let usage = account.usage();
        let view = |window: UsageWindow| UsageWindowView {
            utilization: window.utilization,
            resets_at: window.resets_at,
        };
        Self {
            five_hour: usage.five_hour.map(view),
            seven_day: usage.seven_day.map(view),
            read_at: account.usage_read_at,
        }
    }
}

/// Every credential as clients see it, each with its account when it has one.
///
/// # Errors
/// Propagates any database error.
pub async fn list_responses(
    connection: &mut diesel_async::AsyncPgConnection,
) -> diesel::QueryResult<Vec<LlmResponse>> {
    let llms = llm::list(connection).await?;
    let accounts: HashMap<Uuid, ClaudeAccount> = claude_account::list(connection)
        .await?
        .into_iter()
        .map(|account| (account.llm_id, account))
        .collect();
    Ok(llms
        .into_iter()
        .map(|llm| {
            let account = accounts.get(&llm.id);
            LlmResponse::new(llm, account)
        })
        .collect())
}

/// One credential as clients see it, with its account when it has one.
///
/// # Errors
/// Propagates any database error.
pub async fn response_for(
    connection: &mut diesel_async::AsyncPgConnection,
    llm: Llm,
) -> diesel::QueryResult<LlmResponse> {
    let account = claude_account::find(connection, llm.id).await?;
    Ok(LlmResponse::new(llm, account.as_ref()))
}

/// A sign-in's failure as an HTTP answer. The CLI's refusal (a wrong code, an ended sign-in) is
/// the person's to fix; a down workspace is upstream.
fn sign_in_error(error: SignInError) -> ApiError {
    match error {
        SignInError::NotFound => ApiError::NotFound,
        SignInError::TooMany => {
            ApiError::Conflict("too many sign-ins are waiting; finish or cancel one first")
        }
        SignInError::Refused(reason) => ApiError::BadRequest(reason),
        SignInError::Workspace(error) => ApiError::BadGateway(error.to_string()),
    }
}

/// Rejects names that are only whitespace; `length` alone would accept `"   "`.
fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}

/// A token from a request body, checked as it is parsed.
///
/// Validation happens during deserialization rather than through `validator`,
/// because a validator error report would need to serialize the value itself.
/// The plaintext moves straight into a [`SecretString`], which zeroizes on drop,
/// so a rejected token is wiped as soon as the error is returned.
#[derive(Debug)]
pub struct SecretToken(pub SecretString);

impl<'de> Deserialize<'de> for SecretToken {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        let token = SecretString::from(String::deserialize(deserializer)?);

        if token.expose_secret().trim().is_empty() {
            return Err(serde::de::Error::custom("secretToken must not be blank"));
        }
        if token.expose_secret().len() > SECRET_TOKEN_MAX_BYTES {
            return Err(serde::de::Error::custom("secretToken exceeds 16 KiB"));
        }

        Ok(Self(token))
    }
}
