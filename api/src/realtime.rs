// Copyright © 2026 Jalapeno Labs

//! The event bus behind `GET /api/v1/events`, the one stream that keeps every open
//! frontend current.
//!
//! Anything that changes state a client displays publishes a [`ServerEvent`] here:
//! route handlers after a write, and the fleet watchers as satellites report. Each
//! connected client holds a [`broadcast`] receiver and gets every event as one SSE
//! `message` whose data is `{"type": ..., "data": ...}`.
//!
//! # Delivery
//!
//! Delivery is at most once and in-process. Nothing is replayed: a client that
//! connects, reconnects, or falls behind receives [`ServerEvent::Hello`] or
//! [`ServerEvent::Resync`] and refetches what it shows. That keeps the server free of
//! per-client state, and it is also why the API runs as a single replica today: an
//! event published on one process never reaches clients connected to another. Redis
//! pub/sub is the path to more replicas.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::{Level, event};
use uuid::Uuid;

use crate::fleet::views::{SatelliteStatus, SessionEvent};
use crate::routes::v1::coding_sessions::CodingSessionResponse;
use crate::routes::v1::llms::LlmResponse;
use crate::routes::v1::mail::MailAccountResponse;
use crate::routes::v1::projects::ProjectResponse;
use crate::routes::v1::satellites::SatelliteResponse;

/// How many serialized events a slow client may fall behind before it is told to
/// resync. Thread event bursts (streamed agent output) are the largest source; this
/// absorbs several seconds of a busy thread for a client on a slow link.
const BUS_CAPACITY: usize = 4096;

/// Everything a client can be told. Serialized as `{"type": <rename>, "data": <payload>}`.
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all_fields = "camelCase")]
pub enum ServerEvent {
    /// First message on every connection. Clients load their state after it.
    #[serde(rename = "hello")]
    Hello,
    /// The client missed events and must refetch everything it shows.
    #[serde(rename = "resync")]
    Resync,
    #[serde(rename = "llm.upserted")]
    LlmUpserted(LlmResponse),
    #[serde(rename = "llm.deleted")]
    LlmDeleted { id: Uuid },
    #[serde(rename = "mailbox.upserted")]
    MailAccountUpserted(MailAccountResponse),
    #[serde(rename = "mailbox.deleted")]
    MailAccountDeleted { id: Uuid },
    #[serde(rename = "project.upserted")]
    ProjectUpserted(ProjectResponse),
    #[serde(rename = "project.deleted")]
    ProjectDeleted { id: Uuid },
    #[serde(rename = "satellite.upserted")]
    SatelliteUpserted(SatelliteResponse),
    /// Also removes every session recorded against the satellite.
    #[serde(rename = "satellite.deleted")]
    SatelliteDeleted { id: Uuid },
    #[serde(rename = "satellite.status")]
    SatelliteStatus(SatelliteStatus),
    #[serde(rename = "session.upserted")]
    SessionUpserted(CodingSessionResponse),
    #[serde(rename = "session.deleted")]
    SessionDeleted { id: Uuid },
    #[serde(rename = "session.event")]
    SessionEvent(Box<SessionEvent>),
    /// Live events for the session may have been missed; refetch its history.
    #[serde(rename = "session.resync")]
    SessionResync { id: Uuid },
}

impl ServerEvent {
    /// The event as SSE `data`.
    ///
    /// # Panics
    /// Panics if a payload fails to serialize, which only a programming error in a
    /// `Serialize` implementation can cause: every payload is plain data.
    pub fn to_json(&self) -> Arc<str> {
        serde_json::to_string(self)
            .expect("server events serialize")
            .into()
    }
}

/// Publishing half of the bus. Cheap to clone; clones share one channel.
#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Arc<str>>,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _receiver) = broadcast::channel(BUS_CAPACITY);
        Self { sender }
    }

    /// Sends an event to every connected client. Serialized once, here, rather than
    /// once per client.
    pub fn publish(&self, server_event: &ServerEvent) {
        // An error only means nobody is connected, which is a normal state.
        if self.sender.send(server_event.to_json()).is_err() {
            event!(name: "realtime.publish.unobserved", Level::TRACE, "no clients connected");
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.sender.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn events_serialize_as_a_type_and_data_envelope() {
        let id = Uuid::nil();
        let deleted: Value =
            serde_json::from_str(&ServerEvent::SatelliteDeleted { id }.to_json()).expect("json");
        assert_eq!(
            deleted,
            json!({ "type": "satellite.deleted", "data": { "id": id } })
        );

        let hello: Value = serde_json::from_str(&ServerEvent::Hello.to_json()).expect("json");
        assert_eq!(hello, json!({ "type": "hello" }));
    }

    #[tokio::test]
    async fn every_subscriber_receives_each_published_event() {
        let bus = EventBus::new();
        let mut first = bus.subscribe();
        let mut second = bus.subscribe();

        bus.publish(&ServerEvent::Resync);

        assert_eq!(
            &*first.recv().await.expect("delivered"),
            r#"{"type":"resync"}"#
        );
        assert_eq!(
            &*second.recv().await.expect("delivered"),
            r#"{"type":"resync"}"#
        );
    }
}
