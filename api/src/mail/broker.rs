// Copyright © 2026 Jalapeno Labs

//! Client for the OAuth broker Gmail and Outlook accounts connect through.
//!
//! Elysium holds no OAuth apps of its own. A broker (the community one, or one you
//! run from `oauth-broker/`) holds them and relays three things: the browser's
//! consent flow, the redemption of a handoff code for a refresh token, and every
//! refresh of an access token. See `docs/mail.md` for the protocol.
//!
//! Browsers reach the broker at its public URL. The API can be told to reach it
//! somewhere else, for a broker running beside it that browsers see through a
//! published port.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use url::Url;

use crate::models::mail_account::MailAccountKind;

/// How long the API waits on the broker, which in turn waits on a provider.
///
/// The broker allows a provider 15 seconds; this leaves room for that plus the hop.
const BROKER_TIMEOUT: Duration = Duration::from_secs(20);

/// A fresh 256-bit random value in unpadded base64url: a flow `state` or a PKCE verifier.
///
/// # Panics
/// Panics if the operating system cannot supply random bytes.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random number generator is available");
    BASE64_URL.encode(bytes)
}

/// The PKCE S256 challenge (RFC 7636) for `verifier`.
pub fn pkce_challenge(verifier: &SecretString) -> String {
    BASE64_URL.encode(Sha256::digest(verifier.expose_secret().as_bytes()))
}

/// The broker's name for the provider behind each OAuth account kind.
fn provider_slug(kind: MailAccountKind) -> Option<&'static str> {
    match kind {
        MailAccountKind::Gmail => Some("google"),
        MailAccountKind::Outlook => Some("microsoft"),
        MailAccountKind::SelfHosted => None,
    }
}

/// The broker refused, or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    /// The handoff code or refresh token no longer works. The account must be
    /// connected again.
    #[error("{0}")]
    InvalidGrant(String),
    #[error("{0}")]
    Unavailable(String),
    /// The kind has no OAuth provider, which is a caller bug.
    #[error("{0:?} accounts do not connect through the broker")]
    NotOAuth(MailAccountKind),
}

/// An account the broker handed over.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedeemedAccount {
    pub address: String,
    pub refresh_token: SecretString,
    provider: String,
}

/// A usable access token, and the refresh token to keep if the provider rotated it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessGrant {
    pub access_token: SecretString,
    pub refresh_token: Option<SecretString>,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    error: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Clone)]
pub struct Broker {
    http: reqwest::Client,
    public_url: Url,
    internal_url: Url,
}

impl Broker {
    pub const fn new(http: reqwest::Client, public_url: Url, internal_url: Url) -> Self {
        Self {
            http,
            public_url,
            internal_url,
        }
    }

    /// Where to send the browser to connect a `kind` account.
    ///
    /// # Errors
    /// Returns [`BrokerError::NotOAuth`] for a kind without a provider.
    pub fn authorize_url(
        &self,
        kind: MailAccountKind,
        redirect_uri: &str,
        state: &str,
        code_challenge: &str,
    ) -> Result<Url, BrokerError> {
        let provider = provider_slug(kind).ok_or(BrokerError::NotOAuth(kind))?;
        let mut url = self
            .public_url
            .join("v1/authorize")
            .expect("a static path joins");
        url.query_pairs_mut()
            .append_pair("provider", provider)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("state", state)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url)
    }

    /// Which account kinds this broker can connect, by asking it.
    ///
    /// # Errors
    /// Returns [`BrokerError::Unavailable`] when the broker does not answer.
    pub async fn available_kinds(&self) -> Result<Vec<MailAccountKind>, BrokerError> {
        #[derive(Deserialize)]
        struct Providers {
            providers: Vec<String>,
        }

        let response = self
            .http
            .get(
                self.internal_url
                    .join("v1/providers")
                    .expect("a static path joins"),
            )
            .timeout(BROKER_TIMEOUT)
            .send()
            .await
            .map_err(|error| BrokerError::Unavailable(error.to_string()))?;
        let providers: Providers = read_json(response).await?;

        Ok([MailAccountKind::Gmail, MailAccountKind::Outlook]
            .into_iter()
            .filter(|kind| {
                provider_slug(*kind)
                    .is_some_and(|slug| providers.providers.iter().any(|name| name == slug))
            })
            .collect())
    }

    /// Trades a handoff code for the account it carries.
    ///
    /// # Errors
    /// Returns [`BrokerError::InvalidGrant`] for an expired code, a verifier that does
    /// not match, or a code issued for another kind of account.
    pub async fn redeem(
        &self,
        kind: MailAccountKind,
        handoff: &str,
        code_verifier: &SecretString,
    ) -> Result<RedeemedAccount, BrokerError> {
        let expected_provider = provider_slug(kind).ok_or(BrokerError::NotOAuth(kind))?;
        let response = self
            .http
            .post(
                self.internal_url
                    .join("v1/redeem")
                    .expect("a static path joins"),
            )
            .json(&json!({ "handoff": handoff, "codeVerifier": code_verifier.expose_secret() }))
            .timeout(BROKER_TIMEOUT)
            .send()
            .await
            .map_err(|error| BrokerError::Unavailable(error.to_string()))?;
        let account: RedeemedAccount = read_json(response).await?;

        if account.provider != expected_provider {
            return Err(BrokerError::InvalidGrant(format!(
                "the broker returned a {} account for a {expected_provider} connection",
                account.provider
            )));
        }
        Ok(account)
    }

    /// Trades a refresh token for an access token.
    ///
    /// # Errors
    /// Returns [`BrokerError::InvalidGrant`] when the provider revoked or expired the
    /// refresh token, and [`BrokerError::Unavailable`] for anything else.
    pub async fn refresh(
        &self,
        kind: MailAccountKind,
        refresh_token: &SecretString,
    ) -> Result<AccessGrant, BrokerError> {
        let provider = provider_slug(kind).ok_or(BrokerError::NotOAuth(kind))?;
        let response = self
            .http
            .post(
                self.internal_url
                    .join("v1/refresh")
                    .expect("a static path joins"),
            )
            .json(&json!({ "provider": provider, "refreshToken": refresh_token.expose_secret() }))
            .timeout(BROKER_TIMEOUT)
            .send()
            .await
            .map_err(|error| BrokerError::Unavailable(error.to_string()))?;
        read_json(response).await
    }
}

async fn read_json<Body: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<Body, BrokerError> {
    let status = response.status();
    if status.is_success() {
        return response.json().await.map_err(|error| {
            BrokerError::Unavailable(format!("unreadable broker response: {error}"))
        });
    }

    let Ok(body) = response.json::<ErrorBody>().await else {
        return Err(BrokerError::Unavailable(format!(
            "the broker answered {status}"
        )));
    };
    if body.error == "invalid_grant" {
        return Err(BrokerError::InvalidGrant(body.message));
    }
    Err(BrokerError::Unavailable(format!(
        "{}: {}",
        body.error, body.message
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pkce_challenge_matches_rfc_7636_appendix_b() {
        let verifier = SecretString::from("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(
            pkce_challenge(&verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_authorize_url_carries_the_flow_parameters() {
        let broker = Broker::new(
            reqwest::Client::new(),
            "https://broker.example.com/".parse().expect("url"),
            "http://oauth-broker:8080/".parse().expect("url"),
        );

        let url = broker
            .authorize_url(
                MailAccountKind::Outlook,
                "http://localhost:4000/api/v1/mail/oauth/callback",
                "state-123",
                "challenge",
            )
            .expect("outlook connects through the broker");

        assert_eq!(
            url.origin().ascii_serialization(),
            "https://broker.example.com"
        );
        assert_eq!(url.path(), "/v1/authorize");
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert!(pairs.contains(&("provider".to_owned(), "microsoft".to_owned())));
        assert!(pairs.contains(&("code_challenge_method".to_owned(), "S256".to_owned())));

        assert!(matches!(
            broker.authorize_url(MailAccountKind::SelfHosted, "http://localhost", "s", "c"),
            Err(BrokerError::NotOAuth(MailAccountKind::SelfHosted))
        ));
    }
}
