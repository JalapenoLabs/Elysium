//! Server-Sent Events: the server-to-client realtime channel (see `docs/realtime.md`).
//!
//! This is the transport skeleton. It authenticates exactly like the rest of the
//! API, via the [`crate::session::SessionUser`] cookie extractor, so the single
//! SharedWorker connection the frontend will hold rides the same session with no
//! token in the URL. The typed `EventEnvelope` fan-out and the per-user broadcast
//! bus are the next phase; today the stream proves auth, the connection cap, and
//! liveness with a periodic heartbeat.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::StreamExt;

use crate::session::SessionUser;
use crate::state::AppState;

/// Maximum concurrent SSE connections a single user may hold across all clients.
///
/// One connection per client (the SharedWorker holds it), but a user can have
/// several clients at once (desktop, browser, another device). Adjust here.
pub const MAX_SSE_CONNECTIONS_PER_USER: usize = 30;

/// Interval between heartbeat comments that hold the connection open through idle
/// periods and proxies.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// Tracks live SSE connections per user so the cap can be enforced. Cheaply
/// cloneable; the counts live behind a shared `Arc<Mutex<..>>`.
#[derive(Clone, Debug, Default)]
pub struct SseRegistry {
    counts: Arc<Mutex<HashMap<String, usize>>>,
}

impl SseRegistry {
    /// Reserves a connection slot for `subject`, or returns `None` at the cap.
    ///
    /// The returned guard releases the slot when dropped, i.e. when the stream
    /// ends, so a closed connection frees its slot automatically.
    fn acquire(&self, subject: &str) -> Option<ConnectionGuard> {
        let mut counts = self.counts.lock().expect("SSE registry mutex poisoned");
        let count = counts.entry(subject.to_owned()).or_insert(0);
        if *count >= MAX_SSE_CONNECTIONS_PER_USER {
            return None;
        }
        *count += 1;
        Some(ConnectionGuard {
            registry: self.clone(),
            subject: subject.to_owned(),
        })
    }
}

/// Releases a user's connection slot when the stream is dropped.
#[derive(Debug)]
struct ConnectionGuard {
    registry: SseRegistry,
    subject: String,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let mut counts = self
            .registry
            .counts
            .lock()
            .expect("SSE registry mutex poisoned");
        if let Some(count) = counts.get_mut(&self.subject) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(&self.subject);
            }
        }
    }
}

/// `GET /api/v1/events`: the authenticated event stream.
///
/// Protected by the session cookie: [`SessionUser`] rejects an unauthenticated
/// request with `401` before this runs. A user already holding
/// [`MAX_SSE_CONNECTIONS_PER_USER`] streams is rejected with `429`.
pub async fn events(
    axum::extract::State(state): axum::extract::State<AppState>,
    SessionUser(principal): SessionUser,
) -> Response {
    let Some(guard) = state.sse.acquire(&principal.sub) else {
        tracing::debug!(subject = %principal.sub, "SSE connection cap reached");
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    tracing::debug!(subject = %principal.sub, "SSE stream opened");

    // Placeholder stream until the per-user broadcast bus lands: a periodic tick
    // that keeps the connection warm and proves end-to-end auth + delivery. The
    // move closure owns `guard`, so the slot is released when the stream ends.
    let stream = IntervalStream::new(tokio::time::interval(HEARTBEAT_INTERVAL)).map(move |_| {
        let _keep_slot = &guard;
        Ok::<Event, Infallible>(Event::default().event("heartbeat").data("{}"))
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
