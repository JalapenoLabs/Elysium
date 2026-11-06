// Copyright © 2026 Jalapeno Labs

//! The deadline on producing a response, with the one route that has a deadline of its own.
//!
//! Attaching a file to a page answers only once the storage provider has the file, which for one of
//! `crate::page_attachments::MAX_ATTACHMENT_BYTES` on a slow provider can outlast the deadline every
//! other request is held to. The rest of that request is not the risk a deadline guards against:
//! nginx holds the file until it has arrived whole, so a slow client never reaches the API, and the
//! transfer to the provider ends by itself once it stops moving (`crate::storage::transfer`).
//!
//! The rule lives here, rather than on the route, because a deadline has to wrap everything a
//! request does: this layer is outside the session checks, which a layer on a route is inside. It
//! still runs after routing, so it names the route by the pattern that matched, never by a path.

use std::time::Duration;

use axum::extract::{MatchedPath, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The route that uploads a page's attachment, as the router names it.
const ATTACHMENT_UPLOAD_ROUTE: &str = "/api/v1/pages/{id}/attachments";

/// How long attaching a file may take: its largest size at about 80 KB a second, far below anything
/// a storage provider serves, so this catches only what never finishes. `proxy_read_timeout` on
/// its location in `nginx/locations.conf` matches it.
const ATTACHMENT_UPLOAD_TIMEOUT: Duration = Duration::from_mins(10);

/// Answers `408` when `deadline` passes before the response starts. A response that has started, such
/// as the event stream or a file being served, runs on: only producing it is bounded.
pub async fn enforce(State(deadline): State<Duration>, request: Request, next: Next) -> Response {
    let is_upload = request
        .extensions()
        .get::<MatchedPath>()
        .is_some_and(|route| route.as_str() == ATTACHMENT_UPLOAD_ROUTE);
    let deadline = if is_upload {
        ATTACHMENT_UPLOAD_TIMEOUT
    } else {
        deadline
    };

    match tokio::time::timeout(deadline, next.run(request)).await {
        Ok(response) => response,
        Err(_elapsed) => StatusCode::REQUEST_TIMEOUT.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::middleware::from_fn_with_state;
    use axum::routing::{get, post};
    use tower::ServiceExt as _;

    use super::*;

    async fn slow() -> StatusCode {
        tokio::time::sleep(Duration::from_millis(50)).await;
        StatusCode::OK
    }

    /// Laid out as the API's router is, with the layer put on from outside, as `build_router` does.
    fn router() -> Router {
        let pages = Router::new()
            .route("/", get(slow))
            .route("/{id}/attachments", post(slow));
        let api = Router::new().nest("/v1", Router::new().nest("/pages", pages));
        Router::new()
            .nest("/api", api)
            .layer(from_fn_with_state(Duration::from_millis(1), enforce))
    }

    async fn status_of(method: &str, path: &str) -> StatusCode {
        let request = HttpRequest::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .expect("a request builds");
        router()
            .oneshot(request)
            .await
            .expect("the router answers")
            .status()
    }

    #[tokio::test]
    async fn the_deadline_answers_for_every_other_route() {
        assert_eq!(
            status_of("GET", "/api/v1/pages").await,
            StatusCode::REQUEST_TIMEOUT
        );
    }

    /// Pins what the rule rests on: a layer on the outside of the router sees the pattern that
    /// matched, through every `nest`, or the upload would be cut off at the ordinary deadline.
    #[tokio::test]
    async fn an_upload_has_a_deadline_of_its_own() {
        assert_eq!(
            status_of("POST", "/api/v1/pages/one/attachments").await,
            StatusCode::OK
        );
    }
}
