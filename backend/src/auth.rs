//! Keycloak token validation.
//!
//! Elysium does not issue its own tokens. It validates Keycloak-issued JWTs: the
//! signature against the realm's rotating JWKS, the issuer against the configured
//! realm URL, and the expiry. The BFF flow ([`crate::oidc`]) uses this to validate
//! the ID token it receives at the callback; browser requests then authenticate
//! via the session cookie ([`crate::session::SessionUser`]).
//!
//! The JWKS is fetched lazily and cached, so the API can start before Keycloak is
//! reachable and recovers on key rotation by refetching when a token references an
//! unknown key id.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use eyre::WrapErr;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::RwLock;

/// How long to wait on the JWKS endpoint before giving up.
const JWKS_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Claims read from a validated Keycloak access token.
///
/// Only the fields Elysium acts on are listed; unknown claims are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct Claims {
    /// Subject: the stable, unique user identifier (`sub`).
    pub sub: String,
    /// Email address; empty for tokens that carry none (e.g. service accounts).
    #[serde(default)]
    pub email: String,
    /// Keycloak's `preferred_username`, if present.
    #[serde(default)]
    pub preferred_username: String,
    /// OIDC `nonce`, present on ID tokens; used to bind the login callback.
    #[serde(default)]
    pub nonce: String,
}

/// Where to fetch keys and which issuer to trust, sourced from the environment.
#[derive(Debug, Clone)]
struct AuthConfig {
    /// Issuer (`iss`) that tokens must declare, e.g. the browser-facing realm URL.
    issuer: String,
    /// URL the API fetches the JWKS from; may differ from `issuer` in-cluster.
    jwks_url: String,
}

/// Validates bearer tokens against Keycloak's JWKS. Cheaply cloneable.
#[derive(Clone)]
pub struct Auth {
    config: AuthConfig,
    http: reqwest::Client,
    keys: Arc<RwLock<HashMap<String, DecodingKey>>>,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the decoding keys; the issuer is enough to identify config.
        formatter
            .debug_struct("Auth")
            .field("issuer", &self.config.issuer)
            .finish_non_exhaustive()
    }
}

/// Minimal JWKS shape: the RSA keys Keycloak signs tokens with.
#[derive(Debug, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

/// A single JSON Web Key. Non-RSA keys are ignored, so `n`/`e` may be absent.
#[derive(Debug, Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    #[serde(default)]
    n: String,
    #[serde(default)]
    e: String,
}

impl Auth {
    /// Builds token validation for the given issuer, fetching keys from `jwks_url`.
    ///
    /// # Errors
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(issuer: String, jwks_url: String) -> eyre::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(JWKS_FETCH_TIMEOUT)
            .build()
            .wrap_err("failed to build the JWKS HTTP client")?;

        Ok(Self {
            config: AuthConfig { issuer, jwks_url },
            http,
            keys: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Validates a bearer token and returns its claims.
    ///
    /// # Errors
    /// Returns [`AuthError`] when the token is malformed, signed by an unknown
    /// key, or fails signature/issuer/expiry validation.
    pub async fn authenticate(&self, token: &str) -> Result<Claims, AuthError> {
        let header = decode_header(token).map_err(|_| AuthError::InvalidToken)?;
        let kid = header.kid.ok_or(AuthError::InvalidToken)?;

        let key = self.decoding_key(&kid).await.ok_or(AuthError::UnknownKey)?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.config.issuer]);
        validation.validate_exp = true;
        // Audience is not enforced yet; a resource-server audience mapper is a
        // planned hardening step. Signature, issuer, and expiry are checked.
        validation.validate_aud = false;

        let data =
            decode::<Claims>(token, &key, &validation).map_err(|_| AuthError::InvalidToken)?;
        Ok(data.claims)
    }

    /// Returns the decoding key for `kid`, refetching the JWKS once on a miss.
    async fn decoding_key(&self, kid: &str) -> Option<DecodingKey> {
        if let Some(key) = self.keys.read().await.get(kid).cloned() {
            return Some(key);
        }

        // Unknown key id: Keycloak may have rotated. Refetch and try once more.
        if let Err(error) = self.refresh_jwks().await {
            tracing::error!(error = %error, "failed to refresh JWKS");
            return None;
        }

        self.keys.read().await.get(kid).cloned()
    }

    /// Fetches the JWKS and replaces the cached keys with the RSA ones found.
    async fn refresh_jwks(&self) -> Result<(), AuthError> {
        let jwks: Jwks = self
            .http
            .get(&self.config.jwks_url)
            .send()
            .await
            .map_err(|_| AuthError::JwksUnavailable)?
            .json()
            .await
            .map_err(|_| AuthError::JwksUnavailable)?;

        let mut refreshed = HashMap::new();
        for key in jwks.keys {
            if key.kty != "RSA" {
                continue;
            }
            match DecodingKey::from_rsa_components(&key.n, &key.e) {
                Ok(decoding_key) => {
                    refreshed.insert(key.kid, decoding_key);
                }
                Err(error) => {
                    tracing::warn!(error = %error, kid = %key.kid, "skipping unusable JWKS key");
                }
            }
        }

        *self.keys.write().await = refreshed;
        Ok(())
    }
}

/// Reason a request failed authentication, mapped to an HTTP status on rejection.
#[derive(Debug)]
pub enum AuthError {
    /// The token was malformed or failed signature/issuer/expiry validation.
    InvalidToken,
    /// The token references a signing key not present in the JWKS.
    UnknownKey,
    /// The JWKS endpoint could not be reached to validate the token.
    JwksUnavailable,
}

impl AuthError {
    /// Human-readable reason, shared by the log line and the HTTP response body.
    fn reason(&self) -> &'static str {
        match self {
            AuthError::InvalidToken => "invalid token",
            AuthError::UnknownKey => "unknown signing key",
            AuthError::JwksUnavailable => "identity provider unavailable",
        }
    }

    /// Status returned to the client for this failure.
    fn status(&self) -> StatusCode {
        match self {
            AuthError::JwksUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::UNAUTHORIZED,
        }
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.reason())
    }
}

impl std::error::Error for AuthError {}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (self.status(), self.reason()).into_response()
    }
}
