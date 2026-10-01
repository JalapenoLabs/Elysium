// Copyright © 2026 Jalapeno Labs

//! HTTP routes. Every public path is mounted under `/api`, matching nginx's proxy prefix.
//! Health and build routes sit at the top level; resources are versioned under `/api/v1`.
//! `/internal` answers Kratos alone, on the compose network.

mod internal;
mod ok;
mod ping;
pub mod v1;
mod version;

use axum::Router;
use axum::routing::get;

use crate::errors::ApiError;
use crate::state::AppState;

/// Builds the `/api` router, and `/internal` for Kratos.
pub fn router(state: &AppState) -> Router<AppState> {
    let api = Router::new()
        .route("/ok", get(ok::handle))
        .route("/ping", get(ping::handle))
        .route("/version", get(version::handle))
        .nest("/v1", v1::router(state))
        // Its own fallback, so a path under /api that matches nothing is a JSON 404
        // rather than the web app the outer router falls back to.
        .fallback(not_found);

    Router::new()
        .nest("/api", api)
        .nest("/internal", internal::router(state))
}

async fn not_found() -> ApiError {
    ApiError::NotFound
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use serde_json::Value;
    use tower::ServiceExt as _;

    use super::*;
    use crate::test_support::{app_state, migrated_database};

    /// One path under every `/api/v1` router, and a few deeper ones. A router added later
    /// belongs here too.
    const WORKSPACE_PATHS: &[&str] = &[
        "/api/v1/events",
        "/api/v1/me",
        "/api/v1/action-items",
        "/api/v1/action-items/next",
        "/api/v1/action-items/0199a3c4-0000-7000-8000-000000000001/history",
        "/api/v1/changesets",
        "/api/v1/environment-variables",
        "/api/v1/github-credentials",
        "/api/v1/initiatives",
        "/api/v1/jira-credentials",
        "/api/v1/llms",
        "/api/v1/mail/accounts",
        "/api/v1/projects",
        "/api/v1/satellites",
        "/api/v1/storage-locations",
        "/api/v1/coding-sessions",
        "/api/v1/users",
        "/api/v1/workspace-settings",
    ];

    async fn send(
        state: &AppState,
        method: Method,
        path: &str,
        origin: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        let response = router(state)
            .with_state(state.clone())
            .oneshot(request.body(Body::empty()).expect("a request builds"))
            .await
            .expect("the router answers");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body reads");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// The auth layers are attached with `route_layer` over nested routers, so coverage is a
    /// property of how axum applies them. This pins it: no route answers without a session.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
    async fn every_workspace_route_refuses_a_request_without_a_session() {
        let (url, _connection) = migrated_database().await;
        let state = app_state(&url).await;

        for path in WORKSPACE_PATHS {
            let (status, body) = send(&state, Method::GET, path, None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}: {body}");
            assert_eq!(body["code"], "unauthenticated", "{path}");
        }

        let (status, body) = send(&state, Method::POST, "/api/v1/projects", None).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "an unsafe request needs an origin: {body}"
        );
        assert_eq!(body["code"], "cross_origin");

        let (status, body) = send(
            &state,
            Method::POST,
            "/api/v1/projects",
            Some("http://localhost:4000"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

        let (status, _) = send(&state, Method::GET, "/api/v1/auth/status", None).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "sign-in pages read this without a session"
        );

        for path in [
            "/internal/kratos/registration",
            "/internal/kratos/registered",
            "/internal/kratos/courier",
        ] {
            let (status, _) = send(&state, Method::POST, path, None).await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "{path} needs Kratos's hook key"
            );
        }
    }
}

#[cfg(test)]
mod fallback_tests {
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::*;
    use crate::web_app;

    // The web app is merged in as the outermost fallback. A nested router without a
    // fallback of its own inherits it, which would answer a mistyped API path with the
    // app's HTML and a 200. This builds the same shape `routes::router()` has.
    #[tokio::test]
    async fn an_unknown_api_path_is_a_404_even_beside_the_web_app() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(directory.path().join("index.html"), "<!doctype html>").expect("index.html");
        let api = Router::new()
            .route("/ok", get(|| async { "ok" }))
            .fallback(not_found);
        let app = Router::new()
            .nest("/api", api)
            .merge(web_app::router(directory.path()).expect("a valid build"));

        for path in ["/api/v1/no-such-thing", "/api/nope"] {
            let request = Request::get(path).body(Body::empty()).expect("request");
            let response = app.clone().oneshot(request).await.expect("infallible");
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");

            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("the body");
            let error: serde_json::Value = serde_json::from_slice(&body)
                .unwrap_or_else(|_| panic!("{path} answered with something other than JSON"));
            assert_eq!(error["message"], "resource not found", "{path}");
        }

        let request = Request::get("/projects")
            .body(Body::empty())
            .expect("request");
        let response = app.oneshot(request).await.expect("infallible");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
