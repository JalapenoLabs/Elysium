// Copyright © 2026 Jalapeno Labs

//! The web app's production build, served from the API's own origin.
//!
//! The published image carries the frontend's `vite build` output and names it with
//! `FRONTEND_DIR`, so one container answers both the app and `/api`. Without the
//! variable the API serves `/api` alone, which is how the development stack runs it:
//! there nginx sends every other path to the Vite dev server instead.
//!
//! Vite fingerprints everything under `assets/`, so those files never change under a
//! name and are cached for a year. Every other path is the app's own routing, answered
//! with `index.html` and revalidated on each load so a deploy reaches browsers at once.
//! A missing asset is a 404 rather than `index.html`, so a stale reference fails as a
//! missing file instead of HTML parsed as JavaScript.
//!
//! Responses are compressed with Brotli or gzip, whichever the browser accepts, so a
//! deployment is not slow to load whatever sits in front of it. `/api` is left alone:
//! its responses are small JSON, and its event stream must never be buffered.

use std::path::Path;

use anyhow::{Result, ensure};
use axum::Router;
use axum::http::{HeaderValue, Response, header};
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

/// A year, the longest lifetime caches honour. Asset names change with their content,
/// so a cached copy can never be stale.
const ASSET_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// Revalidate on every load; the `ETag` that `ServeDir` sends keeps an unchanged page cheap.
const PAGE_CACHE_CONTROL: &str = "no-cache";

/// Serves the build in `directory`: `assets/` as immutable files, and every other path
/// as the matching file in the build or else `index.html`.
///
/// Meant as the outermost router's fallback, so `/api` keeps every path it matches.
///
/// # Errors
/// Fails when `directory` has no `index.html`, since an image that names a build it
/// does not carry would otherwise answer every page with a 404.
pub fn router<State>(directory: &Path) -> Result<Router<State>>
where
    State: Clone + Send + Sync + 'static,
{
    let index = directory.join("index.html");
    ensure!(
        index.is_file(),
        "FRONTEND_DIR {} has no index.html",
        directory.display(),
    );

    // Only successful answers are cached: a 404 for an asset a deploy is about to add
    // must not stick for a year.
    let assets = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            |response: &Response<_>| {
                response
                    .status()
                    .is_success()
                    .then_some(HeaderValue::from_static(ASSET_CACHE_CONTROL))
            },
        ))
        .service(ServeDir::new(directory.join("assets")));

    let pages = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static(PAGE_CACHE_CONTROL),
        ))
        .service(ServeDir::new(directory).fallback(ServeFile::new(index)));

    Ok(Router::new()
        .nest_service("/assets", assets)
        .fallback_service(pages)
        .layer(CompressionLayer::new()))
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use tempfile::TempDir;
    use tower::ServiceExt;

    use super::*;

    const INDEX: &str = "<!doctype html><title>app</title>";
    const SCRIPT: &str = "console.log('app')";

    /// A build directory shaped like `vite build` output.
    fn build_directory() -> TempDir {
        let directory = tempfile::tempdir().expect("a temporary directory");
        std::fs::create_dir(directory.path().join("assets")).expect("assets directory");
        std::fs::write(directory.path().join("index.html"), INDEX).expect("index.html");
        std::fs::write(directory.path().join("favicon.svg"), "<svg/>").expect("favicon.svg");
        std::fs::write(directory.path().join("assets/index-3f2a.js"), SCRIPT).expect("the script");
        directory
    }

    async fn get(app: Router, path: &str) -> (StatusCode, Option<String>, String) {
        let request = Request::get(path).body(Body::empty()).expect("request");
        let response = app.oneshot(request).await.expect("infallible");

        let status = response.status();
        let cache_control = response
            .headers()
            .get(header::CACHE_CONTROL)
            .map(|value| value.to_str().expect("ascii").to_owned());
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body");
        let body = String::from_utf8(body.to_vec()).expect("utf-8");

        (status, cache_control, body)
    }

    #[test]
    fn a_directory_without_index_html_is_refused() {
        let directory = tempfile::tempdir().expect("a temporary directory");

        let error = router::<()>(directory.path()).expect_err("no index.html");

        assert!(error.to_string().contains("has no index.html"));
    }

    #[tokio::test]
    async fn an_app_route_answers_with_index_html_and_revalidates() {
        let directory = build_directory();
        let app = router(directory.path()).expect("a valid build");

        let (status, cache_control, body) = get(app, "/projects/0193/settings").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache_control.as_deref(), Some(PAGE_CACHE_CONTROL));
        assert_eq!(body, INDEX);
    }

    #[tokio::test]
    async fn a_file_at_the_build_root_is_served_as_itself() {
        let directory = build_directory();
        let app = router(directory.path()).expect("a valid build");

        let (status, cache_control, body) = get(app, "/favicon.svg").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache_control.as_deref(), Some(PAGE_CACHE_CONTROL));
        assert_eq!(body, "<svg/>");
    }

    #[tokio::test]
    async fn an_asset_is_cached_as_immutable() {
        let directory = build_directory();
        let app = router(directory.path()).expect("a valid build");

        let (status, cache_control, body) = get(app, "/assets/index-3f2a.js").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache_control.as_deref(), Some(ASSET_CACHE_CONTROL));
        assert_eq!(body, SCRIPT);
    }

    #[tokio::test]
    async fn assets_are_compressed_for_a_browser_that_accepts_it() {
        let directory = build_directory();
        // Long enough to clear the compressor's minimum size.
        let script = SCRIPT.repeat(64);
        std::fs::write(directory.path().join("assets/index-3f2a.js"), &script).expect("the script");
        let app = router::<()>(directory.path()).expect("a valid build");

        let request = Request::get("/assets/index-3f2a.js")
            .header(header::ACCEPT_ENCODING, "gzip")
            .body(Body::empty())
            .expect("request");
        let response = app.oneshot(request).await.expect("infallible");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_ENCODING),
            Some(&HeaderValue::from_static("gzip")),
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body");
        assert!(body.len() < script.len());
    }

    #[tokio::test]
    async fn a_missing_asset_is_a_404_that_is_never_cached() {
        let directory = build_directory();
        let app = router(directory.path()).expect("a valid build");

        let (status, cache_control, body) = get(app, "/assets/index-0000.js").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(cache_control, None);
        assert_ne!(body, INDEX);
    }
}
