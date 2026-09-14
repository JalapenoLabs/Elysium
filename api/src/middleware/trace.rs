// Copyright © 2026 Jalapeno Labs

//! Request tracing spans and request id propagation.

use axum::body::Body;
use axum::http::Request;
use tower_http::classify::{ServerErrorsAsFailures, SharedClassifier};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing::Span;

/// Assigns a UUID request id unless the client already supplied one.
pub fn set_request_id_layer() -> SetRequestIdLayer<MakeRequestUuid> {
    SetRequestIdLayer::x_request_id(MakeRequestUuid)
}

/// Echoes the request id back on the response so clients can quote it.
pub fn propagate_request_id_layer() -> PropagateRequestIdLayer {
    PropagateRequestIdLayer::x_request_id()
}

/// Span builder signature `TraceLayer` expects; a plain fn pointer keeps the layer type nameable.
type MakeSpan = fn(&Request<Body>) -> Span;

/// One span per request, carrying method, path, and request id.
pub fn layer() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>, MakeSpan> {
    TraceLayer::new_for_http().make_span_with(make_span as MakeSpan)
}

fn make_span(request: &Request<Body>) -> Span {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    tracing::info_span!(
        "http.request",
        http.request.method = %request.method(),
        url.path = request.uri().path(),
        http.request.id = request_id,
    )
}
