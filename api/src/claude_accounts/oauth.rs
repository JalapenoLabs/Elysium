// Copyright © 2026 Jalapeno Labs

//! The Anthropic OAuth endpoints a Claude account is kept alive with: refresh, profile, usage.
//!
//! These are the endpoints Claude Code itself calls, with its own public client id. None is a
//! documented public API, so every answer is checked for shape and every failure becomes an
//! [`OAuthError`]. They are only ever called for accounts the official CLI signed in, to keep
//! the tokens it minted alive; Elysium never starts an OAuth flow of its own.

use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::usage;
use crate::models::claude_account::{Identity, Tokens, UsageWindows};

/// Where the profile and usage endpoints answer.
const API_ORIGIN: &str = "https://api.anthropic.com";
/// Where tokens are traded.
const TOKEN_ORIGIN: &str = "https://platform.claude.com";
const PROFILE_PATH: &str = "/api/oauth/profile";
const USAGE_PATH: &str = "/api/oauth/usage";
const TOKEN_PATH: &str = "/v1/oauth/token";
/// The beta the profile and usage endpoints require, as Claude Code sends it.
const OAUTH_BETA: &str = "oauth-2025-04-20";
/// Claude Code's public OAuth client id. A token must be refreshed by the client that minted
/// it, and the CLI minted these.
pub const CLAUDE_CODE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// Each endpoint answers in well under a second; this bounds a stalled one.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// An OAuth endpoint refused a call or could not be reached.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct OAuthError {
    /// The HTTP status, when Anthropic answered at all.
    status: Option<u16>,
    message: String,
}

impl OAuthError {
    fn answered(status: StatusCode, detail: &str) -> Self {
        Self {
            status: Some(status.as_u16()),
            message: format!("Anthropic answered {status}: {detail}"),
        }
    }

    fn unreachable(error: &reqwest::Error) -> Self {
        Self {
            status: None,
            message: format!("Anthropic could not be reached: {error}"),
        }
    }

    fn malformed(what: &str) -> Self {
        Self {
            status: None,
            message: format!("Anthropic's answer had no {what}"),
        }
    }

    /// Whether Anthropic refused the token itself (expired, rotated away, or revoked), as
    /// opposed to failing for a reason a later retry could get past.
    pub fn is_token_rejected(&self) -> bool {
        matches!(self.status, Some(400 | 401))
    }
}

/// Anthropic's OAuth endpoints. Cheap to clone; clones share one HTTP client.
#[derive(Debug, Clone)]
pub struct Anthropic {
    http: reqwest::Client,
    api_origin: String,
    token_origin: String,
}

impl Anthropic {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            api_origin: API_ORIGIN.to_owned(),
            token_origin: TOKEN_ORIGIN.to_owned(),
        }
    }

    /// A client that sends every call to `origin`, such as a local fake.
    #[cfg(test)]
    pub fn with_test_origin(http: reqwest::Client, origin: &str) -> Self {
        Self {
            http,
            api_origin: origin.to_owned(),
            token_origin: origin.to_owned(),
        }
    }

    /// Trades `refresh_token` for a new pair. Claude Code's tokens rotate: the answer carries
    /// a new refresh token, and the old one stops working.
    ///
    /// # Errors
    /// [`OAuthError`], with [`OAuthError::is_token_rejected`] true when the refresh token is
    /// no longer accepted.
    pub async fn refresh(
        &self,
        refresh_token: &SecretString,
        now: DateTime<Utc>,
        previous_refresh_expiry: Option<DateTime<Utc>>,
    ) -> Result<Tokens, OAuthError> {
        let response = self
            .http
            .post(format!("{}{TOKEN_PATH}", self.token_origin))
            .timeout(REQUEST_TIMEOUT)
            .json(&json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token.expose_secret(),
                "client_id": CLAUDE_CODE_CLIENT_ID,
            }))
            .send()
            .await
            .map_err(|error| OAuthError::unreachable(&error))?;
        let body = read(response).await?;
        let answer: TokenAnswer =
            serde_json::from_value(body).map_err(|_shape| OAuthError::malformed("access token"))?;
        Ok(answer.into_tokens(refresh_token, now, previous_refresh_expiry))
    }

    /// Whose account `access_token` belongs to.
    ///
    /// # Errors
    /// [`OAuthError`] when Anthropic refuses the token or answers without an account id.
    pub async fn profile(&self, access_token: &SecretString) -> Result<Identity, OAuthError> {
        let body = self.get(PROFILE_PATH, access_token).await?;
        let profile: ProfileAnswer =
            serde_json::from_value(body).map_err(|_shape| OAuthError::malformed("account id"))?;
        Ok(Identity {
            account_uuid: profile.account.uuid,
            email: profile.account.email.unwrap_or_default(),
            organization_uuid: profile
                .organization
                .as_ref()
                .and_then(|organization| organization.uuid),
            organization_name: profile
                .organization
                .and_then(|organization| organization.name),
        })
    }

    /// The usage windows of the account behind `access_token`.
    ///
    /// # Errors
    /// [`OAuthError`] when Anthropic refuses the token or cannot be reached.
    pub async fn usage(&self, access_token: &SecretString) -> Result<UsageWindows, OAuthError> {
        let body = self.get(USAGE_PATH, access_token).await?;
        Ok(usage::parse(&body))
    }

    async fn get(&self, path: &str, access_token: &SecretString) -> Result<Value, OAuthError> {
        let response = self
            .http
            .get(format!("{}{path}", self.api_origin))
            .timeout(REQUEST_TIMEOUT)
            .bearer_auth(access_token.expose_secret())
            .header("anthropic-beta", OAUTH_BETA)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|error| OAuthError::unreachable(&error))?;
        read(response).await
    }
}

/// The JSON object a successful call answers with.
async fn read(response: reqwest::Response) -> Result<Value, OAuthError> {
    let status = response.status();
    if !status.is_success() {
        // Anthropic's error bodies name the problem (`invalid_grant`, ...) and carry no secret.
        let detail = response
            .json::<Value>()
            .await
            .ok()
            .and_then(|body| {
                body.get("error_description")
                    .or_else(|| body.get("error"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "no detail".to_owned());
        return Err(OAuthError::answered(status, &detail));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|_decode| OAuthError::malformed("JSON body"))?;
    if !body.is_object() {
        return Err(OAuthError::malformed("JSON object"));
    }
    Ok(body)
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
    refresh_token_expires_in: Option<i64>,
}

impl TokenAnswer {
    /// The new pair. A rotated refresh token without a stated lifetime does not inherit the
    /// old one's expiry; only an unrotated one keeps it.
    fn into_tokens(
        self,
        previous_refresh_token: &SecretString,
        now: DateTime<Utc>,
        previous_refresh_expiry: Option<DateTime<Utc>>,
    ) -> Tokens {
        let refresh_token_expires_at = match (&self.refresh_token, self.refresh_token_expires_in) {
            (_, Some(seconds)) => Some(now + TimeDelta::seconds(seconds)),
            (Some(_rotated), None) => None,
            (None, None) => previous_refresh_expiry,
        };
        Tokens {
            access_token: SecretString::from(self.access_token),
            access_token_expires_at: now + TimeDelta::seconds(self.expires_in),
            refresh_token: self.refresh_token.map_or_else(
                || SecretString::from(previous_refresh_token.expose_secret().to_owned()),
                SecretString::from,
            ),
            refresh_token_expires_at,
        }
    }
}

#[derive(Deserialize)]
struct ProfileAnswer {
    account: ProfileAccount,
    organization: Option<ProfileOrganization>,
}

#[derive(Deserialize)]
struct ProfileAccount {
    uuid: Uuid,
    email: Option<String>,
}

#[derive(Deserialize)]
struct ProfileOrganization {
    uuid: Option<Uuid>,
    name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::super::fake::FakeAnthropic;
    use super::*;

    #[tokio::test]
    async fn a_refresh_sends_claude_codes_client_and_takes_the_rotated_pair() {
        let fake = FakeAnthropic::start().await;
        let now = Utc::now();

        let tokens = fake
            .client()
            .refresh(&SecretString::from(FakeAnthropic::REFRESH_TOKEN), now, None)
            .await
            .expect("refreshed");

        assert_eq!(tokens.access_token.expose_secret(), "access-rotated-1");
        assert_eq!(tokens.refresh_token.expose_secret(), "refresh-rotated-1");
        assert_eq!(
            tokens.access_token_expires_at.timestamp(),
            (now + TimeDelta::hours(8)).timestamp()
        );
        let request = fake.last_token_request().expect("a token request");
        assert_eq!(request["grant_type"], "refresh_token");
        assert_eq!(request["client_id"], CLAUDE_CODE_CLIENT_ID);
        assert_eq!(request["refresh_token"], FakeAnthropic::REFRESH_TOKEN);
    }

    #[tokio::test]
    async fn a_spent_refresh_token_is_reported_as_rejected() {
        let fake = FakeAnthropic::start().await;

        let error = fake
            .client()
            .refresh(&SecretString::from("rotated-away"), Utc::now(), None)
            .await
            .expect_err("refused");

        assert!(error.is_token_rejected());
        assert!(
            error
                .to_string()
                .contains("Refresh token not found or invalid")
        );
    }

    #[tokio::test]
    async fn an_unreachable_anthropic_is_not_a_rejected_token() {
        // Nothing listens on port 9 of loopback.
        let client = Anthropic::with_test_origin(reqwest::Client::new(), "http://127.0.0.1:9");

        let error = client
            .refresh(&SecretString::from("anything"), Utc::now(), None)
            .await
            .expect_err("unreachable");

        assert!(!error.is_token_rejected());
    }

    #[tokio::test]
    async fn the_profile_names_the_account_and_its_organization() {
        let fake = FakeAnthropic::start().await;

        let identity = fake
            .client()
            .profile(&SecretString::from(FakeAnthropic::ACCESS_TOKEN))
            .await
            .expect("profile");

        assert_eq!(identity.account_uuid, FakeAnthropic::ACCOUNT_UUID);
        assert_eq!(identity.email, "alex@example.com");
        assert_eq!(identity.organization_name.as_deref(), Some("Jalapeno Labs"));
    }

    #[test]
    fn an_unrotated_refresh_token_keeps_its_expiry_and_a_rotated_one_does_not_inherit_it() {
        let now = Utc::now();
        let previous = SecretString::from("old-refresh");
        let previous_expiry = Some(now + TimeDelta::days(20));

        let unrotated = TokenAnswer {
            access_token: "a".to_owned(),
            expires_in: 60,
            refresh_token: None,
            refresh_token_expires_in: None,
        }
        .into_tokens(&previous, now, previous_expiry);
        assert_eq!(unrotated.refresh_token.expose_secret(), "old-refresh");
        assert_eq!(unrotated.refresh_token_expires_at, previous_expiry);

        let rotated = TokenAnswer {
            access_token: "a".to_owned(),
            expires_in: 60,
            refresh_token: Some("new-refresh".to_owned()),
            refresh_token_expires_in: None,
        }
        .into_tokens(&previous, now, previous_expiry);
        assert_eq!(rotated.refresh_token.expose_secret(), "new-refresh");
        assert_eq!(rotated.refresh_token_expires_at, None);

        let stated = TokenAnswer {
            access_token: "a".to_owned(),
            expires_in: 60,
            refresh_token: Some("new-refresh".to_owned()),
            refresh_token_expires_in: Some(3_600),
        }
        .into_tokens(&previous, now, previous_expiry);
        assert_eq!(
            stated.refresh_token_expires_at,
            Some(now + TimeDelta::hours(1))
        );
    }
}
