//! HTTP routing and request handlers for the Elysium API.
//!
//! Routes are intentionally tiny: a liveness check, a greeting, and a database
//! reachability probe that exercises the full API-to-Postgres path. Each returns
//! a protobuf message (see the `elysium.health.v1` contract) so the wire format
//! matches the typed client end to end.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::AuthenticatedUser;
use crate::proto::auth::v1::MeResponse;
use crate::proto::health::v1::{DatabaseHealthResponse, HealthResponse, HelloResponse};
use crate::protobuf::Protobuf;
use crate::state::AppState;

/// Builds the application router with shared state and middleware attached.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/hello", get(hello))
        .route("/api/v1/db/health", get(database_health))
        .route("/api/v1/me", get(me))
        .layer(TraceLayer::new_for_http())
        // Permissive CORS is fine for local development where the Vite dev
        // server calls the API from a different origin. Lock this down before
        // shipping anything to a shared environment.
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Liveness probe. Always returns `200 OK` while the process is running.
async fn health() -> Protobuf<HealthResponse> {
    Protobuf(HealthResponse {
        status: "ok".to_string(),
        service: "elysium-backend".to_string(),
    })
}

/// A friendly greeting, handy for confirming the frontend reached the API.
async fn hello() -> Protobuf<HelloResponse> {
    Protobuf(HelloResponse {
        message: "Hello from the Elysium Rust API 🦀".to_string(),
    })
}

/// Returns the authenticated principal, projected from the validated token.
///
/// Protected: the [`AuthenticatedUser`] extractor rejects the request with `401`
/// before this runs unless a valid Keycloak bearer token is present.
async fn me(AuthenticatedUser(claims): AuthenticatedUser) -> Protobuf<MeResponse> {
    Protobuf(MeResponse {
        subject: claims.sub,
        email: claims.email,
        username: claims.preferred_username,
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
