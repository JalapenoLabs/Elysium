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
use crate::mail::hosting::MailServerStatus;
use crate::routes::v1::action_items::{ActionItemResponse, CommentResponse, HistoryEntryResponse};
use crate::routes::v1::coding_sessions::CodingSessionResponse;
use crate::routes::v1::environment_variables::EnvironmentVariableResponse;
use crate::routes::v1::github_credentials::GithubCredentialResponse;
use crate::routes::v1::initiatives::InitiativeResponse;
use crate::routes::v1::jira_credentials::JiraCredentialResponse;
use crate::routes::v1::llms::LlmResponse;
use crate::routes::v1::mail::{MailAccountResponse, MailDomainResponse};
use crate::routes::v1::projects::ProjectResponse;
use crate::routes::v1::satellites::SatelliteResponse;
use crate::routes::v1::storage_locations::StorageLocationResponse;

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
    #[serde(rename = "actionItem.upserted")]
    ActionItemUpserted(ActionItemResponse),
    /// The item was deleted or, softly, hidden until restored.
    #[serde(rename = "actionItem.deleted")]
    ActionItemDeleted { id: Uuid },
    #[serde(rename = "actionItemComment.upserted")]
    ActionItemCommentUpserted(CommentResponse),
    #[serde(rename = "actionItemComment.deleted")]
    ActionItemCommentDeleted { id: Uuid, action_item_id: Uuid },
    /// A write to an item or initiative recorded this in its history.
    #[serde(rename = "history.appended")]
    HistoryAppended(HistoryEntryResponse),
    #[serde(rename = "environmentVariable.upserted")]
    EnvironmentVariableUpserted(EnvironmentVariableResponse),
    #[serde(rename = "environmentVariable.deleted")]
    EnvironmentVariableDeleted { id: Uuid },
    #[serde(rename = "githubCredential.upserted")]
    GithubCredentialUpserted(GithubCredentialResponse),
    #[serde(rename = "githubCredential.deleted")]
    GithubCredentialDeleted { id: Uuid },
    /// An initiative was created or changed, or its progress moved.
    #[serde(rename = "initiative.upserted")]
    InitiativeUpserted(InitiativeResponse),
    /// The initiative was deleted or, softly, hidden until restored.
    #[serde(rename = "initiative.deleted")]
    InitiativeDeleted { id: Uuid },
    #[serde(rename = "jiraCredential.upserted")]
    JiraCredentialUpserted(JiraCredentialResponse),
    #[serde(rename = "jiraCredential.deleted")]
    JiraCredentialDeleted { id: Uuid },
    #[serde(rename = "llm.upserted")]
    LlmUpserted(LlmResponse),
    #[serde(rename = "llm.deleted")]
    LlmDeleted { id: Uuid },
    #[serde(rename = "mailbox.upserted")]
    MailAccountUpserted(MailAccountResponse),
    #[serde(rename = "mailbox.deleted")]
    MailAccountDeleted { id: Uuid },
    /// The mail server's status changed, including each step while it is created.
    #[serde(rename = "mailServer.updated")]
    MailServerUpdated(MailServerStatus),
    #[serde(rename = "mailDomain.upserted")]
    MailDomainUpserted(MailDomainResponse),
    #[serde(rename = "mailDomain.deleted")]
    MailDomainDeleted { id: Uuid },
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
    #[serde(rename = "storageLocation.upserted")]
    StorageLocationUpserted(StorageLocationResponse),
    #[serde(rename = "storageLocation.deleted")]
    StorageLocationDeleted { id: Uuid },
    #[serde(rename = "session.upserted")]
    SessionUpserted(CodingSessionResponse),
    #[serde(rename = "session.deleted")]
    SessionDeleted { id: i64 },
    #[serde(rename = "session.event")]
    SessionEvent(Box<SessionEvent>),
    /// Live events for the session may have been missed; refetch its history.
    #[serde(rename = "session.resync")]
    SessionResync { id: i64 },
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

        let session_deleted: Value =
            serde_json::from_str(&ServerEvent::SessionDeleted { id: 12 }.to_json()).expect("json");
        assert_eq!(
            session_deleted,
            json!({ "type": "session.deleted", "data": { "id": 12 } })
        );

        let action_item_id = Uuid::now_v7();
        let comment_deleted: Value = serde_json::from_str(
            &ServerEvent::ActionItemCommentDeleted { id, action_item_id }.to_json(),
        )
        .expect("json");
        assert_eq!(
            comment_deleted,
            json!({
                "type": "actionItemComment.deleted",
                "data": { "id": id, "actionItemId": action_item_id },
            })
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
