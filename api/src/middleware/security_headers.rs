// Copyright © 2026 Jalapeno Labs

//! Hardening response headers, equivalent to Express's `helmet` defaults.
//!
//! The API only ever returns JSON or plain text, so the Content Security Policy
//! forbids everything: a browser that somehow renders an API response directly
//! must not execute or embed anything from it.

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

/// Headers applied to every response, overriding anything a handler set.
const SECURITY_HEADERS: [(HeaderName, HeaderValue); 11] = [
    (
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    ),
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

/// Axum middleware that stamps [`SECURITY_HEADERS`] onto the response.
pub async fn apply(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;

    let headers = response.headers_mut();
    for (name, value) in SECURITY_HEADERS {
        headers.insert(name, value);
    }

    response
}
