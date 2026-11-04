// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/events`: the server-sent event stream every frontend keeps open.
//!
//! The first message is `hello`; after it, every [`ServerEvent`] any process publishes
//! arrives as an unnamed SSE message. A client that falls more than the bus capacity
//! behind, or that may have missed events while this process was resubscribing to Redis,
//! receives `resync` in place of what it missed. The stream ends when the process begins
//! shutting down, so open streams never hold up the graceful drain.
//!
//! A stream opens only while this process holds its Redis subscription, so `hello` always
//! means "subscribed": while the subscription is down the route answers `503`, and the
//! browser retries with backoff (`docs/realtime.md`).
//!
//! It also ends when its session does: every [`ACCESS_CHECK_INTERVAL`] it checks the
//! session and the person again, the way a request would, so a person who is signed out
//! or disabled stops receiving events. The browser then reconnects and is refused.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::StreamExt as _;
use futures_util::{future, stream};
use tokio::sync::broadcast::error::RecvError;
use tracing::{Level, event};

use crate::auth::middleware::revoked;
use crate::errors::ApiError;
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Comment frames sent on an idle stream. Shorter than every proxy read timeout in
/// front of the API, so an idle connection is never mistaken for a dead one.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// How often an open stream checks that its session and person may still receive events.
/// The session cache (`crate::auth::sessions`) answers most checks, so this costs a Redis
/// read and a row lookup.
const ACCESS_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Opens the stream.
///
/// # Errors
/// `503` while this process is reconnecting its Redis subscription.
pub async fn handle(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    // Subscribed before `hello` is sent, so whatever the client reads after `hello` is
    // covered by the stream: an event published after the read reaches it. The local
    // receiver comes first, so a subscription lost between the two checks still reaches
    // this stream as the `resync` that follows its return.
    let receiver = state.events.subscribe();
    if !state.events.is_subscribed() {
        event!(
            name: "realtime.stream.refused",
            Level::DEBUG,
            "refused an event stream while the events subscription is down",
        );
        return Err(ApiError::Unavailable(
            "live updates are reconnecting; the page retries on its own",
        ));
    }

    let hello = stream::once(async { ServerEvent::Hello.to_json() });
    let published = stream::unfold(receiver, |mut receiver| async move {
        match receiver.recv().await {
            Ok(payload) => Some((payload, receiver)),
            Err(RecvError::Lagged(skipped)) => {
                event!(
                    name: "realtime.client.lagged",
                    Level::WARN,
                    realtime.skipped = skipped,
                    "a client fell behind the event bus and was told to resync",
                );
                Some((ServerEvent::Resync.to_json(), receiver))
            }
            Err(RecvError::Closed) => None,
        }
    });

    let shutdown = Box::pin(state.shutdown.clone().cancelled_owned());
    let access = Box::pin(revoked(state, headers, ACCESS_CHECK_INTERVAL));
    let messages = hello
        .chain(published)
        .map(|payload| Ok::<Event, Infallible>(Event::default().data(&*payload)))
        .take_until(future::select(shutdown, access));

    Ok((
        // Tells nginx not to buffer the stream, which would hold events back.
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        Sse::new(messages).keep_alive(KeepAlive::new().interval(KEEP_ALIVE_INTERVAL)),
    ))
}
