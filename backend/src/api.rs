//! HTTP routing and request handlers for the Elysium API.
//!
//! Three kinds of routes live here. Infrastructure endpoints (`/` liveness and
//! `/health`) answer as plain text and JSON. The BFF auth routes under `/auth/*`
//! are browser redirects into and out of Keycloak (see [`crate::oidc`]). The
//! versioned `/api/v1/*` routes answer as binary protobuf and are authenticated by
//! the session cookie the auth flow sets.

use axum::extract::State;
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::proto::auth::v1::MeResponse;
use crate::proto::health::v1::{DatabaseHealthResponse, HelloResponse};
use crate::protobuf::Protobuf;
use crate::session::SessionUser;
use crate::state::AppState;
use crate::{oidc, sse};

/// Origin allowed to make credentialed (cookie-bearing) requests to the API.
const DEFAULT_CORS_ORIGIN: &str = "http://localhost:5173";

/// Builds the application router with shared state and middleware attached.
pub fn router(state: AppState) -> Router {
    // Credentialed CORS: the SPA sends the session cookie cross-origin, which
    // forbids a wildcard origin, so we echo one specific, configured origin.
    let allowed_origin =
        std::env::var("CORS_ALLOWED_ORIGIN").unwrap_or_else(|_| DEFAULT_CORS_ORIGIN.to_string());
    let cors = CorsLayer::new()
        .allow_origin(
            allowed_origin
                .parse::<HeaderValue>()
                .expect("CORS_ALLOWED_ORIGIN must be a valid origin"),
        )
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE]);

    Router::new()
        .route("/", get(ping))
        .route("/health", get(health))
        // BFF auth: browser redirects into and out of Keycloak.
        .route("/auth/login", get(oidc::login))
        .route("/auth/callback", get(oidc::callback))
        .route("/auth/account", get(oidc::account))
        // Versioned API (protobuf), authenticated by the session cookie.
        .route("/api/v1/hello", get(hello))
        .route("/api/v1/db/health", get(database_health))
        .route("/api/v1/me", get(me))
        .route("/api/v1/auth/logout", post(oidc::logout))
        .route("/api/v1/events", get(sse::events))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
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
