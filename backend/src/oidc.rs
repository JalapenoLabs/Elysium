//! OIDC Backend-for-Frontend. The browser never talks to Keycloak; the API does.
//!
//! Elysium's API is a confidential OIDC client. `login` sends the browser to
//! Keycloak's Authorization Code + PKCE flow; `callback` exchanges the code for
//! tokens over a backchannel, validates the ID token, creates a server-side
//! [`crate::session`], and hands the browser only an HttpOnly session cookie. No
//! token is ever readable by JavaScript.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::Engine;
use eyre::WrapErr;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::config::Deployment;
use crate::session::{self, NewSession, SESSION_COOKIE};
use crate::state::AppState;

/// Short-lived cookie holding the in-flight login parameters (state:nonce:verifier).
const OAUTH_COOKIE: &str = "elysium_oauth";

/// Fixed client id for the confidential BFF client; matches the realm import.
const OIDC_CLIENT_ID: &str = "elysium-api";

/// Configuration for the confidential OIDC client, resolved from the environment.
///
/// Browser-facing URLs (authorize, account) use the public issuer so redirects
/// land on Elysium's own origin; the backchannel URLs (token, logout) use the
/// internal issuer reachable from inside the Docker network.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub authorize_url: String,
    pub token_url: String,
    pub logout_url: String,
    pub account_url: String,
    pub post_login_url: String,
}

impl OidcConfig {
    /// Builds the OIDC config from the derived deployment plus the client secret.
    ///
    /// The client id is fixed ([`OIDC_CLIENT_ID`]) and every URL comes from
    /// [`Deployment`], so the only OIDC-specific value the environment supplies is
    /// the secret.
    pub fn new(deployment: &Deployment, client_secret: String) -> Self {
        Self {
            client_id: OIDC_CLIENT_ID.to_owned(),
            client_secret,
            redirect_uri: deployment.redirect_uri.clone(),
            authorize_url: deployment.authorize_url.clone(),
            token_url: deployment.token_url.clone(),
            logout_url: deployment.logout_url.clone(),
            account_url: deployment.account_url.clone(),
            post_login_url: deployment.app_origin.clone(),
        }
    }
}

/// `GET /auth/login`: begin the login by redirecting the browser to Keycloak.
pub async fn login(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let config = &state.oidc;

    let csrf_state = Uuid::new_v4().to_string();
    let nonce = Uuid::new_v4().to_string();
    // PKCE verifier: two hyphen-free UUIDs give ~64 unreserved chars (43..=128).
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = pkce_challenge(&verifier);

    // Stash state/nonce/verifier for the callback in an HttpOnly cookie.
    let oauth_cookie = Cookie::build((OAUTH_COOKIE, format!("{csrf_state}:{nonce}:{verifier}")))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .build();

    let mut authorize = reqwest::Url::parse(&config.authorize_url)
        .expect("authorize_url is built from a valid issuer");
    authorize
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", &config.redirect_uri)
        .append_pair("scope", "openid")
        .append_pair("state", &csrf_state)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");

    (jar.add(oauth_cookie), Redirect::to(authorize.as_str()))
}

/// Query parameters Keycloak appends to the callback redirect.
#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// `GET /auth/callback`: exchange the code, establish the session, set the cookie.
pub async fn callback(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<CallbackParams>,
) -> Response {
    let config = &state.oidc;
    // Always clear the in-flight login cookie on the way out.
    let cleared = jar.clone().remove(Cookie::from(OAUTH_COOKIE));

    if let Some(error) = params.error {
        tracing::debug!(error = %error, "OIDC callback returned an error");
        return (cleared, Redirect::to(&config.post_login_url)).into_response();
    }

    let Some(oauth) = jar.get(OAUTH_COOKIE).map(|entry| entry.value().to_owned()) else {
        return (StatusCode::BAD_REQUEST, "missing login state").into_response();
    };
    let Some((expected_state, nonce, verifier)) = split_login_state(&oauth) else {
        return (StatusCode::BAD_REQUEST, "malformed login state").into_response();
    };
    let (Some(code), Some(returned_state)) = (params.code, params.state) else {
        return (StatusCode::BAD_REQUEST, "missing code or state").into_response();
    };
    if returned_state != expected_state {
        return (StatusCode::BAD_REQUEST, "state mismatch").into_response();
    }

    let tokens = match exchange_code(&state, &code, verifier).await {
        Ok(tokens) => tokens,
        Err(error) => {
            tracing::error!(error = %error, "token exchange failed");
            return (StatusCode::BAD_GATEWAY, "token exchange failed").into_response();
        }
    };

    // Validate the ID token (signature + issuer via the JWKS path) and the nonce.
    let claims = match state.auth.authenticate(&tokens.id_token).await {
        Ok(claims) => claims,
        Err(_) => return (StatusCode::BAD_GATEWAY, "invalid id token").into_response(),
    };
    if claims.nonce != nonce {
        return (StatusCode::BAD_REQUEST, "nonce mismatch").into_response();
    }

    let session_id = match session::create(
        &state.pool,
        NewSession {
            sub: claims.sub,
            email: claims.email,
            username: claims.preferred_username,
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token.unwrap_or_default(),
            id_token: tokens.id_token,
            expires_in: tokens.expires_in.unwrap_or(300),
        },
    )
    .await
    {
        Ok(id) => id,
        Err(error) => {
            tracing::error!(error = %error, "failed to create session");
            return (StatusCode::INTERNAL_SERVER_ERROR, "session error").into_response();
        }
    };

    let session_cookie = Cookie::build((SESSION_COOKIE, session_id.to_string()))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::None)
        .path("/")
        .build();

    (
        cleared.add(session_cookie),
        Redirect::to(&config.post_login_url),
    )
        .into_response()
}

/// `POST /api/v1/auth/logout`: end the session locally and at Keycloak.
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(id) = jar
        .get(SESSION_COOKIE)
        .and_then(|entry| Uuid::parse_str(entry.value()).ok())
    {
        match session::delete(&state.pool, id).await {
            Ok(Some((_id_token, refresh_token))) => {
                // Best-effort RP-initiated logout so the Keycloak SSO session ends
                // too, not just the local one. A failure here is non-fatal.
                let _ = state
                    .http
                    .post(&state.oidc.logout_url)
                    .form(&[
                        ("client_id", state.oidc.client_id.as_str()),
                        ("client_secret", state.oidc.client_secret.as_str()),
                        ("refresh_token", refresh_token.as_str()),
                    ])
                    .send()
                    .await;
            }
            Ok(None) => {}
            Err(error) => tracing::error!(error = %error, "failed to delete session"),
        }
    }

    (
        jar.remove(Cookie::from(SESSION_COOKIE)),
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

/// `GET /auth/account`: redirect to Keycloak's themed account console.
pub async fn account(State(state): State<AppState>) -> Redirect {
    Redirect::to(&state.oidc.account_url)
}

/// Token endpoint response (only the fields we consume).
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    id_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
}

/// Exchanges an authorization code for tokens over the confidential backchannel.
async fn exchange_code(
    state: &AppState,
    code: &str,
    verifier: &str,
) -> eyre::Result<TokenResponse> {
    let config = &state.oidc;
    let tokens = state
        .http
        .post(&config.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", config.redirect_uri.as_str()),
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .wrap_err("token request failed")?
        .error_for_status()
        .wrap_err("token endpoint returned an error status")?
        .json::<TokenResponse>()
        .await
        .wrap_err("token response was not valid JSON")?;

    Ok(tokens)
}

/// Computes the PKCE S256 code challenge from a verifier.
fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Splits the `state:nonce:verifier` login-state cookie. None of the three parts
/// contain a colon (UUIDs and hex), so the delimiter is unambiguous.
fn split_login_state(value: &str) -> Option<(&str, &str, &str)> {
    let mut parts = value.splitn(3, ':');
    Some((parts.next()?, parts.next()?, parts.next()?))
}
