//! HTTP routing and request handlers for the Elysium API.
//!
//! Infrastructure endpoints (`/` and `/ok` liveness, plus `/health`) answer as
//! plain text and JSON, the shapes load balancers and uptime monitors expect. The
//! auth endpoints under `/api/v1/auth/*` run the WebAuthn passkey ceremony (see
//! [`crate::webauthn`]) and speak standardized WebAuthn JSON; the rest of
//! `/api/v1/*` answers as binary protobuf, authenticated by the session cookie the
//! sign-in flow sets. The API brokers all authentication.

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower_http::trace::TraceLayer;

use crate::proto::auth::v1::MeResponse;
use crate::proto::health::v1::{DatabaseHealthResponse, HelloResponse};
use crate::protobuf::Protobuf;
use crate::session::SessionUser;
use crate::state::AppState;
use crate::{
    accounts, audit, bootstrap, context, credentials, grants, invites, members, orgs, password,
    policies, profile, recovery, security, sse, teams, webauthn,
};

/// Builds the application router with shared state and middleware attached.
///
/// No CORS layer: the SPA reaches the API same-origin through the frontend proxy,
/// so every browser request is first-party.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(ping))
        .route("/ok", get(ok))
        .route("/health", get(health))
        .route("/api/v1/hello", get(hello))
        // Public instance identity, for a client validating a server before login.
        .route("/api/v1/instance", get(bootstrap::instance))
        .route("/api/v1/db/health", get(database_health))
        .route("/api/v1/me", get(me))
        // Full profile: names, bio, derived presence, and the manual status override.
        .route(
            "/api/v1/users/me",
            get(profile::get_me).patch(profile::update_me),
        )
        .route("/api/v1/users/me/status", put(profile::set_status))
        // Security Center: credentials, plus one combined sessions + sign-ins read.
        .route("/api/v1/me/credentials", get(security::credentials))
        .route(
            "/api/v1/me/security-activity",
            get(security::security_activity),
        )
        .route(
            "/api/v1/me/recovery-codes",
            post(security::regenerate_recovery_codes),
        )
        // Self-service TOTP enrollment (Security Center, and the require-2FA gate).
        .route("/api/v1/me/totp/begin", post(security::totp_begin))
        .route("/api/v1/me/totp/confirm", post(security::totp_confirm))
        // Session revocation: sign one device out, or all others.
        .route(
            "/api/v1/me/sessions/revoke-others",
            post(security::revoke_other_sessions),
        )
        .route(
            "/api/v1/me/sessions/{session_id}",
            delete(security::revoke_session),
        )
        // Session bootstrap: one aggregated read the client gates itself on.
        .route("/api/v1/context", get(context::context))
        .route("/api/v1/events", get(sse::events))
        // Auth: the API brokers every method. First-run claim (bootstrap) and
        // method discovery are public; the SPA drives each ceremony against these.
        .route("/api/v1/auth/context", get(bootstrap::context))
        .route("/api/v1/auth/methods", post(password::methods))
        .route("/api/v1/auth/bootstrap/begin", post(bootstrap::begin))
        .route("/api/v1/auth/bootstrap/finish", post(bootstrap::finish))
        .route(
            "/api/v1/auth/passkey/authenticate/begin",
            post(webauthn::authenticate_begin),
        )
        .route(
            "/api/v1/auth/passkey/authenticate/finish",
            post(webauthn::authenticate_finish),
        )
        .route(
            "/api/v1/auth/password/authenticate/begin",
            post(password::authenticate_begin),
        )
        .route(
            "/api/v1/auth/password/authenticate/finish",
            post(password::authenticate_finish),
        )
        .route("/api/v1/auth/totp/verify", post(password::verify_totp))
        .route(
            "/api/v1/auth/recovery/authenticate",
            post(recovery::authenticate),
        )
        // Invites: mint (authed), preview (public), and method-aware acceptance.
        .route("/api/v1/auth/invites", post(invites::mint))
        .route("/api/v1/auth/invites/{token}", get(invites::preview))
        .route(
            "/api/v1/auth/invites/{token}/accept/begin",
            post(invites::accept_begin),
        )
        .route(
            "/api/v1/auth/invites/{token}/accept/join",
            post(invites::accept_join),
        )
        .route(
            "/api/v1/auth/invites/accept/finish",
            post(invites::accept_finish),
        )
        // Orgs: create additional orgs, and edit name/logo (fans out org.updated).
        .route("/api/v1/orgs", post(orgs::create))
        .route("/api/v1/orgs/{id}", patch(orgs::update))
        .route(
            "/api/v1/orgs/{id}/transfer-ownership",
            post(orgs::transfer_ownership),
        )
        // Org auth policy (admin+): the security control plane over sign-in methods,
        // mandatory 2FA, and session lifetime.
        .route(
            "/api/v1/orgs/{id}/auth-policy",
            get(policies::get_policy).patch(policies::update_policy),
        )
        // Delegated admin grants (owner/admin only): list, and set a member's set.
        .route("/api/v1/orgs/{id}/grants", get(grants::list))
        .route(
            "/api/v1/orgs/{id}/members/{user_id}/grants",
            put(grants::set),
        )
        // Org-wide audit log (admin+): who did what, to whom, when.
        .route("/api/v1/orgs/{id}/audit-log", get(audit::list))
        // Org member management (admin+): view the roster, change a role, remove.
        .route("/api/v1/orgs/{id}/members", get(members::list))
        .route(
            "/api/v1/orgs/{id}/members/{user_id}",
            patch(members::change_role).delete(members::remove),
        )
        .route(
            "/api/v1/orgs/{id}/members/{user_id}/revoke-sessions",
            post(members::revoke_sessions),
        )
        .route(
            "/api/v1/orgs/{id}/members/{user_id}/suspend",
            post(members::suspend),
        )
        .route(
            "/api/v1/orgs/{id}/members/{user_id}/reactivate",
            post(members::reactivate),
        )
        // Teams: org-scoped collection, then team-scoped item and roster.
        .route(
            "/api/v1/orgs/{id}/teams",
            get(teams::list).post(teams::create),
        )
        .route(
            "/api/v1/teams/{team_id}",
            patch(teams::rename).delete(teams::delete),
        )
        .route(
            "/api/v1/teams/{team_id}/members",
            get(teams::members).post(teams::add_member),
        )
        .route(
            "/api/v1/teams/{team_id}/members/{user_id}",
            delete(teams::remove_member),
        )
        // Session accounts: the profile + org switchers.
        .route("/api/v1/session/accounts", get(accounts::list))
        .route("/api/v1/session/switch", post(accounts::switch))
        .route("/api/v1/session/switch-org", post(accounts::switch_org))
        .route("/api/v1/session/signout", post(accounts::signout))
        // Authenticated add-credential (Security Center + post-recovery re-enroll).
        .route(
            "/api/v1/credentials/enroll/begin",
            post(credentials::enroll_begin),
        )
        .route(
            "/api/v1/credentials/enroll/finish",
            post(credentials::enroll_finish),
        )
        .route("/api/v1/auth/logout", post(webauthn::logout))
        .layer(middleware::from_fn(security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Adds baseline security headers to every response.
///
/// Defense in depth for a self-hosted deployment: responses never contain HTML the
/// browser should sniff or frame, so `nosniff` and `DENY` are always safe, and the
/// API should never leak a referrer. TLS/HSTS and the app-origin CSP remain the
/// operator's responsibility (see `CLAUDE.md`).
async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    response
}

/// Liveness ping. Returns `200 OK` with the plain text `pong` while running.
async fn ping() -> &'static str {
    "pong"
}

/// Liveness check. Returns `200 OK` with the plain text `ok` while running.
async fn ok() -> &'static str {
    "ok"
}

/// Server health as JSON, for load balancers and uptime monitors.
///
/// The `services` block reports which optional backing services are configured
/// (wired through the environment), not their live connectivity; Postgres
/// connectivity has its own probe at `/api/v1/db/health`.
async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "elysium-backend",
        "services": {
            "redis": state.redis_url.is_some(),
            "rabbitmq": state.rabbitmq_url.is_some(),
        },
    }))
}

/// A friendly greeting, handy for confirming the frontend reached the API.
async fn hello() -> Protobuf<HelloResponse> {
    Protobuf(HelloResponse {
        message: "Hello from the Elysium Rust API 🦀".to_string(),
    })
}

/// Returns the authenticated principal for the session cookie.
///
/// Protected: the [`SessionUser`] extractor rejects the request with `401` before
/// this runs unless a valid session cookie is present. The SPA also uses this as
/// its session probe (200 = signed in, 401 = anonymous).
async fn me(SessionUser(principal): SessionUser) -> Protobuf<MeResponse> {
    Protobuf(MeResponse {
        subject: principal.sub,
        email: principal.email,
        username: principal.username,
    })
}

/// Verifies Postgres connectivity by running a trivial query through the pool.
///
/// Returns `503 Service Unavailable` when the database cannot be reached, so
/// callers can distinguish a live API from a live-and-connected one.
async fn database_health(State(state): State<AppState>) -> Response {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
    {
        Ok(_) => Protobuf(DatabaseHealthResponse {
            reachable: true,
            error: String::new(),
        })
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "database health check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Protobuf(DatabaseHealthResponse {
                    reachable: false,
                    error: error.to_string(),
                }),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use opaque_ke::ServerSetup;
    use rand::rngs::OsRng;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use crate::crypto::Crypto;
    use crate::opaque::Suite;
    use crate::sse::SseRegistry;

    /// Builds an [`AppState`] with a lazy (never-connected) pool, a throwaway
    /// OPAQUE setup and encryption key, redis configured, and rabbitmq absent, so
    /// both `services` branches are observable.
    fn test_state() -> AppState {
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
            redis_url: Some("redis://:secret@localhost:1".to_string()),
            redis: None,
            http: reqwest::Client::new(),
            geoip_endpoint: None,
            rabbitmq_url: None,
            app_origin: "http://localhost:5173".to_string(),
        }
    }

    async fn get(path: &str) -> (StatusCode, Vec<u8>) {
        let request = Request::get(path)
            .body(Body::empty())
            .expect("a valid test request");
        let response = router(test_state())
            .oneshot(request)
            .await
            .expect("the router must produce a response");

        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("a readable response body")
            .to_bytes()
            .to_vec();

        (status, body)
    }

    #[tokio::test]
    async fn ping_answers_pong_at_the_root() {
        let (status, body) = get("/").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"pong");
    }

    #[tokio::test]
    async fn ok_answers_plain_text_ok() {
        let (status, body) = get("/ok").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"ok");
    }

    #[tokio::test]
    async fn health_reports_status_and_configured_services() {
        let (status, body) = get("/health").await;

        assert_eq!(status, StatusCode::OK);

        let json: Value = serde_json::from_slice(&body).expect("valid JSON from /health");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["service"], "elysium-backend");
        // The test state configures redis but not rabbitmq; the flags must
        // mirror that without exposing the connection strings themselves.
        assert_eq!(json["services"]["redis"], true);
        assert_eq!(json["services"]["rabbitmq"], false);
    }
}
