// Copyright © 2026 Jalapeno Labs

//! The event bus behind `GET /api/v1/events`, the one stream that keeps every open
//! frontend current.
//!
//! Anything that changes state a client displays publishes a [`ServerEvent`] here: route
//! handlers after a write, and every background loop as it works. Each connected client
//! holds a [`broadcast`] receiver and gets every event as one SSE `message` whose data is
//! `{"type": ..., "data": ...}`.
//!
//! # One channel for every process
//!
//! Events travel through one Redis pub/sub channel, [`EVENTS_CHANNEL`], so any process built
//! from this crate reaches every browser: the API's own writes, and a process that serves no
//! HTTP (the engine, `docs/engine.md`), arrive the same way. Each process holds one
//! subscription and relays what it receives into its own [`broadcast`] channel, which its
//! event streams and in-process listeners (Elysia's intake) read. [`EventBus::publish`] sends
//! to Redis alone, never straight to local listeners, so a process hears its own events in
//! the one order Redis gives everyone. The Redis half lives in [`redis_link`].
//!
//! # Delivery
//!
//! Delivery is at most once. Nothing is replayed: a client that connects, reconnects, or
//! falls behind receives [`ServerEvent::Hello`] or [`ServerEvent::Resync`] and refetches what
//! it shows, which keeps the server free of per-client state. Every way an event can be lost
//! on its way through Redis ends in a `resync` for whoever may have missed it: a lost
//! subscription announces one locally once it is restored, and a publish that fails or
//! overflows its queue is followed by one on the channel.
//!
//! # Signals
//!
//! Beside the events, the bus carries [`Signal`]s between processes on a channel of their own,
//! [`SIGNALS_CHANNEL`], which no client hears: the API telling the engine to look at work it
//! just recorded, such as a queued turn. See [`signals`].

mod redis_link;
pub mod signals;

use std::sync::Arc;

use anyhow::Result;
use secrecy::SecretString;
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use self::redis_link::RedisLink;
pub use self::signals::{Signal, SignalReceiver, Wake};

use crate::backups::BackupsStatus;
use crate::mail::hosting::MailServerStatus;
use crate::models::step_job::StepJobState;
use crate::models::workflow_run::WorkflowRunState;
use crate::passwords::VaultStatus;
use crate::routes::v1::action_items::{
    ActionItemLinkResponse, ActionItemResponse, CommentResponse, HistoryEntryResponse,
};
use crate::routes::v1::browser_sessions::BrowserSessionResponse;
use crate::routes::v1::changesets::ChangesetResponse;
use crate::routes::v1::coding_sessions::CodingSessionResponse;
use crate::routes::v1::elysia::{ElysiaResponse, MessageResponse, TurnResponse};
use crate::routes::v1::environment_variables::EnvironmentVariableResponse;
use crate::routes::v1::github_credentials::GithubCredentialResponse;
use crate::routes::v1::initiatives::{InitiativeLinkResponse, InitiativeResponse};
use crate::routes::v1::jira_credentials::JiraCredentialResponse;
use crate::routes::v1::llms::{LlmResponse, UsageView};
use crate::routes::v1::mail::{MailAccountResponse, MailDomainResponse};
use crate::routes::v1::password_vault::PasswordVaultResponse;
use crate::routes::v1::projects::ProjectResponse;
use crate::routes::v1::storage_locations::StorageLocationResponse;
use crate::routes::v1::studio_items::{
    StudioAssetResponse, StudioFeedbackResponse, StudioItemResponse,
};
use crate::routes::v1::users::UserResponse;
use crate::routes::v1::workspace_settings::WorkspaceSettingsResponse;
use crate::sessions::views::{SessionEvent, TurnSummaryView};
use crate::updates::UpdatesStatus;

/// The Redis pub/sub channel every process publishes to and subscribes on. Pub/sub channels
/// span the whole Redis server rather than one database, so the name alone keeps the events
/// apart from anything else on it.
pub const EVENTS_CHANNEL: &str = "elysium:events";

/// The Redis pub/sub channel signals between processes travel on. Its own, so no client
/// stream relays them.
pub const SIGNALS_CHANNEL: &str = "elysium:signals";

/// How many serialized events a slow listener may fall behind before it is told to resync.
/// Session event bursts (streamed agent output) are the largest source; this absorbs several
/// seconds of a busy session for a client on a slow link.
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
    /// A link was added or changed: its primary flag, what the provider reported, or the
    /// writes it owes.
    #[serde(rename = "actionItemLink.upserted")]
    ActionItemLinkUpserted(ActionItemLinkResponse),
    #[serde(rename = "actionItemLink.deleted")]
    ActionItemLinkDeleted { id: Uuid, action_item_id: Uuid },
    /// A container was linked to an initiative, or the watcher read it.
    #[serde(rename = "initiativeLink.upserted")]
    InitiativeLinkUpserted(InitiativeLinkResponse),
    #[serde(rename = "initiativeLink.deleted")]
    InitiativeLinkDeleted { id: Uuid, initiative_id: Uuid },
    #[serde(rename = "actionItemComment.upserted")]
    ActionItemCommentUpserted(CommentResponse),
    #[serde(rename = "actionItemComment.deleted")]
    ActionItemCommentDeleted { id: Uuid, action_item_id: Uuid },
    /// A browser was added, or what it is doing changed: starting, running, or failed.
    #[serde(rename = "browserSession.upserted")]
    BrowserSessionUpserted(BrowserSessionResponse),
    #[serde(rename = "browserSession.deleted")]
    BrowserSessionDeleted { id: Uuid },
    /// A browser went back to its start page in a fresh tab, its other pages closed and its
    /// profile kept. Its status is unchanged, so this is its only announcement.
    #[serde(rename = "browserSession.startedOver")]
    BrowserSessionStartedOver { id: Uuid, created_by: Uuid },
    /// A connector was added or changed: paused, resumed, its content, a sync starting or
    /// ending, a read finding something new, or its checks. Its id alone, since its view names
    /// whose mailbox or meetings it reads; clients that may see it load it by id.
    #[serde(rename = "connector.changed")]
    ConnectorChanged { id: Uuid },
    #[serde(rename = "connector.deleted")]
    ConnectorDeleted { id: Uuid },
    /// A learning was saved, corrected, deleted, restored, or had a change undone. Its id
    /// alone, since a guest hears every event and reaches no knowledge base; members load it by
    /// id.
    #[serde(rename = "learning.changed")]
    LearningChanged { id: Uuid },
    /// A workspace learning category was added, edited, reordered, or removed. Nothing more:
    /// the list is short, and a guest hears every event and reaches no knowledge base; members
    /// load it again.
    #[serde(rename = "learningCategories.changed")]
    LearningCategoriesChanged,
    /// The company doc changed, or a change to it was proposed or decided. Nothing more: the
    /// doc is one, and a guest hears every event and reaches no knowledge base; members load it
    /// again.
    #[serde(rename = "companyDoc.changed")]
    CompanyDocChanged,
    /// An admin chose the model and effort a step or agent runs on, or put it back on its
    /// default. Its name alone: every client hears this, and only admins read the settings, so
    /// their pages load the list again.
    #[serde(rename = "modelSetting.changed")]
    ModelSettingChanged { agent: String },
    /// A check in a run moved on, or (`checkId` null, `state` `finished`) the run ended. Ids and
    /// states only: a check's message can name what an account reaches, and the run answers the
    /// person who started it.
    #[serde(rename = "connectorCheckRun.updated")]
    ConnectorCheckRunUpdated {
        run_id: Uuid,
        connector_id: Option<Uuid>,
        check_id: Option<String>,
        state: String,
    },
    /// Something was ingested onto someone's timeline. Never its content: every client hears
    /// this, and a person's mail is theirs. Its owner's pages and Elysia load it by id.
    #[serde(rename = "timelineRecord.created")]
    TimelineRecordCreated {
        id: Uuid,
        source_kind: String,
        created_by: Uuid,
        /// The inbox item it raised, when it raised one.
        action_item_id: Option<Uuid>,
    },
    /// A draft was written, changed, discarded, or a send of it began, succeeded, or failed.
    /// Its id and writer alone, since its text is the writer's; their pages load it by id.
    #[serde(rename = "draft.changed")]
    DraftChanged { id: Uuid, created_by: Uuid },
    /// A meeting was read, or its processing moved on. Never its title or transcript, for
    /// the same reason as a record's content; its owner's pages load it by id.
    #[serde(rename = "meeting.changed")]
    MeetingChanged {
        id: Uuid,
        /// `pending`, `recorded`, `segmented`, `done`, or `failed`.
        state: String,
        created_by: Uuid,
    },
    /// A write to an item or initiative recorded this in its history.
    #[serde(rename = "history.appended")]
    HistoryAppended(HistoryEntryResponse),
    /// A changeset was proposed, decided on, applied, or undone.
    #[serde(rename = "changeset.upserted")]
    ChangesetUpserted(ChangesetResponse),
    /// Elysia's status, settings, open questions, or work changed.
    #[serde(rename = "elysia.updated")]
    ElysiaUpdated(Box<ElysiaResponse>),
    /// A person or Elysia added to the conversation.
    #[serde(rename = "elysiaMessage.created")]
    ElysiaMessageCreated(MessageResponse),
    /// One of her turns was queued, started, ended, or cancelled.
    #[serde(rename = "elysiaTurn.upserted")]
    ElysiaTurnUpserted(TurnResponse),
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
    /// A Claude account's usage changed. Numbers and instants only, never who the account is.
    #[serde(rename = "llm.usage")]
    LlmUsage { id: Uuid, usage: UsageView },
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
    /// A finished turn's summary was written.
    #[serde(rename = "session.summary")]
    TurnSummarized(TurnSummaryView),
    /// An item was created, renamed, pinned, or restored, or its files changed its
    /// thumbnail or counts.
    #[serde(rename = "studioItem.upserted")]
    StudioItemUpserted(StudioItemResponse),
    /// The item was deleted, softly (it may be restored) or for good.
    #[serde(rename = "studioItem.deleted")]
    StudioItemDeleted { id: Uuid },
    /// A file was pulled out of a workspace and stored.
    #[serde(rename = "studioAsset.created")]
    StudioAssetCreated(StudioAssetResponse),
    /// A prompt with a drawing was sent.
    #[serde(rename = "studioFeedback.created")]
    StudioFeedbackCreated(StudioFeedbackResponse),
    /// The database backups changed: one began or ended, or an admin chose where they go.
    #[serde(rename = "backups.status")]
    BackupsStatus(Box<BackupsStatus>),
    /// Auto-update's state changed: a check, an update beginning or ending, or what it
    /// waits for.
    #[serde(rename = "updates.status")]
    UpdatesStatus(Box<UpdatesStatus>),
    /// A person signed up, was approved, changed role, or was disabled or enabled.
    #[serde(rename = "user.upserted")]
    UserUpserted(UserResponse),
    /// A pending sign-up was rejected.
    #[serde(rename = "user.deleted")]
    UserDeleted { id: Uuid },
    #[serde(rename = "workspaceSettings.updated")]
    WorkspaceSettingsUpdated(WorkspaceSettingsResponse),
    /// The password vault was set up, its collection chosen, or its credentials replaced.
    #[serde(rename = "passwordVault.updated")]
    PasswordVaultUpdated(PasswordVaultResponse),
    #[serde(rename = "passwordVault.deleted")]
    PasswordVaultDeleted,
    /// What the vault's CLI is doing changed: starting, ready, failed, or a sync finished.
    #[serde(rename = "passwordVault.status")]
    PasswordVaultStatus(VaultStatus),
    /// A workflow run started, or it or one of its steps moved on, or a person acted on it.
    /// Its id and state alone: a run's input, output, and logs hold what it read, so clients
    /// load it by id.
    #[serde(rename = "workflowRun.changed")]
    WorkflowRunChanged { id: Uuid, state: WorkflowRunState },
    /// A step of a run moved on, or wrote to its log. Ids and its state alone, for the same
    /// reason; a client showing its log loads it again.
    #[serde(rename = "stepJob.changed")]
    StepJobChanged {
        id: Uuid,
        run_id: Uuid,
        state: StepJobState,
    },
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

/// The bus: publishes to every process, and relays what every process publishes.
///
/// Cheap to clone; clones share its Redis links and local channels.
#[derive(Debug, Clone)]
pub struct EventBus {
    /// This process's fan-out of everything on the events channel, as Redis delivered it.
    local: broadcast::Sender<Arc<str>>,
    /// The same for the signals channel.
    local_signals: broadcast::Sender<Arc<str>>,
    transport: Transport,
}

/// How a published event or signal reaches the local channels.
#[derive(Debug, Clone)]
enum Transport {
    /// Through Redis, for every process subscribed to the channels.
    Redis {
        events: RedisLink,
        signals: RedisLink,
    },
    /// Straight into the local channel, for unit tests that have no Redis.
    #[cfg(test)]
    InProcess,
}

impl EventBus {
    /// Connects to Redis, subscribes to [`EVENTS_CHANNEL`], and starts relaying.
    ///
    /// Returns once Redis has confirmed the subscription, so an event stream answered after
    /// this misses nothing published after it. Call [`EventBus::close`] last on the way out,
    /// to flush what is still queued.
    ///
    /// # Errors
    /// Fails when the URL is malformed, or Redis stays unreachable through the startup
    /// retries of `crate::connections`.
    pub async fn connect(redis_url: &SecretString) -> Result<Self> {
        Self::connect_to(redis_url, EVENTS_CHANNEL, SIGNALS_CHANNEL).await
    }

    /// [`EventBus::connect`] on channels of the test's own, named after `channel`, since every
    /// test shares one Redis and pub/sub channels span the whole server.
    #[cfg(test)]
    pub async fn connect_on(redis_url: &SecretString, channel: &str) -> Result<Self> {
        Self::connect_to(redis_url, channel, &format!("{channel}:signals")).await
    }

    async fn connect_to(
        redis_url: &SecretString,
        events_channel: &str,
        signals_channel: &str,
    ) -> Result<Self> {
        let (local, _receiver) = broadcast::channel(BUS_CAPACITY);
        let (local_signals, _receiver) = broadcast::channel(BUS_CAPACITY);
        let events = RedisLink::start(redis_url, events_channel, local.clone()).await?;
        let signals = RedisLink::start(redis_url, signals_channel, local_signals.clone()).await?;
        Ok(Self {
            local,
            local_signals,
            transport: Transport::Redis { events, signals },
        })
    }

    /// A bus that never leaves the process, for unit tests without Redis: what is published is
    /// delivered at once.
    #[cfg(test)]
    pub fn in_process() -> Self {
        let (local, _receiver) = broadcast::channel(BUS_CAPACITY);
        let (local_signals, _receiver) = broadcast::channel(BUS_CAPACITY);
        Self {
            local,
            local_signals,
            transport: Transport::InProcess,
        }
    }

    /// Sends an event to every listener of every process. Serialized once, here, rather than
    /// once per client.
    ///
    /// Never waits: the event is queued for Redis, in order. One that cannot be queued is
    /// logged, and a `resync` goes out in its place once the queue drains.
    pub fn publish(&self, server_event: &ServerEvent) {
        let payload = server_event.to_json();
        match &self.transport {
            Transport::Redis { events, .. } => events.send(payload),
            #[cfg(test)]
            Transport::InProcess => {
                // An error only means nobody is listening, which is a normal state.
                if self.local.send(payload).is_err() {
                    tracing::event!(
                        name: "realtime.publish.unobserved",
                        tracing::Level::TRACE,
                        "nobody is listening",
                    );
                }
            }
        }
    }

    /// Every event any process publishes from now on, as long as this process holds its
    /// subscription. When it loses it, listeners receive `resync` once it is restored.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.local.subscribe()
    }

    /// Sends `signal` to every process, this one included. Never waits, like
    /// [`EventBus::publish`].
    pub fn signal(&self, signal: &Signal) {
        let payload = signal.to_json();
        match &self.transport {
            Transport::Redis { signals, .. } => signals.send(payload),
            #[cfg(test)]
            Transport::InProcess => {
                // An error only means nobody is listening, which is a normal state.
                if self.local_signals.send(payload).is_err() {
                    tracing::event!(
                        name: "realtime.signal.unobserved",
                        tracing::Level::TRACE,
                        "nobody is listening",
                    );
                }
            }
        }
    }

    /// Every signal any process sends from now on.
    pub fn signals(&self) -> SignalReceiver {
        SignalReceiver::new(self.local_signals.subscribe())
    }

    /// A wake for one loop: it resolves at each signal `wants` accepts, and at a resync.
    /// Taken once, before the loop's first pass, so nothing sent during a pass is missed.
    pub fn wake_on(&self, wants: fn(&Signal) -> bool) -> Wake {
        Wake::new(self.signals(), wants)
    }

    /// Whether this process holds its Redis subscription right now. While it does not, nothing
    /// published anywhere reaches its listeners, so a new event stream must not open: its
    /// `hello` would promise events it cannot deliver.
    pub fn is_subscribed(&self) -> bool {
        match &self.transport {
            Transport::Redis { events, .. } => events.is_subscribed(),
            #[cfg(test)]
            Transport::InProcess => true,
        }
    }

    /// Flushes what is still queued for Redis, then stops relaying.
    ///
    /// The last step of shutdown, once every loop that publishes has stopped: an event
    /// published after it is logged and dropped. Every clone closes with it.
    pub async fn close(&self) {
        match &self.transport {
            Transport::Redis { events, signals } => {
                tokio::join!(events.close(), signals.close());
            }
            #[cfg(test)]
            Transport::InProcess => {}
        }
    }
}

#[cfg(test)]
mod tests;
