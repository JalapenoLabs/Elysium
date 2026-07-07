//! HTTP routing and request handlers for the Elysia API.
//!
//! Routes are intentionally tiny: a liveness check, a greeting, and a database
//! reachability probe that exercises the full API-to-Postgres path.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// Builds the application router with shared state and middleware attached.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/hello", get(hello))
        .route("/api/v1/db/health", get(database_health))
        .layer(TraceLayer::new_for_http())
        // Permissive CORS is fine for local development where the Vite dev
        // server calls the API from a different origin. Lock this down before
        // shipping anything to a shared environment.
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Liveness probe. Always returns `200 OK` while the process is running.
async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "elysia-backend",
    }))
}

/// A friendly greeting, handy for confirming the frontend reached the API.
async fn hello() -> Json<Value> {
    Json(json!({
        "message": "Hello from the Elysia Rust API 🦀",
    }))
}

/// Verifies Postgres connectivity by running a trivial query through the pool.
///
/// Returns `503 Service Unavailable` when the database cannot be reached, so
/// callers can distinguish a live API from a live-and-connected one.
async fn database_health(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
    {
        Ok(_) => Ok(Json(json!({ "database": "reachable" }))),
        Err(error) => {
            tracing::error!(error = %error, "database health check failed");
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "database": "unreachable",
                    "error": error.to_string(),
                })),
            ))
        }
    }
}
