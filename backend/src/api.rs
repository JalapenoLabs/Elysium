//! HTTP routing and request handlers for the Elysium API.
//!
//! Two kinds of routes live here. Infrastructure endpoints (`/` and `/ok`
//! liveness, plus `/health`) answer as plain text and JSON, the shapes load
//! balancers and uptime monitors expect. The versioned `/api/v1/*` routes answer
//! as binary protobuf so the wire format matches the typed client end to end.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::AuthenticatedUser;
use crate::proto::auth::v1::MeResponse;
use crate::proto::health::v1::{DatabaseHealthResponse, HelloResponse};
use crate::protobuf::Protobuf;
use crate::state::AppState;

/// Builds the application router with shared state and middleware attached.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(ping))
        .route("/ok", get(ok))
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

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use crate::auth::Auth;

    /// Builds an [`AppState`] with a lazy (never-connected) pool, test auth
    /// config, redis configured, and rabbitmq absent, so both `services`
    /// branches are observable.
    fn test_state() -> AppState {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://tester:tester@localhost:1/elysia_test")
            .expect("a lazy pool never connects at build time");

        AppState {
            pool,
            auth: Auth::for_tests(),
            redis_url: Some("redis://:secret@localhost:1".to_string()),
            rabbitmq_url: None,
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
