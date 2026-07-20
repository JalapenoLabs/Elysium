//! Elysium as an OIDC/OAuth2 provider: the interactive SSO spine.
//!
//! Every managed service (Forgejo, Harbor, ...) SSOs against Elysium rather than a
//! bolted-on Keycloak, because Elysium already owns identity deeply (OPAQUE,
//! passkeys, TOTP, orgs, sessions). This module implements the standard,
//! spec-compliant **Authorization Code flow with PKCE**, plus discovery, JWKS,
//! token, and userinfo endpoints, all keyed off Elysium's own tables.
//!
//! The pieces, one submodule each:
//!
//! - [`keys`] mints, rotates, and publishes the RS256 signing keys (JWKS) and signs
//!   JWTs. [`clients`] is the registry of relying parties and their authentication.
//!   [`tokens`] builds claims, verifies PKCE, and mints codes, refresh tokens, and
//!   the signed ID/access tokens. [`endpoints`] is the authorize/token/userinfo
//!   surface; [`discovery`] is the two public `.well-known` documents.
//!
//! **Session bridge (silent SSO).** The authorize endpoint reads the Elysium
//! session cookie directly (not the `SessionUser` extractor, which would `401`): a
//! signed-in user whose auth policies pass gets a code with no second prompt, and
//! an anonymous user is bounced to the SPA login and returns. Auth policies
//! (suspension, org membership, mandatory-2FA) are enforced **before** any code or
//! token is issued, reusing the same checks the rest of the app uses
//! ([`crate::authz::role_in_org`], [`crate::context::mandatory_totp_missing`]).

pub mod clients;
pub mod discovery;
pub mod endpoints;
pub mod keys;
pub mod tokens;

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// The scopes this provider understands. A client may request any subset; `openid`
/// is required on the authorize request (it is what makes the request an OIDC one).
///
/// `offline_access` is what gates issuing a refresh token, per OIDC core.
pub const SUPPORTED_SCOPES: [&str; 4] = ["openid", "profile", "email", "offline_access"];

/// The grant types the token endpoint honors.
pub const SUPPORTED_GRANT_TYPES: [&str; 2] = ["authorization_code", "refresh_token"];

/// Constant-time byte equality, so a wrong secret leaks no length-independent timing.
///
/// Used to compare a presented client secret and a PKCE challenge. Returns `false`
/// immediately on a length mismatch (the lengths are not themselves secret), then
/// folds every byte so the loop time does not depend on where the first difference
/// falls.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// A token-endpoint error, rendered as the standard OAuth JSON error body.
///
/// The `error` codes are the RFC 6749 set. `invalid_client` answers `401` (an
/// authentication failure); every other client error is `400`; an internal fault is
/// `500` with the neutral `server_error` code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    InvalidRequest,
    InvalidClient,
    InvalidGrant,
    UnsupportedGrantType,
    InvalidScope,
    ServerError,
}

impl TokenError {
    /// The RFC 6749 `error` code string.
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidClient => "invalid_client",
            Self::InvalidGrant => "invalid_grant",
            Self::UnsupportedGrantType => "unsupported_grant_type",
            Self::InvalidScope => "invalid_scope",
            Self::ServerError => "server_error",
        }
    }

    /// The HTTP status the error is returned with.
    pub fn status(self) -> StatusCode {
        match self {
            Self::InvalidClient => StatusCode::UNAUTHORIZED,
            Self::ServerError => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for TokenError {
    fn into_response(self) -> Response {
        // Token responses must not be cached, per RFC 6749 §5.1; the same applies
        // to error responses that could otherwise be replayed from a cache.
        let mut response = (self.status(), Json(json!({ "error": self.code() }))).into_response();
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_only_equal_slices() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        // A length mismatch is never equal.
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn token_error_codes_and_statuses_follow_the_spec() {
        assert_eq!(TokenError::InvalidGrant.code(), "invalid_grant");
        assert_eq!(TokenError::InvalidClient.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(TokenError::InvalidRequest.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            TokenError::ServerError.status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
