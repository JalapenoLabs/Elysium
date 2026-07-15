//! Server-Sent Events: the server-to-client realtime bus (see `docs/realtime.md`).
//!
//! Each user has a broadcast channel; every live SSE stream for that user
//! subscribes to it, so publishing an event to a user reaches all of their
//! connections (the SharedWorker holds one per app). Events are the typed
//! [`EventEnvelope`] proto, base64-encoded into the SSE `data:` line, with the
//! `kind` mirrored as the SSE `event:` name and a per-user `sequence` as the `id:`
//! for Last-Event-ID resume. Higher-level fan-out (to an org's members, to a
//! user's teammates) lives in [`crate::events`]; this module is the transport.
//!
//! Authentication is the session cookie ([`crate::session::SessionUser`]), so the
//! stream rides the same session with no token in the URL.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::Utc;
use prost::Message;
use tokio::sync::broadcast;
use tokio_stream::wrappers::{BroadcastStream, IntervalStream};
use tokio_stream::StreamExt;

use crate::proto::common::v1::Timestamp;
use crate::proto::realtime::v1::event_envelope::Payload;
use crate::proto::realtime::v1::{EventEnvelope, EventKind};
use crate::session::SessionUser;
use crate::state::AppState;

/// Maximum concurrent SSE connections a single user may hold across all clients.
///
/// One connection per client (the SharedWorker holds it), but a user can have
/// several clients at once (desktop, browser, another device).
pub const MAX_SSE_CONNECTIONS_PER_USER: usize = 30;

/// Events buffered per user before a slow client is told it lagged.
const BROADCAST_CAPACITY: usize = 64;

/// Interval between heartbeat comments that hold the connection open through idle
/// periods and proxies.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// A user's broadcast channel plus its live-connection count and event sequence.
struct UserChannel {
    sender: broadcast::Sender<EventEnvelope>,
    count: usize,
    sequence: u64,
}

/// Per-user realtime channels: the connection cap, presence source, and event bus.
/// Cheaply cloneable; the state lives behind a shared `Arc<Mutex<..>>`.
#[derive(Clone, Default)]
pub struct SseRegistry {
    inner: Arc<Mutex<HashMap<String, UserChannel>>>,
}

impl SseRegistry {
    /// Whether the user currently holds at least one live SSE connection.
    ///
    /// The source of derived presence (see `crate::profile`). Per-instance only.
    pub fn is_online(&self, subject: &str) -> bool {
        let map = self.inner.lock().expect("SSE registry mutex poisoned");
        map.get(subject).is_some_and(|channel| channel.count > 0)
    }

    /// The user's current realtime high-water sequence, or `0` if none exists yet.
    ///
    /// A session-bootstrap read (see `crate::context`) captures this after loading
    /// state, so the client applies only later SSE events and the initial fetch and
    /// the live stream reconcile without a gap.
    pub fn sequence(&self, subject: &str) -> u64 {
        let map = self.inner.lock().expect("SSE registry mutex poisoned");
        map.get(subject).map_or(0, |channel| channel.sequence)
    }

    /// Force-disconnects all of a user's live SSE streams (e.g. on revocation).
    ///
    /// Removes the user's channel, dropping its broadcast sender; each open stream's
    /// `BroadcastStream` then observes the close and ends, so the connections
    /// terminate promptly. A later reconnect simply creates a fresh channel. Used to
    /// cut realtime the instant a session is revoked, so a revoked client cannot
    /// keep receiving events on an already-open stream.
    pub fn disconnect(&self, subject: &str) {
        let mut map = self.inner.lock().expect("SSE registry mutex poisoned");
        map.remove(subject);
    }

    /// Reserves a connection slot and returns a receiver on the user's channel.
    ///
    /// `None` at the cap. The `bool` is whether this connection brought the user
    /// online (their first), so the caller can announce presence.
    fn acquire(&self, subject: &str) -> Option<(broadcast::Receiver<EventEnvelope>, bool)> {
        let mut map = self.inner.lock().expect("SSE registry mutex poisoned");
        let channel = map
            .entry(subject.to_owned())
            .or_insert_with(|| UserChannel {
                sender: broadcast::channel(BROADCAST_CAPACITY).0,
                count: 0,
                sequence: 0,
            });
        if channel.count >= MAX_SSE_CONNECTIONS_PER_USER {
            return None;
        }
        let became_online = channel.count == 0;
        channel.count += 1;
        Some((channel.sender.subscribe(), became_online))
    }

    /// Releases a connection slot; returns whether the user is now offline.
    fn release(&self, subject: &str) -> bool {
        let mut map = self.inner.lock().expect("SSE registry mutex poisoned");
        let Some(channel) = map.get_mut(subject) else {
            return false;
        };
        channel.count = channel.count.saturating_sub(1);
        if channel.count == 0 {
            map.remove(subject);
            return true;
        }
        false
    }

    /// Publishes an event to one user's live connections (no-op if none).
    ///
    /// Assigns the user's next sequence and sends; a send error means no receivers
    /// are attached, which is fine.
    pub fn send(&self, subject: &str, payload: Payload) {
        let mut map = self.inner.lock().expect("SSE registry mutex poisoned");
        let Some(channel) = map.get_mut(subject) else {
            return;
        };
        channel.sequence += 1;
        let envelope = EventEnvelope {
            sequence: channel.sequence,
            emitted_at: Some(now()),
            kind: kind_of(&payload) as i32,
            payload: Some(payload),
        };
        let _ = channel.sender.send(envelope);
    }
}

/// Frees a connection slot when the SSE stream ends, announcing presence offline
/// if it was the user's last one. Held for the lifetime of the stream.
struct ConnectionGuard {
    state: AppState,
    subject: String,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if self.state.sse.release(&self.subject) {
            crate::events::announce_presence(&self.state, self.subject.clone(), false);
        }
    }
}

/// `GET /api/v1/events`: the authenticated realtime stream.
///
/// Protected by the session cookie. A user already holding
/// [`MAX_SSE_CONNECTIONS_PER_USER`] streams is rejected with `429`. The stream
/// merges the user's broadcast channel with a heartbeat so it stays warm while
/// idle.
pub async fn events(
    axum::extract::State(state): axum::extract::State<AppState>,
    SessionUser(principal): SessionUser,
) -> Response {
    let Some((receiver, became_online)) = state.sse.acquire(&principal.sub) else {
        tracing::debug!(subject = %principal.sub, "SSE connection cap reached");
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    tracing::debug!(subject = %principal.sub, "SSE stream opened");

    // Announce presence online now; the guard announces offline when the stream
    // ends. Both are best-effort teammate fan-outs.
    if became_online {
        crate::events::announce_presence(&state, principal.sub.clone(), true);
    }
    let guard = ConnectionGuard {
        state,
        subject: principal.sub,
    };

    // Each broadcast envelope becomes an SSE event; a lagged slow client skips the
    // dropped envelopes (its next event resyncs it).
    let broadcast =
        BroadcastStream::new(receiver).filter_map(|item| item.ok().map(|e| to_event(&e)));
    // The heartbeat keeps the connection warm; the guard moves into this closure,
    // so the slot frees (and offline fires) exactly when the stream ends.
    let heartbeat = IntervalStream::new(tokio::time::interval(HEARTBEAT_INTERVAL)).map(move |_| {
        let _keep_slot = &guard;
        Ok(Event::default().event("heartbeat").data("{}"))
    });

    Sse::new(broadcast.merge(heartbeat))
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Serializes an envelope into an SSE event (base64 proto, kind name, sequence id).
fn to_event(envelope: &EventEnvelope) -> Result<Event, std::convert::Infallible> {
    let kind = EventKind::try_from(envelope.kind).unwrap_or(EventKind::Unspecified);
    Ok(Event::default()
        .event(event_name(kind))
        .id(envelope.sequence.to_string())
        .data(STANDARD.encode(envelope.encode_to_vec())))
}

/// The SSE `event:` name for a kind; clients subscribe by these.
fn event_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Unspecified => "unspecified",
        EventKind::Snapshot => "snapshot",
        EventKind::OrgUpdated => "org.updated",
        EventKind::ProfileUpdated => "profile.updated",
        EventKind::PresenceChanged => "presence.changed",
        EventKind::SessionChanged => "session.changed",
    }
}

/// The kind that matches a payload variant.
fn kind_of(payload: &Payload) -> EventKind {
    match payload {
        Payload::Snapshot(_) => EventKind::Snapshot,
        Payload::OrgUpdated(_) => EventKind::OrgUpdated,
        Payload::ProfileUpdated(_) => EventKind::ProfileUpdated,
        Payload::PresenceChanged(_) => EventKind::PresenceChanged,
        Payload::SessionChanged(_) => EventKind::SessionChanged,
    }
}

/// The current instant as a proto `Timestamp`.
fn now() -> Timestamp {
    let instant = Utc::now();
    Timestamp {
        epoch_seconds: instant.timestamp(),
        nanos: instant.timestamp_subsec_nanos(),
        timezone: "Etc/UTC".to_string(),
    }
}
