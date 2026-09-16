// Copyright © 2026 Jalapeno Labs

//! GitHub's REST API, reached through one [`Github`] handle.
//!
//! Elysium authenticates with a personal access token, classic or fine-grained: both are
//! sent as a bearer token, and the API tells them apart only by what it answers with. A
//! classic token's scopes come back in `X-OAuth-Scopes`; a fine-grained token's
//! permissions are per repository and are not reported, so that header is absent.
//!
//! The host is fixed here, `api.github.com`, so a stored credential carries no endpoint.
//! GitHub Enterprise Server, which lives on a customer's own host, is not supported.
//!
//! GitHub refuses any request without a `User-Agent`; the shared client sets Elysium's.

use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, Utc};
use reqwest::StatusCode;
use reqwest::header::ACCEPT;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tracing::{Level, event};

/// The authenticated user, the one call that proves a token works and says whose it is.
const USER_URL: &str = "https://api.github.com/user";

/// The REST API version these calls are written against. GitHub keeps older versions
/// working, so pinning one means a new default never changes an answer under Elysium.
const API_VERSION: &str = "2022-11-28";

/// How long one call may take. `GET /user` answers in well under a second.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// GitHub refused a call or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    /// The token is wrong, expired, or revoked. The message says what to check.
    #[error("{0}")]
    Unauthorized(&'static str),
    #[error("{0}")]
    Refused(String),
}

/// Who a token belongs to and what it may do, as GitHub reports it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// The account the token acts as, such as `octocat`.
    pub login: String,
    /// A classic token's scopes. Empty for a fine-grained token, whose permissions are
    /// granted per repository and are not reported over the API.
    pub scopes: Vec<String>,
    /// When GitHub stops accepting the token, or `None` for one that does not expire.
    pub token_expires_at: Option<DateTime<Utc>>,
}

/// GitHub's error body, such as `{"message":"Bad credentials"}`.
#[derive(Deserialize)]
struct ErrorBody {
    message: String,
}

/// The parts of `GET /user` Elysium keeps.
#[derive(Deserialize)]
struct UserBody {
    login: String,
}

/// The scopes in an `X-OAuth-Scopes` header, such as `repo, read:org`. An empty header
/// means a classic token with no scopes at all.
fn parse_scopes(header: &str) -> Vec<String> {
    header
        .split(',')
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The instant in a `github-authentication-token-expiration` header.
///
/// GitHub writes it as `2026-12-31 23:59:59 UTC`, and as an offset such as
/// `2026-12-31 23:59:59 +0100` for a token created under one. A header in neither shape
/// reads as no expiry: when the token expires is worth showing, but not worth refusing a
/// working token over.
fn parse_expiration(header: &str) -> Option<DateTime<Utc>> {
    let value = header.trim();
    if value.is_empty() {
        return None;
    }

    if let Ok(offset) = DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S %z") {
        return Some(offset.with_timezone(&Utc));
    }
    let utc = value.strip_suffix(" UTC").unwrap_or(value);
    if let Ok(naive) = NaiveDateTime::parse_from_str(utc, "%Y-%m-%d %H:%M:%S") {
        return Some(naive.and_utc());
    }

    event!(
        name: "github.expiration.unreadable",
        Level::DEBUG,
        github.token.expiration = value,
        "GitHub sent a token expiration in an unknown format",
    );
    None
}

/// GitHub's API client. Cheap to clone; clones share one HTTP client.
#[derive(Debug, Clone)]
pub struct Github {
    http: reqwest::Client,
}

impl Github {
    pub const fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Checks a token with GitHub and reports whose it is, what it may do, and when it
    /// expires.
    ///
    /// # Errors
    /// Returns [`GithubError::Unauthorized`] when GitHub rejects the token, and
    /// [`GithubError::Refused`] when it cannot be reached or refuses the call, such as
    /// for a token an organization has blocked.
    pub async fn verify(&self, token: &SecretString) -> Result<Account, GithubError> {
        let response = self
            .http
            .get(USER_URL)
            .bearer_auth(token.expose_secret())
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| GithubError::Refused(format!("GitHub: {error}")))?;

        let status = response.status();
        // Headers are read before the body, which consumes the response.
        let scopes = response
            .headers()
            .get("x-oauth-scopes")
            .and_then(|value| value.to_str().ok())
            .map(parse_scopes)
            .unwrap_or_default();
        let token_expires_at = response
            .headers()
            .get("github-authentication-token-expiration")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_expiration);

        let body = response
            .text()
            .await
            .map_err(|error| GithubError::Refused(format!("GitHub: {error}")))?;

        if status == StatusCode::UNAUTHORIZED {
            return Err(GithubError::Unauthorized(
                "GitHub refused the token; check that it was copied whole and has not expired or been revoked",
            ));
        }
        if !status.is_success() {
            let message = serde_json::from_str::<ErrorBody>(&body)
                .map_or_else(|_error| status.to_string(), |error| error.message);
            return Err(GithubError::Refused(format!("GitHub: {message}")));
        }

        let user: UserBody = serde_json::from_str(&body).map_err(|error| {
            GithubError::Refused(format!("GitHub sent an unreadable account: {error}"))
        })?;

        Ok(Account {
            login: user.login,
            scopes,
            token_expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn expirations_parse_in_utc_and_under_an_offset() {
        let midnight = Utc.with_ymd_and_hms(2026, 12, 31, 23, 59, 59).unwrap();

        assert_eq!(parse_expiration("2026-12-31 23:59:59 UTC"), Some(midnight));
        assert_eq!(parse_expiration("2026-12-31T23:59:59"), None);
        assert_eq!(
            parse_expiration("2027-01-01 00:59:59 +0100"),
            Some(midnight)
        );
        assert_eq!(parse_expiration(""), None);
        assert_eq!(parse_expiration("never"), None);
    }

    #[test]
    fn scopes_are_split_and_an_empty_header_means_none() {
        assert_eq!(
            parse_scopes("repo, read:org,workflow"),
            vec!["repo", "read:org", "workflow"]
        );
        assert!(parse_scopes("").is_empty());
        assert!(parse_scopes(" , ").is_empty());
    }
}
