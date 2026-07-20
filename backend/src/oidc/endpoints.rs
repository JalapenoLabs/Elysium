//! The interactive OIDC surface: authorize, token, and userinfo.
//!
//! [`authorize`] is session-bridged: it reads the Elysium session cookie directly
//! so a signed-in user gets a code with no second prompt (silent SSO) and an
//! anonymous user is bounced to the SPA login and returns. It validates the client
//! and an **exact** redirect URI before anything else, so a code (or an error) is
//! never returned to an unvalidated destination. [`token`] authenticates the
//! client, verifies the code and PKCE (or a refresh token), and mints the signed
//! ID/access tokens. [`userinfo`] answers a bearer access token with fresh claims.

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{Form, Json};
use axum_extra::extract::cookie::CookieJar;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::authz;
use crate::oidc::clients::{self, Client};
use crate::oidc::tokens::{self, NewCode, Subject};
use crate::oidc::{keys, TokenError, SUPPORTED_SCOPES};
use crate::session;
use crate::state::AppState;
use crate::users;

// --- Authorize endpoint ------------------------------------------------------

/// The authorize request query parameters (all optional so we validate ourselves).
#[derive(Debug, Deserialize)]
pub struct AuthorizeParams {
    #[serde(default)]
    response_type: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code_challenge: Option<String>,
    #[serde(default)]
    code_challenge_method: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
}

/// `GET /api/v1/oidc/authorize`: the session-bridged authorization endpoint.
///
/// Validates the client and an exact redirect URI first; only then may errors ride
/// the redirect. A valid session that passes auth policies yields a `302` back to
/// the client with a fresh code; no session redirects to the SPA login with a
/// `return_to` back here (silent thereafter). A policy failure is `access_denied`
/// on the validated redirect.
pub async fn authorize(
    State(state): State<AppState>,
    jar: CookieJar,
    uri: Uri,
    Query(params): Query<AuthorizeParams>,
) -> Response {
    // 1) The client and redirect URI are validated before anything can redirect: a
    //    code must never travel to an unvalidated destination.
    let Some(client_id) = params.client_id.as_deref().filter(|id| !id.is_empty()) else {
        return bad_request("missing client_id");
    };
    let client = match clients::get_client(&state.pool, client_id).await {
        Ok(Some(client)) => client,
        Ok(None) => return bad_request("unknown client_id"),
        Err(error) => {
            tracing::error!(error = %error, "oidc authorize client lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    let Some(redirect_uri) = params.redirect_uri.as_deref().filter(|uri| !uri.is_empty()) else {
        return bad_request("missing redirect_uri");
    };
    if !client.allows_redirect(redirect_uri) {
        // Never redirect to an unregistered URI; this is a plain error page.
        return bad_request("redirect_uri is not registered for this client");
    }
    let state_param = params.state.as_deref();

    // 2) From here, a protocol error is returned on the validated redirect URI.
    if params.response_type.as_deref() != Some("code") {
        return redirect_error(redirect_uri, "unsupported_response_type", state_param);
    }

    let scopes = parse_scopes(params.scope.as_deref());
    if !scopes.iter().any(|scope| scope == "openid") {
        return redirect_error(redirect_uri, "invalid_scope", state_param);
    }
    if let Some(error) = validate_scopes(&scopes, &client) {
        return redirect_error(redirect_uri, error, state_param);
    }

    // PKCE is mandatory: a present challenge with the S256 method.
    let Some(code_challenge) = params.code_challenge.as_deref().filter(|c| !c.is_empty()) else {
        return redirect_error(redirect_uri, "invalid_request", state_param);
    };
    let challenge_method = params.code_challenge_method.as_deref().unwrap_or("S256");
    if challenge_method != "S256" {
        return redirect_error(redirect_uri, "invalid_request", state_param);
    }

    // 3) Session bridge: no live session means send the user to log in and return.
    let Some(session_id) = session::session_id(&jar) else {
        return redirect_to_login(&uri);
    };
    let principal = match session::principal(&state.pool, session_id).await {
        Ok(Some(principal)) => principal,
        Ok(None) => return redirect_to_login(&uri),
        Err(error) => {
            tracing::error!(error = %error, "oidc authorize session lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // 4) Enforce auth policies before issuing anything (suspension, org membership,
    //    mandatory 2FA), reusing the same checks the rest of the app uses.
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return redirect_error(redirect_uri, "access_denied", state_param),
        Err(error) => {
            tracing::error!(error = %error, "oidc authorize user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    match enforce_policies(&state, &principal.sub, user_id, principal.active_org_id).await {
        Ok(true) => {}
        Ok(false) => return redirect_error(redirect_uri, "access_denied", state_param),
        Err(error) => {
            tracing::error!(error = %error, "oidc authorize policy check failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    // 5) Mint a single-use code bound to this client, redirect, PKCE challenge, and
    //    org context, then redirect it back to the client.
    let code = tokens::issue_code(
        &state.pool,
        NewCode {
            client_id,
            user_id,
            org_id: principal.active_org_id,
            redirect_uri,
            scopes: &scopes,
            code_challenge,
            code_challenge_method: challenge_method,
            nonce: params.nonce.as_deref(),
        },
    )
    .await;

    match code {
        Ok(code) => {
            let location = build_redirect(
                redirect_uri,
                &[("code", Some(&code)), ("state", state_param)],
            );
            found(&location)
        }
        Err(error) => {
            tracing::error!(error = %error, "oidc authorization code issuance failed");
            redirect_error(redirect_uri, "server_error", state_param)
        }
    }
}

/// Whether the caller passes the auth policies required before token issuance.
///
/// Reuses [`authz::role_in_org`] (a live, non-suspended membership) and
/// [`crate::context::mandatory_totp_missing`] (mandatory 2FA satisfied) rather than
/// re-deriving them. A `None` active org needs no membership check; a mandating org
/// with no confirmed TOTP is refused.
///
/// # Errors
/// Returns an error if a policy query fails.
async fn enforce_policies(
    state: &AppState,
    subject: &str,
    user_id: Uuid,
    active_org_id: Option<Uuid>,
) -> sqlx::Result<bool> {
    if let Some(org_id) = active_org_id {
        // A suspended or non-member caller has no effective role: refuse.
        if authz::role_in_org(&state.pool, user_id, org_id)
            .await?
            .is_none()
        {
            return Ok(false);
        }
    }
    // Mandatory 2FA must be satisfied for the profile before SSO into a service.
    if crate::context::mandatory_totp_missing(state, subject).await? {
        return Ok(false);
    }
    Ok(true)
}

// --- Token endpoint ----------------------------------------------------------

/// The token request form body (`application/x-www-form-urlencoded`).
#[derive(Debug, Deserialize)]
pub struct TokenParams {
    #[serde(default)]
    grant_type: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// `POST /api/v1/oidc/token`: exchange a code (or refresh token) for tokens.
///
/// The client authenticates by HTTP Basic or `client_secret_post`; a public client
/// presents no secret and proves itself with PKCE. Answers the standard JSON token
/// response or an RFC 6749 error body.
pub async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(params): Form<TokenParams>,
) -> Response {
    // Resolve client credentials from Basic auth or the form body.
    let (client_id, client_secret) = match client_credentials(&headers, &params) {
        Some(creds) => creds,
        None => return TokenError::InvalidClient.into_response(),
    };

    let client = match clients::get_client(&state.pool, &client_id).await {
        Ok(Some(client)) => client,
        Ok(None) => return TokenError::InvalidClient.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "oidc token client lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    match clients::authenticate(&client, &state.crypto, client_secret.as_deref()) {
        Ok(true) => {}
        Ok(false) => return TokenError::InvalidClient.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "oidc client authentication failed");
            return TokenError::ServerError.into_response();
        }
    }

    match params.grant_type.as_deref() {
        Some("authorization_code") => authorization_code_grant(&state, &client, &params).await,
        Some("refresh_token") => refresh_token_grant(&state, &client, &params).await,
        _ => TokenError::UnsupportedGrantType.into_response(),
    }
}

/// Completes the `authorization_code` grant: verify the code and PKCE, then issue.
async fn authorization_code_grant(
    state: &AppState,
    client: &Client,
    params: &TokenParams,
) -> Response {
    if !client.allows_grant("authorization_code") {
        return TokenError::UnsupportedGrantType.into_response();
    }
    let Some(code) = params.code.as_deref().filter(|c| !c.is_empty()) else {
        return TokenError::InvalidRequest.into_response();
    };

    // Consume the code single-use; an unknown, expired, or reused code is invalid.
    let redeemed = match tokens::redeem_code(&state.pool, code).await {
        Ok(Some(redeemed)) => redeemed,
        Ok(None) => return TokenError::InvalidGrant.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "oidc code redemption failed");
            return TokenError::ServerError.into_response();
        }
    };

    // The code must belong to this client and to the same redirect URI it was
    // bound to at authorize time.
    if redeemed.client_id != client.client_id {
        return TokenError::InvalidGrant.into_response();
    }
    if params.redirect_uri.as_deref() != Some(redeemed.redirect_uri.as_str()) {
        return TokenError::InvalidGrant.into_response();
    }

    // PKCE: the verifier must hash to the stored challenge (S256 only).
    let Some(verifier) = params.code_verifier.as_deref().filter(|v| !v.is_empty()) else {
        return TokenError::InvalidGrant.into_response();
    };
    if !tokens::verify_pkce(
        verifier,
        &redeemed.code_challenge,
        &redeemed.code_challenge_method,
    ) {
        return TokenError::InvalidGrant.into_response();
    }

    // Defense in depth: honor a narrowing of the client's scopes since the code was
    // issued, so a code can never be redeemed for more than the client may hold now.
    if let Some(response) = reject_ungranted_scopes(&redeemed.scopes, client) {
        return response;
    }

    issue(
        state,
        &client.client_id,
        redeemed.user_id,
        redeemed.org_id,
        &redeemed.scopes,
        redeemed.nonce.as_deref(),
    )
    .await
}

/// Completes the `refresh_token` grant: rotate the token and re-issue.
async fn refresh_token_grant(state: &AppState, client: &Client, params: &TokenParams) -> Response {
    if !client.allows_grant("refresh_token") {
        return TokenError::UnsupportedGrantType.into_response();
    }
    let Some(refresh) = params.refresh_token.as_deref().filter(|t| !t.is_empty()) else {
        return TokenError::InvalidRequest.into_response();
    };

    // Consume the presented token (rotation); an invalid or reused one is refused.
    let redeemed = match tokens::redeem_refresh_token(&state.pool, &client.client_id, refresh).await
    {
        Ok(Some(redeemed)) => redeemed,
        Ok(None) => return TokenError::InvalidGrant.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "oidc refresh redemption failed");
            return TokenError::ServerError.into_response();
        }
    };

    if let Some(response) = reject_ungranted_scopes(&redeemed.scopes, client) {
        return response;
    }

    issue(
        state,
        &client.client_id,
        redeemed.user_id,
        redeemed.org_id,
        &redeemed.scopes,
        redeemed.nonce.as_deref(),
    )
    .await
}

/// Refuses (as `invalid_scope`) any scope no longer granted to the client.
///
/// A code or refresh token records the scopes granted at authorize time; if an
/// admin has since narrowed the client's allowed scopes, this rejects the exchange
/// rather than re-issue a token broader than the client may now hold.
fn reject_ungranted_scopes(scopes: &[String], client: &Client) -> Option<Response> {
    let ungranted = scopes
        .iter()
        .any(|scope| !client.scopes.iter().any(|allowed| allowed == scope));
    ungranted.then(|| TokenError::InvalidScope.into_response())
}

/// Loads the subject and signs the token response, or an error if the user is gone.
async fn issue(
    state: &AppState,
    client_id: &str,
    user_id: Uuid,
    org_id: Option<Uuid>,
    scopes: &[String],
    nonce: Option<&str>,
) -> Response {
    let subject = match tokens::load_subject(&state.pool, user_id, org_id).await {
        Ok(Some(subject)) => subject,
        // The account vanished between authorize and token: treat as invalid grant.
        Ok(None) => return TokenError::InvalidGrant.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "oidc subject load failed");
            return TokenError::ServerError.into_response();
        }
    };

    match tokens::issue_tokens(state, client_id, &subject, scopes, nonce).await {
        Ok(issued) => {
            let mut body = json!({
                "access_token": issued.access_token,
                "id_token": issued.id_token,
                "token_type": "Bearer",
                "expires_in": issued.expires_in,
                "scope": issued.scope,
            });
            if let Some(refresh) = issued.refresh_token {
                body["refresh_token"] = Value::String(refresh);
            }
            let mut response = Json(body).into_response();
            // Token responses must not be cached (RFC 6749 §5.1).
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(error) => {
            tracing::error!(error = %error, "oidc token issuance failed");
            TokenError::ServerError.into_response()
        }
    }
}

// --- Userinfo endpoint -------------------------------------------------------

/// `GET /api/v1/oidc/userinfo`: the claims for a presented bearer access token.
///
/// Verifies the token's RS256 signature against the published key selected by its
/// `kid`, checks the issuer and expiry, then returns fresh claims loaded from the
/// database keyed by the token's subject. An invalid or missing token is `401` with
/// a `WWW-Authenticate: Bearer` challenge.
pub async fn userinfo(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(token) = bearer_token(&headers) else {
        return unauthorized_bearer("invalid_token");
    };

    let verified = match verify_access_token(&state, &token).await {
        Ok(Some(claims)) => claims,
        Ok(None) => return unauthorized_bearer("invalid_token"),
        Err(error) => {
            tracing::error!(error = %error, "oidc userinfo token verification failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let subject = match tokens::load_subject(&state.pool, verified.user_id, verified.org_id).await {
        Ok(Some(subject)) => subject,
        Ok(None) => return unauthorized_bearer("invalid_token"),
        Err(error) => {
            tracing::error!(error = %error, "oidc userinfo subject load failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    Json(userinfo_body(&subject)).into_response()
}

/// The identity recovered from a verified access token.
struct VerifiedToken {
    user_id: Uuid,
    org_id: Option<Uuid>,
}

/// Verifies an access token and extracts its subject and org context.
///
/// # Errors
/// Returns an error only if the key store is unreachable; a bad token is `Ok(None)`.
async fn verify_access_token(state: &AppState, token: &str) -> eyre::Result<Option<VerifiedToken>> {
    use jsonwebtoken::{decode, decode_header, Algorithm, Validation};

    let Ok(header) = decode_header(token) else {
        return Ok(None);
    };
    let Some(kid) = header.kid else {
        return Ok(None);
    };
    let Some(decoding) = keys::verifier_for_kid(&state.pool, &kid).await? else {
        return Ok(None);
    };

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[state.app_origin.as_str()]);
    // The audience is the client id, not a fixed value; the signature and issuer
    // are what prove the token is ours, so audience validation is disabled here.
    validation.validate_aud = false;

    let Ok(data) = decode::<tokens::Claims>(token, &decoding, &validation) else {
        return Ok(None);
    };
    let Ok(user_id) = Uuid::parse_str(&data.claims.sub) else {
        return Ok(None);
    };
    let org_id = data
        .claims
        .org
        .as_ref()
        .and_then(|org| Uuid::parse_str(&org.id).ok());
    Ok(Some(VerifiedToken { user_id, org_id }))
}

/// Renders a subject as the userinfo JSON response.
fn userinfo_body(subject: &Subject) -> Value {
    let mut body = json!({
        "sub": subject.user_id.to_string(),
        "email": subject.email,
        "email_verified": subject.email_verified,
        "name": subject.name,
        "given_name": subject.given_name,
        "family_name": subject.family_name,
        "roles": subject.roles,
        "groups": subject.groups,
    });
    if let Some(org) = &subject.org {
        body["org"] = json!({ "id": org.id, "slug": org.slug, "role": org.role });
    }
    body
}

// --- Shared helpers ----------------------------------------------------------

/// Splits a space-delimited `scope` string into a deduplicated list.
fn parse_scopes(scope: Option<&str>) -> Vec<String> {
    let mut scopes: Vec<String> = Vec::new();
    for token in scope.unwrap_or_default().split_whitespace() {
        let token = token.to_string();
        if !scopes.contains(&token) {
            scopes.push(token);
        }
    }
    scopes
}

/// Returns an error code when a requested scope is unsupported or not granted to
/// the client; `None` when every requested scope is permitted.
fn validate_scopes(scopes: &[String], client: &Client) -> Option<&'static str> {
    for scope in scopes {
        if !SUPPORTED_SCOPES.contains(&scope.as_str()) {
            return Some("invalid_scope");
        }
        if !client.scopes.iter().any(|allowed| allowed == scope) {
            return Some("invalid_scope");
        }
    }
    None
}

/// Resolves the token client's id and optional secret from Basic auth or the body.
///
/// HTTP Basic (`client_secret_basic`) wins when present; otherwise the form's
/// `client_id`/`client_secret` (`client_secret_post`) are used. Returns `None` when
/// no client id can be determined at all.
fn client_credentials(
    headers: &HeaderMap,
    params: &TokenParams,
) -> Option<(String, Option<String>)> {
    if let Some((id, secret)) = basic_auth(headers) {
        return Some((id, Some(secret)));
    }
    let client_id = params.client_id.as_deref().filter(|id| !id.is_empty())?;
    Some((
        client_id.to_string(),
        params
            .client_secret
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    ))
}

/// Decodes an HTTP Basic `Authorization` header into `(client_id, client_secret)`.
fn basic_auth(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let encoded = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))?;
    let decoded = BASE64_STANDARD.decode(encoded.trim()).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (id, secret) = decoded.split_once(':')?;
    Some((id.to_string(), secret.to_string()))
}

/// Extracts a bearer token from the `Authorization` header, if present.
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(|token| token.trim().to_string())
}

/// A `400` error page for a request that cannot safely redirect.
fn bad_request(message: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, message).into_response()
}

/// A `401` bearer challenge for a bad or missing access token.
fn unauthorized_bearer(error: &'static str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            header::HeaderValue::from_str(&format!("Bearer error=\"{error}\""))
                .unwrap_or_else(|_| header::HeaderValue::from_static("Bearer")),
        )],
    )
        .into_response()
}

/// Builds a `302` redirect to the client with an OAuth `error` (and echoed state).
fn redirect_error(redirect_uri: &str, error: &str, state: Option<&str>) -> Response {
    let location = build_redirect(redirect_uri, &[("error", Some(error)), ("state", state)]);
    found(&location)
}

/// Sends an anonymous user to the SPA login, returning to this authorize URL.
///
/// The full authorize path and query are the `return_to`, so after signing in the
/// SPA replays the exact request and the second pass finds a live session (silent
/// thereafter).
fn redirect_to_login(uri: &Uri) -> Response {
    let target = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_string(), |pq| pq.to_string());
    found(&format!("/login?return_to={}", percent_encode(&target)))
}

/// A `302 Found` to `location`, or `500` if the location is not a valid header.
fn found(location: &str) -> Response {
    match header::HeaderValue::from_str(location) {
        Ok(value) => (StatusCode::FOUND, [(header::LOCATION, value)]).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Appends query parameters to a base URL, percent-encoding each value.
///
/// A `None` value drops the pair (an absent `state` is not echoed). The separator
/// is chosen from whether the base already carries a query string.
fn build_redirect(base: &str, pairs: &[(&str, Option<&str>)]) -> String {
    let mut url = base.to_string();
    let mut separator = if base.contains('?') { '&' } else { '?' };
    for (key, value) in pairs {
        let Some(value) = value else { continue };
        url.push(separator);
        url.push_str(key);
        url.push('=');
        url.push_str(&percent_encode(value));
        separator = '&';
    }
    url
}

/// Percent-encodes a query value, preserving only the RFC 3986 unreserved set.
///
/// Self-contained so the crate needs no URL dependency: everything outside
/// `A-Za-z0-9-._~` becomes `%XX`, which is safe for both query keys and values.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            other => {
                out.push('%');
                out.push_str(&format!("{other:02X}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scopes_splits_and_deduplicates() {
        assert_eq!(
            parse_scopes(Some("openid profile openid email")),
            vec!["openid", "profile", "email"]
        );
        assert!(parse_scopes(None).is_empty());
        assert!(parse_scopes(Some("   ")).is_empty());
    }

    #[test]
    fn percent_encode_preserves_unreserved_and_escapes_the_rest() {
        assert_eq!(percent_encode("abcXYZ019-._~"), "abcXYZ019-._~");
        // A space, slash, and colon are escaped; state values ride through safely.
        assert_eq!(percent_encode("a b/c:d"), "a%20b%2Fc%3Ad");
    }

    #[test]
    fn build_redirect_chooses_the_separator_and_drops_absent_pairs() {
        // No existing query: first pair uses '?'.
        assert_eq!(
            build_redirect("https://svc/cb", &[("code", Some("xyz")), ("state", None)]),
            "https://svc/cb?code=xyz"
        );
        // Existing query: pairs append with '&', and state is percent-encoded.
        assert_eq!(
            build_redirect(
                "https://svc/cb?x=1",
                &[("code", Some("abc")), ("state", Some("a b"))]
            ),
            "https://svc/cb?x=1&code=abc&state=a%20b"
        );
    }

    #[test]
    fn basic_auth_decodes_client_credentials() {
        let mut headers = HeaderMap::new();
        // base64("svc:secret") = c3ZjOnNlY3JldA==
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_static("Basic c3ZjOnNlY3JldA=="),
        );
        assert_eq!(
            basic_auth(&headers),
            Some(("svc".to_string(), "secret".to_string()))
        );
    }

    #[test]
    fn bearer_token_is_extracted_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_static("Bearer abc.def.ghi"),
        );
        assert_eq!(bearer_token(&headers).as_deref(), Some("abc.def.ghi"));
        // No header yields nothing.
        assert!(bearer_token(&HeaderMap::new()).is_none());
    }

    #[test]
    fn client_credentials_prefer_basic_over_body() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_static("Basic c3ZjOnNlY3JldA=="),
        );
        let params = TokenParams {
            grant_type: None,
            code: None,
            redirect_uri: None,
            client_id: Some("other".to_string()),
            client_secret: Some("body-secret".to_string()),
            code_verifier: None,
            refresh_token: None,
        };
        // Basic auth wins when present.
        assert_eq!(
            client_credentials(&headers, &params),
            Some(("svc".to_string(), Some("secret".to_string())))
        );
        // Falls back to the body when no Basic header is present.
        assert_eq!(
            client_credentials(&HeaderMap::new(), &params),
            Some(("other".to_string(), Some("body-secret".to_string())))
        );
    }

    // --- DB-gated silent SSO and policy enforcement --------------------------
    //
    // Need a reachable postgres; ignored by default, run in CI with
    // `cargo test -- --ignored`. Each test seeds its own throwaway org, user, and
    // session so it never collides with other DB tests.

    use std::sync::Arc;

    use axum_extra::extract::cookie::Cookie;
    use opaque_ke::ServerSetup;
    use rand::rngs::OsRng;

    use crate::crypto::Crypto;
    use crate::opaque::Suite;
    use crate::session::SESSION_COOKIE;

    /// Builds a full [`AppState`] around a connected pool for the flow tests.
    async fn connect_test_state() -> AppState {
        use sqlx::postgres::PgPoolOptions;
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this test");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("could not connect to postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");
        let opaque_setup = Arc::new(ServerSetup::<Suite>::new(&mut OsRng));
        let crypto = Crypto::new(&[8u8; 32]).expect("32-byte test key");
        AppState::new(pool, opaque_setup, crypto).expect("app state builds")
    }

    /// Seeds an org, a user, an owner membership, and a live session on it.
    ///
    /// Returns `(user_id, subject, org_id, session_id)`.
    async fn seed_session(state: &AppState) -> (Uuid, String, Uuid, Uuid) {
        let pool = &state.pool;
        let suffix = Uuid::new_v4();

        let org_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO orgs (name, slug) VALUES ('Test', $1) RETURNING id",
        )
        .bind(format!("org-{suffix}"))
        .fetch_one(pool)
        .await
        .expect("seed org");

        let subject = format!("subject-{suffix}");
        let user_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email) VALUES ($1, $2) RETURNING id",
        )
        .bind(&subject)
        .bind(format!("user-{suffix}@example.test"))
        .fetch_one(pool)
        .await
        .expect("seed user");

        sqlx::query("INSERT INTO org_members (user_id, org_id, role) VALUES ($1, $2, 'owner')")
            .bind(user_id)
            .bind(org_id)
            .execute(pool)
            .await
            .expect("seed membership");

        let session_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO sessions (active_user_id, active_org_id, expires_at)
             VALUES ($1, $2, now() + interval '1 day') RETURNING id",
        )
        .bind(user_id)
        .bind(org_id)
        .fetch_one(pool)
        .await
        .expect("seed session");
        sqlx::query("INSERT INTO session_accounts (session_id, user_id) VALUES ($1, $2)")
            .bind(session_id)
            .bind(user_id)
            .execute(pool)
            .await
            .expect("seed session account");

        (user_id, subject, org_id, session_id)
    }

    /// Registers a first-party client for the authorize flow.
    async fn seed_client(state: &AppState) -> String {
        let client_id = format!("client-{}", Uuid::new_v4());
        sqlx::query(
            "INSERT INTO oidc_clients (client_id, name, redirect_uris, scopes, grant_types, is_first_party)
             VALUES ($1, 'Test', ARRAY['https://svc.example/cb'], ARRAY['openid','profile'],
                     ARRAY['authorization_code'], TRUE)",
        )
        .bind(&client_id)
        .execute(&state.pool)
        .await
        .expect("seed client");
        client_id
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn existing_session_authorizes_a_code_without_re_auth() {
        let state = connect_test_state().await;
        let (user_id, _subject, _org_id, session_id) = seed_session(&state).await;
        let client_id = seed_client(&state).await;

        // A real PKCE S256 challenge, though authorize only records it.
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(<sha2::Sha256 as sha2::Digest>::digest(b"verifier"));
        let query = format!(
            "response_type=code&client_id={client_id}&redirect_uri=https://svc.example/cb\
             &scope=openid%20profile&state=xyz&code_challenge={challenge}&code_challenge_method=S256"
        );
        let uri: Uri = format!("/api/v1/oidc/authorize?{query}")
            .parse()
            .expect("valid uri");
        let jar = CookieJar::new().add(Cookie::new(SESSION_COOKIE, session_id.to_string()));

        let params = AuthorizeParams {
            response_type: Some("code".to_string()),
            client_id: Some(client_id.clone()),
            redirect_uri: Some("https://svc.example/cb".to_string()),
            scope: Some("openid profile".to_string()),
            state: Some("xyz".to_string()),
            code_challenge: Some(challenge),
            code_challenge_method: Some("S256".to_string()),
            nonce: Some("n-1".to_string()),
        };

        let response = authorize(State(state.clone()), jar, uri, Query(params)).await;
        // Silent SSO: an existing session yields a 302 straight back to the client.
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .expect("a redirect location")
            .to_str()
            .expect("ascii location");
        assert!(location.starts_with("https://svc.example/cb?"));
        assert!(location.contains("state=xyz"));

        // The redirected code redeems to this user, proving no re-auth was needed.
        let code = location
            .split("code=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .expect("a code parameter");
        let redeemed = tokens::redeem_code(&state.pool, code)
            .await
            .expect("redeem query")
            .expect("the issued code redeems");
        assert_eq!(redeemed.user_id, user_id);
        assert_eq!(redeemed.client_id, client_id);
        assert_eq!(redeemed.nonce.as_deref(), Some("n-1"));
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn policies_pass_for_a_live_member_and_refuse_suspension_and_non_membership() {
        let state = connect_test_state().await;
        let (user_id, subject, org_id, _session_id) = seed_session(&state).await;

        // A live, non-suspended member with no 2FA mandate passes.
        assert!(enforce_policies(&state, &subject, user_id, Some(org_id))
            .await
            .expect("policy check"));

        // A suspended membership has no effective role: refused.
        sqlx::query(
            "UPDATE org_members SET suspended_at = now() WHERE user_id = $1 AND org_id = $2",
        )
        .bind(user_id)
        .bind(org_id)
        .execute(&state.pool)
        .await
        .expect("suspend member");
        assert!(!enforce_policies(&state, &subject, user_id, Some(org_id))
            .await
            .expect("policy check"));

        // An org the user does not belong to is refused.
        assert!(
            !enforce_policies(&state, &subject, user_id, Some(Uuid::new_v4()))
                .await
                .expect("policy check")
        );
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn mandatory_2fa_is_enforced_before_issuance() {
        let state = connect_test_state().await;
        let (user_id, subject, org_id, _session_id) = seed_session(&state).await;

        // The org mandates TOTP; the user has none confirmed, so SSO is refused.
        sqlx::query(
            "INSERT INTO org_auth_policies (org_id, require_totp) VALUES ($1, TRUE)
             ON CONFLICT (org_id) DO UPDATE SET require_totp = TRUE",
        )
        .bind(org_id)
        .execute(&state.pool)
        .await
        .expect("mandate totp");
        assert!(!enforce_policies(&state, &subject, user_id, Some(org_id))
            .await
            .expect("policy check"));

        // Once a confirmed authenticator exists, the 2FA policy is satisfied.
        sqlx::query(
            "INSERT INTO user_totp_secrets (user_id, secret, confirmed_at)
             VALUES ($1, $2, now())",
        )
        .bind(user_id)
        .bind(vec![0u8; 16])
        .execute(&state.pool)
        .await
        .expect("confirm totp");
        assert!(enforce_policies(&state, &subject, user_id, Some(org_id))
            .await
            .expect("policy check"));
    }
}
