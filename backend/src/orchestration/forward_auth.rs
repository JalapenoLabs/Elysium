//! Forward-auth for the reverse proxy: zero-prompt SSO into managed services.
//!
//! OIDC (`crate::oidc`) is the standards path, but it still involves a redirect
//! dance. For a truly seamless embedded UI the proxy instead **asserts** the
//! Elysium identity to the upstream via trusted request headers, which
//! Forgejo/Gitea/Harbor consume as reverse-proxy authentication. Opening a managed
//! service while signed into Elysium then lands authenticated with no visible login.
//!
//! [`authorize`] is the choke point [`crate::orchestration::routing::authorize_proxy`]
//! delegates to. It reuses the exact session and policy checks the OIDC authorize
//! endpoint uses (`session::session_id` + `session::principal`, then
//! `crate::oidc::endpoints::enforce_policies`), so a signed-out, suspended,
//! non-member, or 2FA-delinquent caller never reaches a service. A signed-in caller
//! yields the trusted identity header set ([`Identity::headers`]).
//!
//! # The trusted header contract
//!
//! On the allow path the proxy injects, and a managed service is configured to
//! trust, exactly these headers (see `config::render_git` for the Forgejo side):
//!
//! - `X-WEBAUTH-USER` — the stable login name (Forgejo's default reverse-proxy user
//!   header); the email local-part, sanitized to a valid username.
//! - `X-Elysium-User` — the stable subject (the account's uuid, matching the OIDC
//!   `sub`).
//! - `X-Elysium-Email` — the account's email.
//! - `X-Elysium-Name` — the account's display name (for JIT full-name).
//! - `X-Elysium-Groups` — comma-joined team slugs in the active org.
//!
//! # Spoofing defense
//!
//! These headers are meaningful only when Elysium sets them. A client must never be
//! able to present its own. [`strip_trusted_headers`] removes every one of them from
//! the inbound request before anything else, on **every** proxied path, so the only
//! values that can reach an upstream are the ones [`authorize`] injects. The proxy
//! runs the strip before it even reads the session, so a forged header can influence
//! neither the injected identity nor the auth decision.
//!
//! # Git transport tokens (HTTPS)
//!
//! A git client cannot carry an Elysium session cookie, so a smart-HTTP git request
//! with no session authenticates with an `ely_pat_` scoped token instead. The token
//! rides in the HTTP Basic password (the username is ignored; an empty password
//! falls back to the username, tolerating clients that put the token there), so
//! `git clone https://anything:<token>@host/services/{id}/org/repo.git` just works.
//! [`git_transport_op`] classifies the request: a fetch (`git-upload-pack`) needs the
//! `git.read` scope, a push (`git-receive-pack`) needs `git.write` specifically. A
//! token bound to an integration may transport only for that integration. On success
//! the same trusted [`Identity`] is asserted (Forgejo sees the token's owner, no git
//! login) and `last_used_at` is stamped best-effort. A missing or unresolvable
//! (unknown, expired, revoked) token yields `401` with
//! `WWW-Authenticate: Basic realm="Elysium Git"` so the client re-prompts; an
//! authenticated token lacking the scope, bound elsewhere, or whose owner fails the
//! policy gate yields `403`. Revocation fails transport at once, since resolution
//! checks live state. SSH transport auth is the SSH router's job (#30), which
//! resolves a presented key via [`crate::transport::resolve_ssh_key_owner`].

use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use sqlx::PgPool;
use uuid::Uuid;

use crate::oidc::tokens::{self, Subject};
use crate::orchestration::routing::ProxyAuth;
use crate::proto::security::v1::TokenScope;
use crate::session;
use crate::state::AppState;
use crate::transport::{self, TokenGrant};
use crate::users;

/// Forgejo's default reverse-proxy user header: the injected login name.
///
/// Lowercase because [`HeaderName::from_static`] requires it and header names are
/// case-insensitive on the wire; Forgejo's default `X-WEBAUTH-USER` matches it.
pub const HEADER_WEBAUTH_USER: &str = "x-webauth-user";

/// The stable Elysium subject (the account uuid), asserted to the upstream.
pub const HEADER_ELYSIUM_USER: &str = "x-elysium-user";

/// The account's email, asserted to the upstream (JIT registration email).
pub const HEADER_ELYSIUM_EMAIL: &str = "x-elysium-email";

/// The account's display name, asserted to the upstream (JIT full name).
pub const HEADER_ELYSIUM_NAME: &str = "x-elysium-name";

/// Comma-joined team slugs in the active org, asserted to the upstream.
pub const HEADER_ELYSIUM_GROUPS: &str = "x-elysium-groups";

/// Every identity header Elysium alone may set.
///
/// The proxy strips all of these from an inbound request ([`strip_trusted_headers`])
/// so a client can never spoof an identity, then injects the authenticated values.
pub const TRUSTED_IDENTITY_HEADERS: [&str; 5] = [
    HEADER_WEBAUTH_USER,
    HEADER_ELYSIUM_USER,
    HEADER_ELYSIUM_EMAIL,
    HEADER_ELYSIUM_NAME,
    HEADER_ELYSIUM_GROUPS,
];

/// Removes every trusted identity header a client may have supplied.
///
/// The spoofing defense: called on the inbound request before the session is read
/// and before anything is forwarded, so no client-supplied `X-WEBAUTH-USER` or
/// `X-Elysium-*` can ever reach an upstream or influence the auth decision. Only
/// [`authorize`] re-adds them, with the authenticated values.
pub fn strip_trusted_headers(headers: &mut HeaderMap) {
    for name in TRUSTED_IDENTITY_HEADERS {
        // `remove` drops the whole entry; the loop is defensive against a header
        // that was appended with multiple values.
        while headers.remove(name).is_some() {}
    }
}

/// The identity asserted to an upstream on the allow path.
///
/// Built from the same [`Subject`] an OIDC token would carry, so forward-auth and
/// OIDC assert one consistent identity for the same account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The stable login name (`X-WEBAUTH-USER`), sanitized to a valid username.
    pub username: String,
    /// The stable subject (`X-Elysium-User`): the account uuid.
    pub subject: String,
    /// The account's email (`X-Elysium-Email`).
    pub email: String,
    /// The account's display name (`X-Elysium-Name`).
    pub name: String,
    /// The account's team slugs in the active org (`X-Elysium-Groups`).
    pub groups: Vec<String>,
}

impl Identity {
    /// Derives the asserted identity from a loaded [`Subject`].
    ///
    /// The username is the email local-part sanitized to a Forgejo-valid name,
    /// falling back to the subject uuid when the local-part sanitizes to nothing.
    pub fn from_subject(subject: &Subject) -> Self {
        let username = webauth_username(&subject.email);
        let username = if username.is_empty() {
            subject.user_id.to_string()
        } else {
            username
        };
        Self {
            username,
            subject: subject.user_id.to_string(),
            email: subject.email.clone(),
            name: subject.name.clone(),
            groups: subject.groups.clone(),
        }
    }

    /// Builds the trusted identity headers to inject into the upstream request.
    ///
    /// A value that is not header-safe (e.g. a display name with control bytes) is
    /// omitted rather than failing the request; the username and subject are always
    /// ASCII and therefore always present.
    pub fn headers(&self) -> HeaderMap {
        let mut map = HeaderMap::with_capacity(TRUSTED_IDENTITY_HEADERS.len());
        insert_identity(&mut map, HEADER_WEBAUTH_USER, &self.username);
        insert_identity(&mut map, HEADER_ELYSIUM_USER, &self.subject);
        insert_identity(&mut map, HEADER_ELYSIUM_EMAIL, &self.email);
        insert_identity(&mut map, HEADER_ELYSIUM_NAME, &self.name);
        insert_identity(&mut map, HEADER_ELYSIUM_GROUPS, &self.groups.join(","));
        map
    }
}

/// Inserts one identity header, skipping (with a debug log) a non-header-safe value.
fn insert_identity(map: &mut HeaderMap, name: &'static str, value: &str) {
    match HeaderValue::from_str(value) {
        Ok(value) => {
            map.insert(HeaderName::from_static(name), value);
        }
        Err(_) => {
            tracing::debug!(
                header.name = name,
                "forward-auth identity value is not header-safe; header omitted",
            );
        }
    }
}

/// Sanitizes an email local-part into a stable, reverse-proxy-safe username.
///
/// Keeps `A-Za-z0-9`, `-`, `_`, and `.`, replacing anything else with `-`, then
/// trims those separators from the ends (Forgejo forbids them there). Returns the
/// empty string when nothing usable remains, so the caller can fall back.
///
/// Two distinct emails can share a local-part (`a@x.com`, `a@y.com`); the username
/// is therefore a best-effort human-facing handle, while `X-Elysium-User` (the
/// uuid) is the collision-free identity. A dedicated unique login column is the
/// clean long-term source for this handle.
fn webauth_username(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    let sanitized: String = local
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    sanitized
        .trim_matches(|c| matches!(c, '-' | '_' | '.'))
        .to_string()
}

/// Authorizes a proxied request and returns the identity to assert on success.
///
/// Mirrors `crate::oidc::endpoints::authorize`: read the session cookie, resolve the
/// active [`session::Principal`], enforce the same auth policies (suspension, org
/// membership, mandatory 2FA), then load the identity. A signed-out or policy-failed
/// caller is denied; a browser navigation is bounced to the SPA login (returning
/// afterward), everything else gets `401`.
pub(crate) async fn authorize(state: &AppState, parts: &Parts, integration_id: Uuid) -> ProxyAuth {
    let jar = CookieJar::from_headers(&parts.headers);

    // A live Elysium session is the primary path (the web UI, and any client that
    // carries the cookie). Without one, a git smart-HTTP request may still
    // authenticate with an `ely_pat_` scoped token; anything else is refused
    // (see `authorize_without_session`).
    let principal = match session_principal(&state.pool, &jar).await {
        Ok(Some(principal)) => principal,
        Ok(None) => return authorize_without_session(state, parts, integration_id).await,
        Err(error) => {
            tracing::error!(
                error = %error,
                integration.id = %integration_id,
                "forward-auth session lookup failed",
            );
            return ProxyAuth::Deny(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };

    // Resolve the profile and enforce the same policies OIDC does before SSO.
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(user_id)) => user_id,
        Ok(None) => return deny_unauthenticated(parts),
        Err(error) => {
            tracing::error!(error = %error, "forward-auth user lookup failed");
            return ProxyAuth::Deny(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    match crate::oidc::endpoints::enforce_policies(
        state,
        &principal.sub,
        user_id,
        principal.active_org_id,
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => return deny_unauthenticated(parts),
        Err(error) => {
            tracing::error!(error = %error, "forward-auth policy check failed");
            return ProxyAuth::Deny(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    }

    // Load the canonical identity (the same Subject an OIDC token would carry).
    let subject = match tokens::load_subject(&state.pool, user_id, principal.active_org_id).await {
        Ok(Some(subject)) => subject,
        Ok(None) => return deny_unauthenticated(parts),
        Err(error) => {
            tracing::error!(error = %error, "forward-auth subject load failed");
            return ProxyAuth::Deny(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };

    ProxyAuth::Allow {
        identity: Identity::from_subject(&subject).headers(),
    }
}

/// Resolves the live session's active principal, if the request carries one.
///
/// A missing or malformed cookie is `Ok(None)` (no session), letting the caller fall
/// through to token auth without a database round-trip.
///
/// # Errors
/// Returns an error if the session lookup query fails.
async fn session_principal(
    pool: &PgPool,
    jar: &CookieJar,
) -> sqlx::Result<Option<session::Principal>> {
    let Some(session_id) = session::session_id(jar) else {
        return Ok(None);
    };
    session::principal(pool, session_id).await
}

/// Authorizes a request that carries no Elysium session.
///
/// A git smart-HTTP request authenticates with an `ely_pat_` scoped token
/// ([`authorize_git_token`]); anything else keeps the #33 behavior: a browser
/// navigation bounces to login, an API request gets `401`.
async fn authorize_without_session(
    state: &AppState,
    parts: &Parts,
    integration_id: Uuid,
) -> ProxyAuth {
    match git_transport_op(parts.uri.path(), parts.uri.query()) {
        Some(op) => authorize_git_token(state, parts, integration_id, op).await,
        None => deny_unauthenticated(parts),
    }
}

/// The capability a git smart-HTTP request needs: read (fetch) or write (push).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitOp {
    /// A fetch/clone (`git-upload-pack`); requires [`TokenScope::GitRead`].
    Read,
    /// A push (`git-receive-pack`); requires [`TokenScope::GitWrite`].
    Write,
}

/// The git service name a fetch advertises and runs under.
const GIT_UPLOAD_PACK: &str = "git-upload-pack";

/// The git service name a push advertises and runs under.
const GIT_RECEIVE_PACK: &str = "git-receive-pack";

/// The realm on the git Basic-auth challenge, naming the credential a client must send.
const GIT_AUTH_REALM: &str = "Elysium Git";

/// Classifies a request as a git smart-HTTP transport op, or `None` if it is not one.
///
/// Git's HTTP transport has two phases, and each names its service the same way:
/// the ref-advertisement `GET .../info/refs?service=git-(upload|receive)-pack`, then
/// the RPC `POST .../git-(upload|receive)-pack`. `git-upload-pack` is a fetch (read),
/// `git-receive-pack` is a push (write). Any other path is not a git transport
/// request and stays on the session/redirect path.
fn git_transport_op(path: &str, query: Option<&str>) -> Option<GitOp> {
    // Ref-advertisement phase: the op is named by the `service` query parameter.
    if path.ends_with("/info/refs") {
        return match query.and_then(service_param) {
            Some(GIT_UPLOAD_PACK) => Some(GitOp::Read),
            Some(GIT_RECEIVE_PACK) => Some(GitOp::Write),
            _ => None,
        };
    }
    // RPC phase: the op is named by the final path segment.
    if path.ends_with("/git-upload-pack") {
        return Some(GitOp::Read);
    }
    if path.ends_with("/git-receive-pack") {
        return Some(GitOp::Write);
    }
    None
}

/// Extracts the `service` query-parameter value from a raw query string.
fn service_param(query: &str) -> Option<&str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("service="))
}

/// Authorizes a git smart-HTTP request against an `ely_pat_` scoped token.
///
/// Decodes the Basic credential, resolves the token (failing closed on an unknown,
/// expired, or revoked one), enforces the op's scope and the token's optional
/// integration bind, runs the same account-policy gate the session and OIDC paths
/// use, and on success asserts the token owner's trusted identity and stamps usage.
async fn authorize_git_token(
    state: &AppState,
    parts: &Parts,
    integration_id: Uuid,
    op: GitOp,
) -> ProxyAuth {
    // The PAT rides in the Basic password (username fallback). No credential means
    // the client has not authenticated yet: challenge it so git re-prompts/retries.
    let Some(token) =
        basic_credential(&parts.headers).and_then(|(user, pass)| token_from_basic(&user, &pass))
    else {
        return ProxyAuth::Deny(git_auth_challenge());
    };

    // Resolve the token to its owner and grant. An unknown/expired/revoked token
    // resolves to `None`; a bad credential is an auth challenge, not a hard refusal.
    let grant = match transport::resolve_access_token(&state.pool, &token).await {
        Ok(Some(grant)) => grant,
        Ok(None) => return ProxyAuth::Deny(git_auth_challenge()),
        Err(error) => {
            tracing::error!(error = %error, "git token resolution failed");
            return ProxyAuth::Deny(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };

    // Authenticated but unauthorized past this point is a `403`, distinct from the
    // `401` challenge above: the credential is valid, it just may not do this.
    if !grant_permits(&grant, op) {
        return ProxyAuth::Deny(forbidden());
    }
    if !bind_admits(grant.integration_id, integration_id) {
        return ProxyAuth::Deny(forbidden());
    }

    // The asserted identity's group claims and the membership policy are evaluated
    // in the context of the org that owns the targeted service.
    let org_id = match integration_org(&state.pool, integration_id).await {
        Ok(Some(org_id)) => org_id,
        Ok(None) => return ProxyAuth::Deny(forbidden()),
        Err(error) => {
            tracing::error!(error = %error, "git token integration lookup failed");
            return ProxyAuth::Deny(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    let subject = match users::subject_for_id(&state.pool, grant.user_id).await {
        Ok(Some(subject)) => subject,
        Ok(None) => return ProxyAuth::Deny(forbidden()),
        Err(error) => {
            tracing::error!(error = %error, "git token subject lookup failed");
            return ProxyAuth::Deny(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };

    // The same policy gate as the session and OIDC paths: a suspended, non-member,
    // or mandatory-2FA-delinquent owner never transports (parity with OIDC/SSO; a
    // PAT is not a substitute for a required second factor).
    match crate::oidc::endpoints::enforce_policies(state, &subject, grant.user_id, Some(org_id))
        .await
    {
        Ok(true) => {}
        Ok(false) => return ProxyAuth::Deny(forbidden()),
        Err(error) => {
            tracing::error!(error = %error, "git token policy check failed");
            return ProxyAuth::Deny(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    }

    // Load the canonical identity (the same Subject a session or OIDC token carries).
    let identity = match tokens::load_subject(&state.pool, grant.user_id, Some(org_id)).await {
        Ok(Some(subject)) => Identity::from_subject(&subject),
        Ok(None) => return ProxyAuth::Deny(forbidden()),
        Err(error) => {
            tracing::error!(error = %error, "git token subject load failed");
            return ProxyAuth::Deny(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };

    // Record usage out of band: the stamp must never fail or delay the transport.
    stamp_token_used_detached(&state.pool, grant.token_id);

    ProxyAuth::Allow {
        identity: identity.headers(),
    }
}

/// Whether `grant` carries the exact scope git operation `op` requires.
///
/// A push requires [`TokenScope::GitWrite`] specifically; a read-only token never
/// satisfies it, and a write-only token does not imply read.
fn grant_permits(grant: &TokenGrant, op: GitOp) -> bool {
    let required = match op {
        GitOp::Read => TokenScope::GitRead,
        GitOp::Write => TokenScope::GitWrite,
    };
    grant.scopes.contains(&required)
}

/// Whether a token's optional integration bind admits the targeted integration.
///
/// `None` binds nothing (any git integration the owner can reach); `Some(id)` admits
/// only that exact integration.
fn bind_admits(bound: Option<Uuid>, target: Uuid) -> bool {
    match bound {
        Some(bound) => bound == target,
        None => true,
    }
}

/// The org that owns an integration, or `None` when no such integration exists.
///
/// # Errors
/// Returns an error if the query fails.
async fn integration_org(pool: &PgPool, integration_id: Uuid) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar::<_, Uuid>("SELECT org_id FROM integrations WHERE id = $1")
        .bind(integration_id)
        .fetch_optional(pool)
        .await
}

/// Decodes an HTTP `Authorization: Basic` header into its username and password.
///
/// Returns `None` when the header is absent, is not the Basic scheme, is not valid
/// base64/UTF-8, or lacks the `:` separator. The scheme token is matched
/// case-insensitively, as HTTP auth schemes are.
fn basic_credential(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, encoded) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = STANDARD.decode(encoded.trim()).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (username, password) = decoded.split_once(':')?;
    Some((username.to_string(), password.to_string()))
}

/// Extracts the presented Elysium token from a decoded Basic credential.
///
/// Git sends `username:password` with the PAT as the password, so the password wins;
/// an empty password falls back to the username, tolerating clients that place the
/// token there. Both empty yields `None` (no credential presented).
fn token_from_basic(username: &str, password: &str) -> Option<String> {
    if !password.is_empty() {
        Some(password.to_string())
    } else if !username.is_empty() {
        Some(username.to_string())
    } else {
        None
    }
}

/// A `401` that asks a git client to retry with Basic credentials.
///
/// The `WWW-Authenticate` challenge is what makes git prompt for (or replay) a
/// credential rather than give up, and distinguishes "who are you?" from the `403`
/// "you may not do this".
fn git_auth_challenge() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            format!("Basic realm=\"{GIT_AUTH_REALM}\""),
        )],
    )
        .into_response()
}

/// A plain `403`: the caller authenticated but is not authorized for this request.
fn forbidden() -> Response {
    StatusCode::FORBIDDEN.into_response()
}

/// Stamps a token's usage without blocking the request or being able to fail it.
///
/// Spawned so a slow or failing stamp cannot delay or reject the transport it
/// records; a failure is logged and dropped.
fn stamp_token_used_detached(pool: &PgPool, token_id: Uuid) {
    let pool = pool.clone();
    tokio::spawn(async move {
        if let Err(error) = transport::stamp_token_used(&pool, token_id).await {
            tracing::debug!(error = %error, "git token usage stamp failed");
        }
    });
}

/// Denies a request that carries no usable identity.
///
/// A browser navigation (an `Accept` that lists `text/html`) is redirected to the
/// SPA login carrying `return_to`; every other request (API, git) gets a plain
/// `401`, so a non-browser client is never redirected.
fn deny_unauthenticated(parts: &Parts) -> ProxyAuth {
    ProxyAuth::Deny(unauthenticated_response(
        &parts.uri,
        wants_html(&parts.headers),
    ))
}

/// The response for an unauthenticated request: a login bounce or a `401`.
fn unauthenticated_response(uri: &Uri, wants_html: bool) -> Response {
    if wants_html {
        let target = uri
            .path_and_query()
            .map_or_else(|| uri.path().to_string(), ToString::to_string);
        found(&format!("/login?return_to={}", percent_encode(&target)))
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

/// Whether the request is a browser navigation (its `Accept` lists `text/html`).
fn wants_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| {
            accept
                .split(',')
                .any(|media_type| media_type.trim().starts_with("text/html"))
        })
}

/// A `302 Found` to `location`, or `500` if the location is not a valid header.
fn found(location: &str) -> Response {
    match HeaderValue::from_str(location) {
        Ok(value) => (StatusCode::FOUND, [(header::LOCATION, value)]).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Percent-encodes a query value, preserving only the RFC 3986 unreserved set.
///
/// Self-contained (no URL dependency), matching the OIDC authorize endpoint's
/// `return_to` encoding: everything outside `A-Za-z0-9-._~` becomes `%XX`.
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

    // --- Header stripping and spoofing defense -------------------------------

    #[test]
    fn strip_trusted_headers_removes_every_client_supplied_identity_header() {
        let mut headers = HeaderMap::new();
        for name in TRUSTED_IDENTITY_HEADERS {
            headers.insert(
                HeaderName::from_static(name),
                HeaderValue::from_static("attacker"),
            );
        }
        // A benign header must survive the strip.
        headers.insert("cookie", HeaderValue::from_static("elysium_session=abc"));

        strip_trusted_headers(&mut headers);

        for name in TRUSTED_IDENTITY_HEADERS {
            assert!(headers.get(name).is_none(), "{name} must be stripped");
        }
        assert_eq!(headers.get("cookie").unwrap(), "elysium_session=abc");
    }

    #[test]
    fn spoofed_identity_header_is_stripped_then_replaced_with_the_authenticated_value() {
        // A client presents its own X-WEBAUTH-USER, hoping to impersonate.
        let mut headers = HeaderMap::new();
        headers.insert("x-webauth-user", HeaderValue::from_static("attacker"));
        headers.insert("x-elysium-email", HeaderValue::from_static("evil@x.test"));

        // The proxy strips inbound identity headers, then layers the authenticated
        // identity over the top (exactly what `routing::forward` does).
        strip_trusted_headers(&mut headers);
        let identity = Identity {
            username: "alex".to_string(),
            subject: "11111111-2222-3333-4444-555555555555".to_string(),
            email: "alex@example.test".to_string(),
            name: "Alex Rivera".to_string(),
            groups: vec!["dev".to_string()],
        };
        for (name, value) in &identity.headers() {
            headers.insert(name.clone(), value.clone());
        }

        // The spoofed values are gone; the authenticated ones stand.
        assert_eq!(headers.get("x-webauth-user").unwrap(), "alex");
        assert_eq!(headers.get("x-elysium-email").unwrap(), "alex@example.test");
    }

    // --- Identity header construction ----------------------------------------

    fn sample_subject() -> Subject {
        Subject {
            user_id: Uuid::from_u128(0x1234),
            email: "alex.rivera@example.test".to_string(),
            email_verified: true,
            name: "Alex Rivera".to_string(),
            given_name: "Alex".to_string(),
            family_name: "Rivera".to_string(),
            org: None,
            groups: vec!["dev".to_string(), "qa".to_string()],
            roles: vec!["admin".to_string()],
        }
    }

    #[test]
    fn identity_headers_carry_all_five_trusted_headers_with_correct_values() {
        let identity = Identity::from_subject(&sample_subject());
        let headers = identity.headers();

        assert_eq!(headers.len(), 5, "all five identity headers present");
        // Username is the sanitized email local-part.
        assert_eq!(headers.get(HEADER_WEBAUTH_USER).unwrap(), "alex.rivera");
        // Subject is the stable uuid (matches the OIDC `sub`).
        assert_eq!(
            headers.get(HEADER_ELYSIUM_USER).unwrap(),
            &Uuid::from_u128(0x1234).to_string()
        );
        assert_eq!(
            headers.get(HEADER_ELYSIUM_EMAIL).unwrap(),
            "alex.rivera@example.test"
        );
        assert_eq!(headers.get(HEADER_ELYSIUM_NAME).unwrap(), "Alex Rivera");
        // Groups are the team slugs, comma-joined.
        assert_eq!(headers.get(HEADER_ELYSIUM_GROUPS).unwrap(), "dev,qa");
    }

    #[test]
    fn identity_groups_are_empty_string_when_the_user_has_no_teams() {
        let mut subject = sample_subject();
        subject.groups = Vec::new();
        let headers = Identity::from_subject(&subject).headers();
        assert_eq!(headers.get(HEADER_ELYSIUM_GROUPS).unwrap(), "");
    }

    #[test]
    fn webauth_username_sanitizes_and_trims() {
        // Plain local-part rides through.
        assert_eq!(webauth_username("alex@example.test"), "alex");
        // Dots and dashes are kept; a plus becomes a dash then trims if trailing.
        assert_eq!(webauth_username("alex.rivera@x.test"), "alex.rivera");
        assert_eq!(webauth_username("a+b@x.test"), "a-b");
        // Leading/trailing separators are trimmed (Forgejo forbids them there).
        assert_eq!(webauth_username("+alex+@x.test"), "alex");
    }

    #[test]
    fn identity_username_falls_back_to_the_uuid_when_the_local_part_is_unusable() {
        let mut subject = sample_subject();
        // A local-part of only separators sanitizes to empty, forcing the fallback.
        subject.email = "+++@example.test".to_string();
        let identity = Identity::from_subject(&subject);
        assert_eq!(identity.username, subject.user_id.to_string());
    }

    #[test]
    fn identity_omits_a_header_whose_value_is_not_header_safe() {
        let mut subject = sample_subject();
        // A newline is not a valid header value; the name header is omitted, but the
        // always-safe headers (username, subject, email) remain.
        subject.name = "bad\nname".to_string();
        let headers = Identity::from_subject(&subject).headers();
        assert!(headers.get(HEADER_ELYSIUM_NAME).is_none());
        assert!(headers.get(HEADER_WEBAUTH_USER).is_some());
        assert!(headers.get(HEADER_ELYSIUM_EMAIL).is_some());
    }

    // --- Browser-vs-API deny decision ----------------------------------------

    #[test]
    fn wants_html_detects_a_browser_navigation() {
        let mut browser = HeaderMap::new();
        browser.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml,*/*;q=0.9"),
        );
        assert!(wants_html(&browser));

        let mut api = HeaderMap::new();
        api.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        assert!(!wants_html(&api));

        // A missing Accept is treated as a non-browser (git/API) request.
        assert!(!wants_html(&HeaderMap::new()));
    }

    #[test]
    fn browser_request_redirects_to_login_with_a_return_to() {
        let uri: Uri = "/services/abc/repo/tree?ref=main".parse().unwrap();
        let response = unauthenticated_response(&uri, true);
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        // The original service URL is the percent-encoded return_to.
        assert!(location.starts_with("/login?return_to="));
        assert!(location.contains("%2Fservices%2Fabc%2Frepo%2Ftree%3Fref%3Dmain"));
    }

    #[test]
    fn api_request_is_unauthorized_never_redirected() {
        let uri: Uri = "/services/abc/api/v1/repos".parse().unwrap();
        let response = unauthenticated_response(&uri, false);
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get(header::LOCATION).is_none());
    }

    // --- Git transport detection and classification --------------------------

    #[test]
    fn git_transport_op_classifies_the_ref_advertisement_phase() {
        // GET .../info/refs?service=git-upload-pack is a fetch (read).
        assert_eq!(
            git_transport_op(
                "/services/x/org/repo.git/info/refs",
                Some("service=git-upload-pack")
            ),
            Some(GitOp::Read)
        );
        // ...git-receive-pack is a push (write).
        assert_eq!(
            git_transport_op(
                "/services/x/org/repo.git/info/refs",
                Some("service=git-receive-pack")
            ),
            Some(GitOp::Write)
        );
    }

    #[test]
    fn git_transport_op_classifies_the_rpc_phase() {
        // POST .../git-upload-pack is a fetch; .../git-receive-pack is a push.
        assert_eq!(
            git_transport_op("/services/x/org/repo.git/git-upload-pack", None),
            Some(GitOp::Read)
        );
        assert_eq!(
            git_transport_op("/services/x/org/repo.git/git-receive-pack", None),
            Some(GitOp::Write)
        );
    }

    #[test]
    fn git_transport_op_reads_the_service_among_other_query_params() {
        // The service parameter is found regardless of order or extra params.
        assert_eq!(
            git_transport_op(
                "/services/x/org/repo.git/info/refs",
                Some("foo=1&service=git-upload-pack")
            ),
            Some(GitOp::Read)
        );
    }

    #[test]
    fn git_transport_op_ignores_non_git_requests() {
        // info/refs with no or an unknown service is not a git transport op.
        assert_eq!(
            git_transport_op("/services/x/repo.git/info/refs", None),
            None
        );
        assert_eq!(
            git_transport_op("/services/x/repo.git/info/refs", Some("service=bogus")),
            None
        );
        // An ordinary web/API path under a service is not a git transport op.
        assert_eq!(git_transport_op("/services/x/org/repo", None), None);
        assert_eq!(
            git_transport_op("/services/x/api/v1/repos", Some("page=1")),
            None
        );
    }

    // --- Basic-auth decode and token extraction ------------------------------

    fn basic_header(username: &str, password: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let encoded = STANDARD.encode(format!("{username}:{password}"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Basic {encoded}")).unwrap(),
        );
        headers
    }

    #[test]
    fn basic_credential_decodes_username_and_password() {
        let (user, pass) = basic_credential(&basic_header("git", "ely_pat_secret")).unwrap();
        assert_eq!(user, "git");
        assert_eq!(pass, "ely_pat_secret");
    }

    #[test]
    fn basic_credential_matches_the_scheme_case_insensitively() {
        let mut headers = HeaderMap::new();
        let encoded = STANDARD.encode("a:b");
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("basic {encoded}")).unwrap(),
        );
        assert_eq!(
            basic_credential(&headers),
            Some(("a".to_string(), "b".to_string()))
        );
    }

    #[test]
    fn basic_credential_rejects_malformed_input() {
        // No header at all.
        assert_eq!(basic_credential(&HeaderMap::new()), None);
        // A different scheme.
        let mut bearer = HeaderMap::new();
        bearer.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer xyz"),
        );
        assert_eq!(basic_credential(&bearer), None);
        // Not valid base64.
        let mut bad = HeaderMap::new();
        bad.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic not-base64!!"),
        );
        assert_eq!(basic_credential(&bad), None);
        // Decodes, but has no `:` separator.
        let mut no_colon = HeaderMap::new();
        no_colon.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Basic {}", STANDARD.encode("nocolon"))).unwrap(),
        );
        assert_eq!(basic_credential(&no_colon), None);
    }

    #[test]
    fn token_from_basic_prefers_the_password_then_the_username() {
        // The PAT is the password (git's `x:<token>` form).
        assert_eq!(
            token_from_basic("x", "ely_pat_a"),
            Some("ely_pat_a".to_string())
        );
        // An empty password falls back to the username.
        assert_eq!(
            token_from_basic("ely_pat_b", ""),
            Some("ely_pat_b".to_string())
        );
        // Both empty is no credential at all.
        assert_eq!(token_from_basic("", ""), None);
    }

    // --- Scope and integration-bind enforcement ------------------------------

    fn grant_with(scopes: Vec<TokenScope>, integration_id: Option<Uuid>) -> TokenGrant {
        TokenGrant {
            token_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            scopes,
            integration_id,
        }
    }

    #[test]
    fn grant_permits_requires_the_exact_scope_for_the_op() {
        let read_only = grant_with(vec![TokenScope::GitRead], None);
        let write_only = grant_with(vec![TokenScope::GitWrite], None);
        let both = grant_with(vec![TokenScope::GitRead, TokenScope::GitWrite], None);

        // A read scope allows a fetch but never a push.
        assert!(grant_permits(&read_only, GitOp::Read));
        assert!(!grant_permits(&read_only, GitOp::Write));
        // A write scope allows a push; it does not imply read.
        assert!(grant_permits(&write_only, GitOp::Write));
        assert!(!grant_permits(&write_only, GitOp::Read));
        // A full token does both.
        assert!(grant_permits(&both, GitOp::Read));
        assert!(grant_permits(&both, GitOp::Write));
    }

    #[test]
    fn bind_admits_matches_or_allows_when_unbound() {
        let target = Uuid::new_v4();
        // An unbound token admits any integration.
        assert!(bind_admits(None, target));
        // A bound token admits only its exact integration.
        assert!(bind_admits(Some(target), target));
        assert!(!bind_admits(Some(Uuid::new_v4()), target));
    }

    // --- 401 challenge vs 403 forbidden --------------------------------------

    #[test]
    fn git_auth_challenge_is_a_401_with_a_basic_realm() {
        let response = git_auth_challenge();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Basic realm=\"Elysium Git\""
        );
    }

    #[test]
    fn forbidden_is_a_403_without_a_challenge() {
        let response = forbidden();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        // A 403 is authenticated-but-unauthorized: it must not invite a retry.
        assert!(response.headers().get(header::WWW_AUTHENTICATE).is_none());
    }

    // --- Full authorize deny path (no DB needed: returns before any query) ----

    /// A state whose lazy pool never connects; the no-session path returns first.
    fn lazy_state() -> AppState {
        use std::sync::Arc;

        use opaque_ke::ServerSetup;
        use rand::rngs::OsRng;
        use sqlx::postgres::PgPoolOptions;

        use crate::crypto::Crypto;
        use crate::opaque::Suite;
        use crate::orchestration::provisioner::ProvisionerHandle;
        use crate::orchestration::routing::RouteTable;
        use crate::sse::SseRegistry;

        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://tester:tester@localhost:1/elysia_test")
            .expect("a lazy pool never connects at build time");
        AppState {
            pool,
            sse: SseRegistry::default(),
            webauthn: crate::webauthn::build("localhost", "http://localhost:5173")
                .expect("the test WebAuthn RP builds"),
            opaque_setup: Arc::new(ServerSetup::<Suite>::new(&mut OsRng)),
            crypto: Crypto::new(&[0u8; 32]).expect("a 32-byte test key is valid"),
            redis_url: None,
            redis: None,
            http: reqwest::Client::new(),
            geoip_endpoint: None,
            rabbitmq_url: None,
            app_origin: "http://localhost:5173".to_string(),
            data_dir: "/srv/elysium".to_string(),
            provisioner: ProvisionerHandle::in_memory(),
            routes: RouteTable::new(),
        }
    }

    fn parts_with(accept: &str, cookie: Option<&str>) -> Parts {
        use axum::body::Body;
        use axum::http::Request;

        let mut builder = Request::builder()
            .uri("/services/11111111-2222-3333-4444-555555555555/repo?x=1")
            .header(header::ACCEPT, accept);
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        builder.body(Body::empty()).unwrap().into_parts().0
    }

    #[tokio::test]
    async fn no_session_browser_request_is_denied_with_a_login_redirect() {
        let state = lazy_state();
        let parts = parts_with("text/html", None);
        let decision = authorize(&state, &parts, Uuid::new_v4()).await;
        let ProxyAuth::Deny(response) = decision else {
            panic!("a request with no session must be denied");
        };
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = response
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.starts_with("/login?return_to="));
        assert!(location.contains("%2Fservices%2F"));
    }

    #[tokio::test]
    async fn no_session_api_request_is_denied_with_401() {
        let state = lazy_state();
        let parts = parts_with("application/json", None);
        let decision = authorize(&state, &parts, Uuid::new_v4()).await;
        let ProxyAuth::Deny(response) = decision else {
            panic!("a request with no session must be denied");
        };
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// A git request with a bare URI and the given optional Authorization header.
    fn git_parts(uri: &str, authorization: Option<&str>) -> Parts {
        use axum::body::Body;
        use axum::http::Request;

        let mut builder = Request::builder().uri(uri);
        if let Some(authorization) = authorization {
            builder = builder.header(header::AUTHORIZATION, authorization);
        }
        builder.body(Body::empty()).unwrap().into_parts().0
    }

    #[tokio::test]
    async fn git_request_without_a_credential_is_challenged_never_redirected() {
        // Even with a browser Accept, a git transport request with no credential must
        // get the 401 Basic challenge (so git prompts), not a login redirect. This
        // returns before any query, so the lazy pool is never touched.
        let state = lazy_state();
        let integration_id = Uuid::new_v4();
        let uri =
            format!("/services/{integration_id}/org/repo.git/info/refs?service=git-upload-pack");
        let mut parts = git_parts(&uri, None);
        parts
            .headers
            .insert(header::ACCEPT, HeaderValue::from_static("text/html"));

        let decision = authorize(&state, &parts, integration_id).await;
        let ProxyAuth::Deny(response) = decision else {
            panic!("a git request with no credential must be denied");
        };
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Basic realm=\"Elysium Git\""
        );
        // Never a redirect: git must re-prompt, not bounce to the SPA login.
        assert!(response.headers().get(header::LOCATION).is_none());
    }

    // --- DB-gated full authorize (needs postgres via DATABASE_URL) -----------
    //
    // Ignored by default; run in CI with `cargo test -- --ignored`. Each test
    // seeds its own throwaway org, user, team, and session so it never collides.

    use axum_extra::extract::cookie::Cookie;

    async fn connect_test_state() -> AppState {
        use std::sync::Arc;

        use opaque_ke::ServerSetup;
        use rand::rngs::OsRng;
        use sqlx::postgres::PgPoolOptions;

        use crate::crypto::Crypto;
        use crate::opaque::Suite;

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

    /// Seeds an org, a user with an owner membership, a `dev` team, and a session.
    ///
    /// Returns `(subject_email_local, org_id, session_id)`.
    async fn seed_session(state: &AppState) -> (String, Uuid, Uuid) {
        let pool = &state.pool;
        let suffix = Uuid::new_v4();

        let org_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO orgs (name, slug) VALUES ('Test', $1) RETURNING id",
        )
        .bind(format!("org-{suffix}"))
        .fetch_one(pool)
        .await
        .expect("seed org");

        let email_local = format!("user-{suffix}");
        let user_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email, display_name) VALUES ($1, $2, 'Test User')
             RETURNING id",
        )
        .bind(format!("subject-{suffix}"))
        .bind(format!("{email_local}@example.test"))
        .fetch_one(pool)
        .await
        .expect("seed user");

        sqlx::query("INSERT INTO org_members (user_id, org_id, role) VALUES ($1, $2, 'owner')")
            .bind(user_id)
            .bind(org_id)
            .execute(pool)
            .await
            .expect("seed membership");

        let team_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO teams (org_id, name, slug) VALUES ($1, 'Developers', 'dev') RETURNING id",
        )
        .bind(org_id)
        .fetch_one(pool)
        .await
        .expect("seed team");
        sqlx::query("INSERT INTO team_members (user_id, team_id, role) VALUES ($1, $2, 'member')")
            .bind(user_id)
            .bind(team_id)
            .execute(pool)
            .await
            .expect("seed team membership");

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

        (email_local, org_id, session_id)
    }

    fn parts_with_session(session_id: Uuid) -> Parts {
        let jar =
            CookieJar::new().add(Cookie::new(session::SESSION_COOKIE, session_id.to_string()));
        let cookie = jar
            .get(session::SESSION_COOKIE)
            .map(|c| format!("{}={}", c.name(), c.value()))
            .expect("cookie present");
        parts_with("text/html", Some(&cookie))
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn authenticated_session_allows_with_the_injected_identity() {
        let state = connect_test_state().await;
        let (email_local, _org_id, session_id) = seed_session(&state).await;

        let parts = parts_with_session(session_id);
        let decision = authorize(&state, &parts, Uuid::new_v4()).await;
        let ProxyAuth::Allow { identity } = decision else {
            panic!("an authenticated, policy-passing session must be allowed");
        };

        // The upstream sees the authenticated user, email, and team group.
        let value = |name: &'static str| {
            identity
                .get(name)
                .expect("header present")
                .to_str()
                .expect("ascii header")
                .to_string()
        };
        assert_eq!(value(HEADER_WEBAUTH_USER), email_local);
        assert_eq!(
            value(HEADER_ELYSIUM_EMAIL),
            format!("{email_local}@example.test")
        );
        assert_eq!(value(HEADER_ELYSIUM_GROUPS), "dev");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn suspended_member_is_denied() {
        let state = connect_test_state().await;
        let (_email_local, org_id, session_id) = seed_session(&state).await;

        // Suspending the active-org membership must bar the caller from the service.
        sqlx::query(
            "UPDATE org_members SET suspended_at = now()
              WHERE org_id = $1 AND user_id = (SELECT active_user_id FROM sessions WHERE id = $2)",
        )
        .bind(org_id)
        .bind(session_id)
        .execute(&state.pool)
        .await
        .expect("suspend member");

        let parts = parts_with_session(session_id);
        let decision = authorize(&state, &parts, Uuid::new_v4()).await;
        assert!(
            matches!(decision, ProxyAuth::Deny(_)),
            "a suspended member must be denied",
        );
    }

    // --- DB-gated git transport-token authorize ------------------------------
    //
    // The HTTPS half of #75: a git smart-HTTP request authenticates with an
    // `ely_pat_` scoped token and lands as the token owner, no git login. Ignored
    // by default; run in CI with `cargo test -- --ignored`.

    /// Seeds an org, a user with an owner membership, and a `dev` team membership.
    ///
    /// Returns `(subject_email_local, org_id, user_id)`. Unlike [`seed_session`],
    /// no session is created: the token path authenticates without one.
    async fn seed_git_fixture(state: &AppState) -> (String, Uuid, Uuid) {
        let pool = &state.pool;
        let suffix = Uuid::new_v4();

        let org_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO orgs (name, slug) VALUES ('Test', $1) RETURNING id",
        )
        .bind(format!("org-{suffix}"))
        .fetch_one(pool)
        .await
        .expect("seed org");

        let email_local = format!("user-{suffix}");
        let user_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email, display_name) VALUES ($1, $2, 'Test User')
             RETURNING id",
        )
        .bind(format!("subject-{suffix}"))
        .bind(format!("{email_local}@example.test"))
        .fetch_one(pool)
        .await
        .expect("seed user");

        sqlx::query("INSERT INTO org_members (user_id, org_id, role) VALUES ($1, $2, 'owner')")
            .bind(user_id)
            .bind(org_id)
            .execute(pool)
            .await
            .expect("seed membership");

        let team_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO teams (org_id, name, slug) VALUES ($1, 'Developers', 'dev') RETURNING id",
        )
        .bind(org_id)
        .fetch_one(pool)
        .await
        .expect("seed team");
        sqlx::query("INSERT INTO team_members (user_id, team_id, role) VALUES ($1, $2, 'member')")
            .bind(user_id)
            .bind(team_id)
            .execute(pool)
            .await
            .expect("seed team membership");

        (email_local, org_id, user_id)
    }

    /// Inserts an org-scoped managed git integration, returning its id.
    async fn seed_git_integration(pool: &PgPool, org_id: Uuid) -> Uuid {
        // kind=1 (git), mode=1 (managed), owner_scope=1 (org), visibility=1 (org-wide).
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO integrations
                 (org_id, kind, mode, owner_scope, owner_id, name, visibility, config)
             VALUES ($1, 1, 1, 1, $1, 'git', 1, '{}'::jsonb) RETURNING id",
        )
        .bind(org_id)
        .fetch_one(pool)
        .await
        .expect("seed integration")
    }

    /// Lowercase-hex SHA-256, mirroring `transport::hash_token` (private to that
    /// module) so a seeded token row resolves through `resolve_access_token`.
    fn sha256_hex(secret: &str) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(secret.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Inserts a scoped access token, returning `(token_id, plaintext_secret)`.
    async fn insert_git_token(
        pool: &PgPool,
        user_id: Uuid,
        scopes: &[TokenScope],
        integration_id: Option<Uuid>,
    ) -> (Uuid, String) {
        let secret = format!("ely_pat_{}", Uuid::new_v4().simple());
        let db_scopes: Vec<i16> = scopes.iter().map(|scope| *scope as i16).collect();
        let token_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO access_tokens (user_id, name, token_hash, scopes, integration_id)
             VALUES ($1, 'git-ci', $2, $3, $4) RETURNING id",
        )
        .bind(user_id)
        .bind(sha256_hex(&secret))
        .bind(&db_scopes)
        .bind(integration_id)
        .fetch_one(pool)
        .await
        .expect("insert token");
        (token_id, secret)
    }

    /// The `Authorization: Basic` header a git client sends for `x:<token>`.
    fn git_basic(token: &str) -> String {
        format!("Basic {}", STANDARD.encode(format!("x:{token}")))
    }

    /// Polls until the token's `last_used_at` is stamped, tolerating the async spawn.
    async fn last_used_stamped(pool: &PgPool, token_id: Uuid) -> bool {
        for _ in 0..40 {
            let stamped = sqlx::query_scalar::<_, bool>(
                "SELECT last_used_at IS NOT NULL FROM access_tokens WHERE id = $1",
            )
            .bind(token_id)
            .fetch_one(pool)
            .await
            .expect("query last_used_at");
            if stamped {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        false
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn authenticated_git_token_allows_with_the_injected_identity_and_stamps_usage() {
        let state = connect_test_state().await;
        let (email_local, org_id, user_id) = seed_git_fixture(&state).await;
        let integration_id = seed_git_integration(&state.pool, org_id).await;
        let (token_id, secret) = insert_git_token(
            &state.pool,
            user_id,
            &[TokenScope::GitRead, TokenScope::GitWrite],
            None,
        )
        .await;

        let uri =
            format!("/services/{integration_id}/org/repo.git/info/refs?service=git-upload-pack");
        let parts = git_parts(&uri, Some(&git_basic(&secret)));
        let decision = authorize(&state, &parts, integration_id).await;
        let ProxyAuth::Allow { identity } = decision else {
            panic!("a live, scoped token must be allowed");
        };

        // The upstream sees the token owner, not a git login.
        let value = |name: &'static str| {
            identity
                .get(name)
                .expect("header present")
                .to_str()
                .expect("ascii header")
                .to_string()
        };
        assert_eq!(value(HEADER_WEBAUTH_USER), email_local);
        assert_eq!(
            value(HEADER_ELYSIUM_EMAIL),
            format!("{email_local}@example.test")
        );
        assert_eq!(value(HEADER_ELYSIUM_GROUPS), "dev");

        // Usage is stamped best-effort, out of band.
        assert!(
            last_used_stamped(&state.pool, token_id).await,
            "last_used_at must be stamped after a token authenticates",
        );
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn revoked_git_token_is_challenged_with_401() {
        let state = connect_test_state().await;
        let (_email_local, org_id, user_id) = seed_git_fixture(&state).await;
        let integration_id = seed_git_integration(&state.pool, org_id).await;
        let (token_id, secret) =
            insert_git_token(&state.pool, user_id, &[TokenScope::GitRead], None).await;

        // Revocation deletes the row, so resolution fails closed at once.
        sqlx::query("DELETE FROM access_tokens WHERE id = $1")
            .bind(token_id)
            .execute(&state.pool)
            .await
            .expect("revoke token");

        let uri =
            format!("/services/{integration_id}/org/repo.git/info/refs?service=git-upload-pack");
        let parts = git_parts(&uri, Some(&git_basic(&secret)));
        let decision = authorize(&state, &parts, integration_id).await;
        let ProxyAuth::Deny(response) = decision else {
            panic!("a revoked token must be denied");
        };
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Basic realm=\"Elysium Git\""
        );
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn suspended_owner_git_token_is_forbidden() {
        let state = connect_test_state().await;
        let (_email_local, org_id, user_id) = seed_git_fixture(&state).await;
        let integration_id = seed_git_integration(&state.pool, org_id).await;
        let (_token_id, secret) =
            insert_git_token(&state.pool, user_id, &[TokenScope::GitRead], None).await;

        // Suspending the owner's membership must bar the token from transporting.
        sqlx::query(
            "UPDATE org_members SET suspended_at = now() WHERE org_id = $1 AND user_id = $2",
        )
        .bind(org_id)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .expect("suspend member");

        let uri =
            format!("/services/{integration_id}/org/repo.git/info/refs?service=git-upload-pack");
        let parts = git_parts(&uri, Some(&git_basic(&secret)));
        let decision = authorize(&state, &parts, integration_id).await;
        let ProxyAuth::Deny(response) = decision else {
            panic!("a suspended owner's token must be denied");
        };
        // Authenticated but unauthorized: 403, not the 401 auth challenge.
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn git_read_token_pushing_is_forbidden() {
        let state = connect_test_state().await;
        let (_email_local, org_id, user_id) = seed_git_fixture(&state).await;
        let integration_id = seed_git_integration(&state.pool, org_id).await;
        // A read-only token: it may clone, never push.
        let (_token_id, secret) =
            insert_git_token(&state.pool, user_id, &[TokenScope::GitRead], None).await;

        let uri =
            format!("/services/{integration_id}/org/repo.git/info/refs?service=git-receive-pack");
        let parts = git_parts(&uri, Some(&git_basic(&secret)));
        let decision = authorize(&state, &parts, integration_id).await;
        let ProxyAuth::Deny(response) = decision else {
            panic!("a read-only token pushing must be denied");
        };
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn git_token_bound_to_another_integration_is_forbidden() {
        let state = connect_test_state().await;
        let (_email_local, org_id, user_id) = seed_git_fixture(&state).await;
        let target = seed_git_integration(&state.pool, org_id).await;
        let other = seed_git_integration(&state.pool, org_id).await;
        // The token is bound to `other`, but the request targets `target`.
        let (_token_id, secret) =
            insert_git_token(&state.pool, user_id, &[TokenScope::GitRead], Some(other)).await;

        let uri = format!("/services/{target}/org/repo.git/info/refs?service=git-upload-pack");
        let parts = git_parts(&uri, Some(&git_basic(&secret)));
        let decision = authorize(&state, &parts, target).await;
        let ProxyAuth::Deny(response) = decision else {
            panic!("a token bound to another integration must be denied");
        };
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
