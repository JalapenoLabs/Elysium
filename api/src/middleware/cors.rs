// Copyright © 2026 Jalapeno Labs

//! Cross-origin policy.
//!
//! Behind nginx the frontend and API share one origin, so the allow-list is
//! normally empty and every cross-origin browser request is refused. Populating
//! `CORS_ALLOWED_ORIGINS` opens the API to exactly those origins and nothing more.

use std::time::Duration;

use axum::http::{HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// How long browsers may cache a preflight answer. One hour keeps preflight
/// traffic negligible while still letting origin changes roll out same-day.
const PREFLIGHT_MAX_AGE: Duration = Duration::from_hours(1);

/// Builds the CORS layer for the given explicit origin allow-list.
pub fn layer(allowed_origins: Vec<HeaderValue>) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::ACCEPT, header::AUTHORIZATION, header::CONTENT_TYPE])
        .expose_headers([header::RETRY_AFTER])
        .max_age(PREFLIGHT_MAX_AGE)
}
