// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/events`: the server-sent event stream every frontend keeps open.
//!
//! The first message is `hello`; after it, every [`ServerEvent`] published on the bus
//! arrives as an unnamed SSE message. A client that falls more than the bus capacity
//! behind receives `resync` in place of what it missed. The stream ends when the
//! process begins shutting down, so open streams never hold up the graceful drain.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderName, HeaderValue};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::StreamExt as _;
use futures_util::stream;
use tokio::sync::broadcast::error::RecvError;
use tracing::{Level, event};

use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Comment frames sent on an idle stream. Shorter than every proxy read timeout in
/// front of the API, so an idle connection is never mistaken for a dead one.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);

pub async fn handle(State(state): State<AppState>) -> impl IntoResponse {
    let receiver = state.events.subscribe();

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

    let messages = hello
        .chain(published)
        .map(|payload| Ok::<Event, Infallible>(Event::default().data(&*payload)))
        .take_until(state.shutdown.cancelled_owned());

    (
        // Tells nginx not to buffer the stream, which would hold events back.
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        Sse::new(messages).keep_alive(KeepAlive::new().interval(KEEP_ALIVE_INTERVAL)),
    )
}
