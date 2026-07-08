//! HTTP routing and request handlers for the Elysium API.
//!
//! Infrastructure endpoints (`/` liveness and `/health`) answer as plain text and
//! JSON. The auth endpoints under `/api/v1/auth/*` run the WebAuthn passkey
//! ceremony (see [`crate::webauthn`]) and speak the standardized WebAuthn JSON.
//! The rest of `/api/v1/*` answers as binary protobuf, authenticated by the
//! session cookie the passkey flow sets. The browser never contacts Keycloak.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower_http::trace::TraceLayer;

use crate::proto::auth::v1::MeResponse;
use crate::proto::health::v1::{DatabaseHealthResponse, HelloResponse};
use crate::protobuf::Protobuf;
use crate::session::SessionUser;
use crate::state::AppState;
use crate::{sse, webauthn};

/// Builds the application router with shared state and middleware attached.
///
/// No CORS layer: the SPA reaches the API same-origin through the frontend proxy,
/// so every browser request is first-party.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(ping))
        .route("/health", get(health))
        .route("/api/v1/hello", get(hello))
        .route("/api/v1/db/health", get(database_health))
        .route("/api/v1/me", get(me))
        .route("/api/v1/events", get(sse::events))
        // Auth: the API is the WebAuthn Relying Party; the SPA drives the native
        // passkey ceremony against these endpoints (WebAuthn JSON).
        .route("/api/v1/auth/context", get(webauthn::context))
        .route(
            "/api/v1/auth/bootstrap/begin",
            post(webauthn::bootstrap_begin),
        )
        .route(
            "/api/v1/auth/bootstrap/finish",
            post(webauthn::bootstrap_finish),
        )
        .route(
            "/api/v1/auth/passkey/authenticate/begin",
            post(webauthn::authenticate_begin),
        )
        .route(
            "/api/v1/auth/passkey/authenticate/finish",
            post(webauthn::authenticate_finish),
        )
        .route("/api/v1/auth/logout", post(webauthn::logout))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Liveness ping. Returns `200 OK` with the plain text `pong` while running.
async fn ping() -> &'static str {
    "pong"
}

/// Server health as JSON, for load balancers and uptime monitors.
async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "elysium-backend",
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
