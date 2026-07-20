//! The two public discovery documents: OpenID configuration and JWKS.
//!
//! An OIDC client bootstraps itself from `/.well-known/openid-configuration`
//! (where the endpoints live, what the provider supports) and verifies token
//! signatures against `/.well-known/jwks.json` (the public signing keys). Both are
//! anonymous: a client reads them before any user is involved. The issuer is the
//! deployment's app origin, so every URL in the document is same-origin.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::oidc::keys;
use crate::oidc::{SUPPORTED_GRANT_TYPES, SUPPORTED_SCOPES};
use crate::state::AppState;

/// `GET /.well-known/openid-configuration`: the provider metadata document.
///
/// The `issuer` is the app origin (matching the `iss` claim in every token), and
/// the endpoint URLs are derived from it. The `*_supported` arrays advertise
/// exactly what this provider implements: Authorization Code + refresh grants,
/// the `code` response type, S256 PKCE, RS256 signing, and public subjects.
pub async fn configuration(State(state): State<AppState>) -> Response {
    let issuer = &state.app_origin;
    let document = json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/api/v1/oidc/authorize"),
        "token_endpoint": format!("{issuer}/api/v1/oidc/token"),
        "userinfo_endpoint": format!("{issuer}/api/v1/oidc/userinfo"),
        "jwks_uri": format!("{issuer}/.well-known/jwks.json"),
        "scopes_supported": SUPPORTED_SCOPES,
        "response_types_supported": ["code"],
        "grant_types_supported": SUPPORTED_GRANT_TYPES,
        "code_challenge_methods_supported": ["S256"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "subject_types_supported": ["public"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post", "none"],
        "claims_supported": [
            "sub", "iss", "aud", "iat", "exp", "nonce",
            "email", "email_verified", "name", "given_name", "family_name",
            "org", "roles", "groups"
        ],
    });
    Json(document).into_response()
}

/// `GET /.well-known/jwks.json`: the public signing keys, for token verification.
///
/// Publishes every non-retired key so a token signed just before a rotation still
/// verifies. Answers `503` if the keys cannot be loaded, never a partial set.
pub async fn jwks(State(state): State<AppState>) -> Response {
    match keys::jwks(&state.pool).await {
        Ok(document) => Json(document).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "failed to build the JWKS document");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}
