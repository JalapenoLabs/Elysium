// Copyright © 2026 Jalapeno Labs

//! Per-client token-bucket rate limiting.
//!
//! Clients are keyed by IP. Because the API only ever sits behind nginx, the IP
//! comes from the `X-Forwarded-For` / `X-Real-IP` headers nginx sets, falling back
//! to the peer address. Never expose the API port directly: a client that can
//! reach it without nginx could forge those headers and dodge its bucket.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderValue, Response, StatusCode, header};
use governor::middleware::StateInformationMiddleware;
use tokio::task::JoinHandle;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::key_extractor::SmartIpKeyExtractor;
use tower_governor::{GovernorError, GovernorLayer};
use tracing::{Level, event};

/// Sustained requests per second allowed per client IP.
///
/// Fixed in code rather than read from the environment so every deployment enforces
/// the same limit. Ten per second is well above what a person clicking through the UI
/// produces, while still throttling scripted abuse.
const REQUESTS_PER_SECOND: u64 = 10;

/// Requests a client may spend at once before the sustained rate applies. Covers a
/// page load that fires several API calls in parallel.
const BURST_SIZE: u32 = 30;

/// Buckets for clients that stopped talking are dropped on this cadence so the
/// key map cannot grow without bound under a scan.
const STALE_KEY_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// The configured governor layer type, spelled once.
pub type Layer = GovernorLayer<SmartIpKeyExtractor, StateInformationMiddleware, Body>;

/// The rate limiting layer plus the background task that keeps it bounded.
pub struct RateLimiter {
    pub layer: Layer,
    /// Aborted at shutdown; the sweep has nothing to flush.
    pub sweeper: JoinHandle<()>,
}

/// Builds a limiter allowing [`BURST_SIZE`] immediate requests, refilling
/// [`REQUESTS_PER_SECOND`] each second.
pub fn build() -> RateLimiter {
    let config = GovernorConfigBuilder::default()
        .key_extractor(SmartIpKeyExtractor)
        .per_second(REQUESTS_PER_SECOND)
        .burst_size(BURST_SIZE)
        .use_headers()
        .finish()
        .expect("REQUESTS_PER_SECOND and BURST_SIZE are non-zero constants");
    let config = Arc::new(config);

    let limiter = Arc::clone(config.limiter());
    let sweeper = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(STALE_KEY_SWEEP_INTERVAL);
        loop {
            ticker.tick().await;
            limiter.retain_recent();
            event!(
                name: "rate_limit.sweep.complete",
                Level::DEBUG,
                rate_limit.tracked_clients = limiter.len(),
                "swept stale rate limit buckets",
            );
        }
    });

    let layer = GovernorLayer::new(config).error_handler(reject);

    RateLimiter { layer, sweeper }
}

/// Turns a limiter decision into a JSON error response.
fn reject(error: GovernorError) -> Response<Body> {
    let (status, message, wait_time, headers) = match error {
        GovernorError::TooManyRequests { wait_time, headers } => (
            StatusCode::TOO_MANY_REQUESTS,
            "Too many requests",
            Some(wait_time),
            headers,
        ),
        GovernorError::UnableToExtractKey => {
            event!(
                name: "rate_limit.key.missing",
                Level::ERROR,
                "request carried no client IP; nginx must set X-Real-IP or X-Forwarded-For",
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to identify client",
                None,
                None,
            )
        }
        GovernorError::Other { code, msg, headers } => {
            event!(
                name: "rate_limit.error.other",
                Level::ERROR,
                http.response.status_code = code.as_u16(),
                error.message = msg.as_deref().unwrap_or_default(),
                "rate limiter failed",
            );
            (code, "Rate limiter error", None, headers)
        }
    };

    let body = serde_json::json!({ "message": message }).to_string();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    if let Some(headers) = headers {
        *response.headers_mut() = headers;
    }
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    if let Some(seconds) = wait_time {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, seconds.into());
    }

    response
}
