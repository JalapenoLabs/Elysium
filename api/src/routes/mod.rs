// Copyright © 2026 Jalapeno Labs

//! HTTP routes. Every path is mounted under `/api`, matching nginx's proxy prefix.
//! Health and build routes sit at the top level; resources are versioned under `/api/v1`.

mod ok;
mod ping;
pub mod v1;
mod version;

use axum::Router;
use axum::routing::get;

use crate::errors::ApiError;
use crate::state::AppState;

/// Builds the `/api` router.
pub fn router() -> Router<AppState> {
    let api = Router::new()
        .route("/ok", get(ok::handle))
        .route("/ping", get(ping::handle))
        .route("/version", get(version::handle))
        .nest("/v1", v1::router())
        // Its own fallback, so a path under /api that matches nothing is a JSON 404
        // rather than the web app the outer router falls back to.
        .fallback(not_found);

    Router::new().nest("/api", api)
}

async fn not_found() -> ApiError {
    ApiError::NotFound
}

#[cfg(test)]
mod tests {
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
            assert!(
                !body.starts_with(b"<!doctype"),
                "{path} answered with the web app"
            );
        }

        let request = Request::get("/projects")
            .body(Body::empty())
            .expect("request");
        let response = app.oneshot(request).await.expect("infallible");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
