// Copyright © 2026 Jalapeno Labs

//! The mail providers the broker holds OAuth apps for, and the two token calls it
//! makes to them.
//!
//! Everything provider-specific is data in a [`ProviderSpec`]: endpoints, scopes,
//! and the extra authorize parameters each one needs to issue a refresh token. Adding
//! a provider is a new variant and a new spec, not new branches in the routes.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tracing::{Level, event};

/// A provider the broker can connect accounts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Google,
    Microsoft,
}

/// Every provider, for listing what a deployment has enabled.
pub const ALL_PROVIDERS: [Provider; 2] = [Provider::Google, Provider::Microsoft];

/// How to talk OAuth to one provider.
#[derive(Debug)]
pub struct ProviderSpec {
    /// The provider's name in URLs and JSON, matching its serde form.
    pub slug: &'static str,
    /// Shown on the consent interstitial.
    pub display_name: &'static str,
    pub authorize_url: &'static str,
    pub token_url: &'static str,
    /// `openid email` names the account; the rest grant IMAP and SMTP access.
    pub scopes: &'static str,
    /// Parameters beyond the standard code flow.
    pub extra_authorize_params: &'static [(&'static str, &'static str)],
    /// Microsoft's v2 endpoint expects the scopes again when refreshing; Google
    /// treats them as a request to narrow the grant, so it gets none.
    pub refresh_repeats_scopes: bool,
}

const GOOGLE: ProviderSpec = ProviderSpec {
    slug: "google",
    display_name: "Google",
    authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    // `https://mail.google.com/` is the only scope Gmail's IMAP and SMTP accept. It is
    // a restricted scope: see the broker README for what that means for verification.
    scopes: "openid email https://mail.google.com/",
    // `access_type=offline` asks for a refresh token; `prompt=consent` makes Google
    // issue one again for an account that consented before.
    extra_authorize_params: &[("access_type", "offline"), ("prompt", "consent")],
    refresh_repeats_scopes: false,
};

const MICROSOFT: ProviderSpec = ProviderSpec {
    slug: "microsoft",
    display_name: "Microsoft",
    // `common` admits work, school, and personal Outlook.com accounts alike.
    authorize_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
    token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
    // `offline_access` asks for a refresh token.
    scopes: concat!(
        "openid email offline_access ",
        "https://outlook.office.com/IMAP.AccessAsUser.All ",
        "https://outlook.office.com/SMTP.Send",
    ),
    extra_authorize_params: &[("prompt", "select_account")],
    refresh_repeats_scopes: true,
};

impl Provider {
    pub const fn spec(self) -> &'static ProviderSpec {
        match self {
            Self::Google => &GOOGLE,
            Self::Microsoft => &MICROSOFT,
        }
    }
}

/// The OAuth app credentials registered with one provider.
#[derive(Debug, Clone)]
pub struct AppCredentials {
    pub client_id: String,
    pub client_secret: SecretString,
}

/// How long the broker waits on a provider's token endpoint.
///
/// A browser is waiting on the callback, so this stays well under the timeouts of
/// the proxies in front of it.
pub const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// A provider refused a token request, or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    /// The code or refresh token is invalid, expired, or revoked. The account has
    /// to be connected again.
    #[error("{0}")]
    InvalidGrant(String),
    /// Any other refusal, or a transport failure.
    #[error("{0}")]
    Upstream(String),
}

/// A provider's token endpoint response, reduced to what the broker uses.
#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: SecretString,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<SecretString>,
    #[serde(default)]
    pub id_token: Option<String>,
}

impl std::fmt::Debug for TokenResponse {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TokenResponse")
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

/// The RFC 6749 error body.
#[derive(Deserialize)]
struct TokenErrorBody {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Exchanges an authorization code for tokens.
///
/// # Errors
/// Returns [`TokenError`] when the provider refuses the code or cannot be reached.
pub async fn exchange_code(
    http: &reqwest::Client,
    provider: Provider,
    credentials: &AppCredentials,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse, TokenError> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", credentials.client_id.as_str()),
        ("client_secret", credentials.client_secret.expose_secret()),
        ("code_verifier", code_verifier),
    ];
    request_tokens(http, provider, &form).await
}

/// Trades a refresh token for a fresh access token.
///
/// # Errors
/// Returns [`TokenError::InvalidGrant`] when the refresh token no longer works, and
/// [`TokenError::Upstream`] for any other failure.
pub async fn refresh(
    http: &reqwest::Client,
    provider: Provider,
    credentials: &AppCredentials,
    refresh_token: &SecretString,
) -> Result<TokenResponse, TokenError> {
    let spec = provider.spec();
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.expose_secret()),
        ("client_id", credentials.client_id.as_str()),
        ("client_secret", credentials.client_secret.expose_secret()),
    ];
    if spec.refresh_repeats_scopes {
        form.push(("scope", spec.scopes));
    }
    request_tokens(http, provider, &form).await
}

async fn request_tokens(
    http: &reqwest::Client,
    provider: Provider,
    form: &[(&str, &str)],
) -> Result<TokenResponse, TokenError> {
    let response = http
        .post(provider.spec().token_url)
        .form(form)
        .timeout(TOKEN_REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|error| TokenError::Upstream(format!("token endpoint unreachable: {error}")))?;

    if response.status().is_success() {
        return response
            .json::<TokenResponse>()
            .await
            .map_err(|error| TokenError::Upstream(format!("token response unreadable: {error}")));
    }

    let status = response.status();
    let Ok(body) = response.json::<TokenErrorBody>().await else {
        return Err(TokenError::Upstream(format!(
            "token endpoint answered {status}"
        )));
    };

    event!(
        name: "provider.token.refused",
        Level::WARN,
        provider = ?provider,
        http.response.status_code = status.as_u16(),
        error.type = %body.error,
        "a provider refused a token request",
    );

    let message = body.error_description.unwrap_or_else(|| body.error.clone());
    if body.error == "invalid_grant" {
        return Err(TokenError::InvalidGrant(message));
    }
    Err(TokenError::Upstream(message))
}

/// The claims of an ID token that name the account.
#[derive(Deserialize)]
struct AddressClaims {
    #[serde(default)]
    email: Option<String>,
    /// Microsoft personal accounts often omit `email` but always carry this.
    #[serde(default)]
    preferred_username: Option<String>,
}

/// Reads the mailbox address out of an ID token.
///
/// The signature is deliberately not verified. The token came straight from the
/// provider's token endpoint over TLS in response to the broker's own request, which
/// `OpenID` Connect Core §3.1.3.7 accepts in place of signature validation.
pub fn address_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let decoded = BASE64_URL.decode(payload.trim_end_matches('=')).ok()?;
    let claims: AddressClaims = serde_json::from_slice(&decoded).ok()?;

    claims
        .email
        .or(claims.preferred_username)
        .filter(|address| address.contains('@'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn id_token_with(claims: &serde_json::Value) -> String {
        let payload = BASE64_URL.encode(claims.to_string());
        format!("eyJhbGciOiJSUzI1NiJ9.{payload}.signature")
    }

    #[test]
    fn the_address_comes_from_email_first() {
        let token = id_token_with(
            &json!({ "email": "a@gmail.com", "preferred_username": "b@outlook.com" }),
        );
        assert_eq!(
            address_from_id_token(&token).as_deref(),
            Some("a@gmail.com")
        );
    }

    #[test]
    fn microsoft_personal_accounts_fall_back_to_preferred_username() {
        let token = id_token_with(&json!({ "preferred_username": "someone@outlook.com" }));
        assert_eq!(
            address_from_id_token(&token).as_deref(),
            Some("someone@outlook.com")
        );
    }

    #[test]
    fn a_token_without_an_address_names_nobody() {
        let no_claims = id_token_with(&json!({ "sub": "123" }));
        let not_an_address = id_token_with(&json!({ "preferred_username": "someone" }));
        for token in [no_claims.as_str(), not_an_address.as_str(), "garbage", ""] {
            assert_eq!(address_from_id_token(token), None, "{token:?}");
        }
    }

    #[test]
    fn debug_output_never_reveals_tokens() {
        let response: TokenResponse = serde_json::from_value(json!({
            "access_token": "ya29.secret-access",
            "refresh_token": "1//secret-refresh",
            "expires_in": 3599,
        }))
        .expect("parses");
        let rendered = format!("{response:?}");
        assert!(!rendered.contains("secret-access"));
        assert!(!rendered.contains("secret-refresh"));
    }
}
