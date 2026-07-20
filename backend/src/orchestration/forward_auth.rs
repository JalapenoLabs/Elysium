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

use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use uuid::Uuid;

use crate::oidc::tokens::{self, Subject};
use crate::orchestration::routing::ProxyAuth;
use crate::session;
use crate::state::AppState;
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

    // No live session: sign the user in (browser) or refuse (API/git).
    let Some(session_id) = session::session_id(&jar) else {
        return deny_unauthenticated(parts);
    };
    let principal = match session::principal(&state.pool, session_id).await {
        Ok(Some(principal)) => principal,
        Ok(None) => return deny_unauthenticated(parts),
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
}
