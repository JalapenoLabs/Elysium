// Copyright © 2026 Jalapeno Labs

//! Hardening response headers, equivalent to Express's `helmet` defaults.
//!
//! Every response carries the same headers except the Content Security Policy, which
//! depends on what is answering. `/api` only ever returns JSON or plain text, so its
//! policy forbids everything: a browser that somehow renders an API response directly
//! must not execute or embed anything from it. Every other path is the web app, whose
//! policy allows exactly what the app loads, all from its own origin.

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

/// The policy for `/api`: nothing may load, run, or embed the response.
const API_CONTENT_SECURITY_POLICY: HeaderValue =
    HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'");

/// The policy for the web app. Everything comes from its own origin, and no script runs
/// inline: the pre-paint theme script is `public/theme.js` for that reason.
///
/// - `style-src 'unsafe-inline'`: React Aria and `HeroUI` position overlays and animate
///   through `<style>` elements and style attributes they write at runtime.
/// - `img-src data: blob:`: a cover chosen for upload is previewed from a blob URL
///   before it is sent, and icon libraries inline small images as data URLs.
/// - `font-src data:`: bundled CSS may inline small fonts.
const WEB_APP_CONTENT_SECURITY_POLICY: HeaderValue = HeaderValue::from_static(concat!(
    "default-src 'self'; ",
    "script-src 'self'; ",
    "style-src 'self' 'unsafe-inline'; ",
    "img-src 'self' data: blob:; ",
    "font-src 'self' data:; ",
    "connect-src 'self'; ",
    "object-src 'none'; ",
    "base-uri 'self'; ",
    "form-action 'self'; ",
    "frame-ancestors 'none'",
));

/// Headers applied to every response, overriding anything a handler set.
const SECURITY_HEADERS: [(HeaderName, HeaderValue); 11] = [
    (
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    ),
    (
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    ),
    (
        HeaderName::from_static("origin-agent-cluster"),
        HeaderValue::from_static("?1"),
    ),
    // Not part of helmet's defaults. The web app uses none of these, so a script that
    // somehow runs in it cannot ask for them either.
    (
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), geolocation=(), microphone=()"),
    ),
    (
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    ),
    // Two years, matching helmet; harmless over plain HTTP because browsers ignore it there.
    (
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=63072000; includeSubDomains"),
    ),
    (
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    ),
    (
        header::X_DNS_PREFETCH_CONTROL,
        HeaderValue::from_static("off"),
    ),
    (
        HeaderName::from_static("x-download-options"),
        HeaderValue::from_static("noopen"),
    ),
    (header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")),
    (
        HeaderName::from_static("x-permitted-cross-domain-policies"),
        HeaderValue::from_static("none"),
    ),
];

/// Axum middleware that stamps [`SECURITY_HEADERS`] and the matching Content Security
/// Policy onto the response.
pub async fn apply(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let content_security_policy = if path == "/api" || path.starts_with("/api/") {
        API_CONTENT_SECURITY_POLICY
    } else {
        WEB_APP_CONTENT_SECURITY_POLICY
    };

    let mut response = next.run(request).await;

    let headers = response.headers_mut();
    for (name, value) in SECURITY_HEADERS {
        headers.insert(name, value);
    }
    headers.insert(header::CONTENT_SECURITY_POLICY, content_security_policy);

    response
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::routing::get;
    use tower::ServiceExt;

    use super::*;

    async fn content_security_policy(path: &str) -> HeaderValue {
        let app = Router::new()
            .fallback(get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(apply));
        let request = Request::get(path).body(Body::empty()).expect("request");
        let response = app.oneshot(request).await.expect("infallible");

        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("every response has a policy")
            .clone()
    }

    #[tokio::test]
    async fn the_api_forbids_everything() {
        assert_eq!(
            content_security_policy("/api").await,
            API_CONTENT_SECURITY_POLICY
        );
        assert_eq!(
            content_security_policy("/api/v1/projects").await,
            API_CONTENT_SECURITY_POLICY
        );
    }

    #[tokio::test]
    async fn the_web_app_gets_its_own_policy() {
        for path in ["/", "/projects", "/assets/index-3f2a.js", "/apiary"] {
            assert_eq!(
                content_security_policy(path).await,
                WEB_APP_CONTENT_SECURITY_POLICY,
                "{path}",
            );
        }
    }

    #[test]
    fn the_web_app_never_runs_inline_scripts() {
        let policy = WEB_APP_CONTENT_SECURITY_POLICY;
        let script_source = policy
            .to_str()
            .expect("ascii")
            .split("; ")
            .find(|directive| directive.starts_with("script-src"))
            .expect("a script-src directive");

        assert!(!script_source.contains("unsafe-inline"));
        assert!(!script_source.contains("unsafe-eval"));
    }
}
