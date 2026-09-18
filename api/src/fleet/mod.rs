// Copyright © 2026 Jalapeno Labs

//! The fleet: Elysium's live connection to every Arsox satellite it knows.
//!
//! The frontend never talks to a satellite. Satellites speak protobuf and hold a
//! bearer secret that must not reach a browser, so the API is their only client, and
//! this module is where it keeps those connections and turns what satellites report
//! into [`ServerEvent`]s.
//!
//! # Watchers
//!
//! Each active satellite has a **satellite watcher** task. Every
//! [`STATUS_POLL_INTERVAL`] it reads the satellite's status and lists the threads
//! Elysium opened there, publishing `satellite.status` when reachability changes and
//! `session.upserted` when a session's thread changes state. Polling stands in for
//! the satellite's control stream, which the Rust SDK does not expose yet.
//!
//! Each session on an active satellite has a **session watcher** task. It follows
//! the thread's event stream from the latest sequence and publishes every event as
//! `session.event`. History is not replayed here; clients fetch it on demand from
//! `GET /api/v1/coding-sessions/{id}/events` and merge on `sequence`. After a
//! dropped stream the watcher resumes from the last sequence it delivered. If that
//! resume point cannot be honored, it tails from the latest event instead and
//! publishes `session.resync` so clients refetch the history they missed.
//!
//! Beside each session watcher runs the session's **relay** ([`relay`]), which answers the
//! tool calls its agent makes to Elysium over a socket Elysium opens to the satellite.
//!
//! # Lifetimes
//!
//! Every watcher runs under a child of the process shutdown token and is tracked, so
//! [`Fleet::shutdown`] cancels and awaits them all. Changing or deleting a satellite
//! restarts or stops its watchers through [`Fleet::reload_satellite`] and
//! [`Fleet::forget_satellite`].

mod relay;
pub mod views;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::Context;
use arsox_sdk::client::Satellite as SatelliteClient;
use arsox_sdk::proto::error::v1::ErrorCode;
use arsox_sdk::proto::thread::v1::ThreadOrder;
use futures_util::StreamExt as _;
use secrecy::ExposeSecret;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Level, event};
use uuid::Uuid;

use self::views::{SatelliteStatus, SessionEvent, ThreadState, ThreadStatus};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::coding_session::{self, CodingSession};
use crate::models::satellite::{self, Satellite};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::coding_sessions::CodingSessionResponse;
use crate::storage::Storage;
use crate::tools::ToolContext;

/// Metadata key marking a thread as opened by Elysium, so the watcher lists only those.
pub const MANAGED_METADATA_KEY: &str = "elysium.managed";

/// Metadata key carrying the number of the Elysium session a thread belongs to, for
/// operators reading a satellite directly. Nothing matches on it: numbers repeat across
/// Elysium installs sharing a satellite, so sessions find their threads by thread id on
/// their own satellite.
pub const SESSION_METADATA_KEY: &str = "elysium.session_id";

/// How often each satellite is polled. Bounds how stale a thread's displayed state
/// can be; each poll is two small requests per satellite.
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Watcher bookkeeping locks are never held across an await or a call that can panic,
/// so a poisoned lock means a bug elsewhere already brought the process down.
const LOCK_POISONED: &str = "fleet bookkeeping lock poisoned by an earlier panic";

/// First delay before a session watcher reconnects to a dropped thread stream.
const RECONNECT_BACKOFF_INITIAL: Duration = Duration::from_secs(1);

/// Longest delay between reconnect attempts, reached after repeated failures.
const RECONNECT_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Why a satellite client could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    #[error("satellite is inactive")]
    Inactive,
    #[error(transparent)]
    Satellite(#[from] arsox_sdk::client::Error),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<ClientError> for ApiError {
    fn from(error: ClientError) -> Self {
        match error {
            ClientError::Database(database_error) => Self::from(database_error),
            ClientError::Inactive => Self::Conflict("the satellite is inactive"),
            ClientError::Satellite(satellite_error) => Self::from(satellite_error),
            ClientError::Internal(internal_error) => Self::Internal(internal_error),
        }
    }
}

/// Handle to the fleet. Cheap to clone; clones share every connection and watcher.
#[derive(Clone)]
pub struct Fleet {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
    /// What each session's relay answers its agent's tool calls with.
    tools: ToolContext,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    /// Connected clients by satellite id, created on first use.
    clients: Mutex<HashMap<Uuid, SatelliteClient>>,
    /// Bumped whenever a client is invalidated, so a connect that raced the
    /// invalidation does not cache a client built from stale settings.
    client_generation: AtomicU64,
    satellite_watchers: Mutex<HashMap<Uuid, CancellationToken>>,
    session_watchers: Mutex<HashMap<i64, SessionWatch>>,
    satellite_statuses: RwLock<HashMap<Uuid, SatelliteStatus>>,
    /// Thread status by session id.
    thread_statuses: RwLock<HashMap<i64, ThreadStatus>>,
}

#[derive(Debug)]
struct SessionWatch {
    satellite_id: Uuid,
    cancel: CancellationToken,
}

/// How one attempt at following a thread stream ended.
enum FollowOutcome {
    Cancelled,
    /// The satellite no longer has the thread; carries why, as a final state.
    ThreadGone(ThreadState),
    Interrupted {
        reason: String,
        delivered: bool,
    },
}

impl Fleet {
    pub fn new(
        database: Pool,
        cipher: Arc<Cipher>,
        events: EventBus,
        storage: Storage,
        shutdown: CancellationToken,
    ) -> Self {
        let tools = ToolContext {
            database: database.clone(),
            cipher: Arc::clone(&cipher),
            storage,
            events: events.clone(),
        };
        Self {
            inner: Arc::new(Inner {
                database,
                cipher,
                events,
                tools,
                shutdown,
                tasks: TaskTracker::new(),
                clients: Mutex::new(HashMap::new()),
                client_generation: AtomicU64::new(0),
                satellite_watchers: Mutex::new(HashMap::new()),
                session_watchers: Mutex::new(HashMap::new()),
                satellite_statuses: RwLock::new(HashMap::new()),
                thread_statuses: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Starts watching every active satellite and the sessions recorded on them.
    ///
    /// # Errors
    /// Fails when the database cannot be read.
    pub async fn start(&self) -> anyhow::Result<()> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let satellites = satellite::list(&mut connection).await?;
        let sessions = coding_session::list(&mut connection).await?;

        let mut active = HashSet::new();
        for satellite in satellites.iter().filter(|satellite| satellite.is_active) {
            active.insert(satellite.id);
            self.watch_satellite(satellite.id);
        }
        for session in sessions {
            if active.contains(&session.satellite_id) {
                self.watch_session(session);
            }
        }

        event!(
            name: "fleet.start.success",
            Level::INFO,
            fleet.satellites = active.len(),
            "fleet watchers started",
        );
        Ok(())
    }

    /// Cancels every watcher and waits for them to finish.
    pub async fn shutdown(&self) {
        self.inner.shutdown.cancel();
        self.inner.tasks.close();
        self.inner.tasks.wait().await;
    }

    /// A connected client for an active satellite, reused across calls.
    ///
    /// # Errors
    /// Fails for an unknown or inactive satellite, a secret that cannot be decrypted,
    /// or a satellite that is unreachable or speaks an incompatible contract.
    pub async fn client(&self, satellite_id: Uuid) -> Result<SatelliteClient, ClientError> {
        if let Some(client) = self
            .inner
            .clients
            .lock()
            .expect(LOCK_POISONED)
            .get(&satellite_id)
        {
            return Ok(client.clone());
        }

        let generation = self.inner.client_generation.load(Ordering::SeqCst);
        let satellite = self.find_satellite(satellite_id).await?;
        if !satellite.is_active {
            return Err(ClientError::Inactive);
        }
        let client = connect(&satellite, &self.inner.cipher).await?;

        let mut clients = self.inner.clients.lock().expect(LOCK_POISONED);
        if self.inner.client_generation.load(Ordering::SeqCst) == generation {
            clients.insert(satellite_id, client.clone());
        }
        Ok(client)
    }

    /// Applies a created or changed satellite: drops its client and watchers, then
    /// watches it and its sessions again if it is active.
    ///
    /// # Errors
    /// Fails when the satellite's sessions cannot be read.
    pub async fn reload_satellite(&self, satellite: &Satellite) -> Result<(), ApiError> {
        self.forget_satellite(satellite.id);
        if !satellite.is_active {
            return Ok(());
        }

        self.watch_satellite(satellite.id);
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        for session in coding_session::list_for_satellite(&mut connection, satellite.id).await? {
            self.watch_session(session);
        }
        Ok(())
    }

    /// Stops watching a satellite and its sessions and drops its client.
    pub fn forget_satellite(&self, satellite_id: Uuid) {
        self.invalidate_client(satellite_id);

        if let Some(cancel) = self
            .inner
            .satellite_watchers
            .lock()
            .expect(LOCK_POISONED)
            .remove(&satellite_id)
        {
            cancel.cancel();
        }
        self.inner
            .satellite_statuses
            .write()
            .expect(LOCK_POISONED)
            .remove(&satellite_id);

        let mut forgotten_sessions = Vec::new();
        self.inner
            .session_watchers
            .lock()
            .expect(LOCK_POISONED)
            .retain(|session_id, watch| {
                if watch.satellite_id != satellite_id {
                    return true;
                }
                watch.cancel.cancel();
                forgotten_sessions.push(*session_id);
                false
            });
        let mut thread_statuses = self.inner.thread_statuses.write().expect(LOCK_POISONED);
        for session_id in forgotten_sessions {
            thread_statuses.remove(&session_id);
        }
    }

    /// Starts following a session's thread and relaying its tool calls, replacing any
    /// existing watcher for it.
    pub fn watch_session(&self, session: CodingSession) {
        let cancel = self.inner.shutdown.child_token();
        let watch = SessionWatch {
            satellite_id: session.satellite_id,
            cancel: cancel.clone(),
        };
        if let Some(previous) = self
            .inner
            .session_watchers
            .lock()
            .expect(LOCK_POISONED)
            .insert(session.id, watch)
        {
            previous.cancel.cancel();
        }
        self.inner.tasks.spawn(relay::run_relay(
            self.clone(),
            session.clone(),
            cancel.clone(),
        ));
        self.inner
            .tasks
            .spawn(run_session_watcher(self.clone(), session, cancel));
    }

    /// Stops following a session's thread and relaying its tool calls.
    pub fn forget_session(&self, session_id: i64) {
        if let Some(watch) = self
            .inner
            .session_watchers
            .lock()
            .expect(LOCK_POISONED)
            .remove(&session_id)
        {
            watch.cancel.cancel();
        }
        self.inner
            .thread_statuses
            .write()
            .expect(LOCK_POISONED)
            .remove(&session_id);
    }

    /// What the last poll learned about a satellite, if it has been polled.
    pub fn satellite_status(&self, satellite_id: Uuid) -> Option<SatelliteStatus> {
        self.inner
            .satellite_statuses
            .read()
            .expect(LOCK_POISONED)
            .get(&satellite_id)
            .cloned()
    }

    /// The state of a session's thread as of the last poll, if known.
    pub fn thread_status(&self, session_id: i64) -> Option<ThreadStatus> {
        self.inner
            .thread_statuses
            .read()
            .expect(LOCK_POISONED)
            .get(&session_id)
            .cloned()
    }

    fn watch_satellite(&self, satellite_id: Uuid) {
        let cancel = self.inner.shutdown.child_token();
        if let Some(previous) = self
            .inner
            .satellite_watchers
            .lock()
            .expect(LOCK_POISONED)
            .insert(satellite_id, cancel.clone())
        {
            previous.cancel();
        }
        self.inner
            .tasks
            .spawn(run_satellite_watcher(self.clone(), satellite_id, cancel));
    }

    fn invalidate_client(&self, satellite_id: Uuid) {
        let mut clients = self.inner.clients.lock().expect(LOCK_POISONED);
        self.inner.client_generation.fetch_add(1, Ordering::SeqCst);
        clients.remove(&satellite_id);
    }

    async fn find_satellite(&self, satellite_id: Uuid) -> Result<Satellite, ClientError> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        Ok(satellite::find(&mut connection, satellite_id).await?)
    }

    /// One status poll: reachability, then the state of every Elysium thread.
    async fn poll_satellite(&self, satellite_id: Uuid) {
        let client = match self.client(satellite_id).await {
            Ok(client) => client,
            Err(error) => {
                self.record_satellite_status(unreachable(satellite_id, &error));
                return;
            }
        };

        let status = match client.status().await {
            Ok(status) => status,
            Err(error) => {
                // A fresh client re-runs the version check once the satellite returns.
                self.invalidate_client(satellite_id);
                self.record_satellite_status(unreachable(satellite_id, &error));
                return;
            }
        };
        self.record_satellite_status(SatelliteStatus {
            satellite_id,
            reachable: true,
            version: Some(status.satellite_version),
            running_threads: Some(status.running_threads),
            max_concurrent_threads: Some(status.max_concurrent_threads),
            error: None,
        });

        if let Err(error) = self.poll_threads(&client, satellite_id).await {
            event!(
                name: "fleet.threads.poll.failure",
                Level::WARN,
                satellite.id = %satellite_id,
                error.message = %error,
                "could not refresh thread states",
            );
        }
    }

    async fn poll_threads(
        &self,
        client: &SatelliteClient,
        satellite_id: Uuid,
    ) -> anyhow::Result<()> {
        let filter = BTreeMap::from([(MANAGED_METADATA_KEY.to_owned(), "true".to_owned())]);
        let summaries = client
            .threads()
            .list_ordered(filter, ThreadOrder::LastActivity, true)
            .await?;
        let summaries_by_thread: HashMap<&str, _> = summaries
            .iter()
            .map(|summary| (summary.thread_id.as_str(), summary))
            .collect();

        let sessions = {
            let mut connection = self
                .inner
                .database
                .get()
                .await
                .context("no database connection available")?;
            coding_session::list_for_satellite(&mut connection, satellite_id).await?
        };

        for session in sessions {
            // A thread missing from the listing keeps its last known status: the
            // listing is paged, so absence alone does not mean the thread is gone.
            let Some(summary) = summaries_by_thread.get(session.thread_id.as_str()) else {
                continue;
            };
            let status = ThreadStatus::from(*summary);

            let changed = {
                let mut thread_statuses = self.inner.thread_statuses.write().expect(LOCK_POISONED);
                let changed = thread_statuses.get(&session.id) != Some(&status);
                if changed {
                    thread_statuses.insert(session.id, status.clone());
                }
                changed
            };
            if changed {
                self.inner.events.publish(&ServerEvent::SessionUpserted(
                    CodingSessionResponse::new(session, Some(status)),
                ));
            }
        }
        Ok(())
    }

    /// Marks a session's thread as ended, since no listing will report it again, and
    /// tells clients so the session stops offering a composer.
    fn record_thread_ended(&self, session: CodingSession, final_state: ThreadState) {
        let status = {
            let mut thread_statuses = self.inner.thread_statuses.write().expect(LOCK_POISONED);
            let previous = thread_statuses.get(&session.id);
            let status = ThreadStatus {
                state: final_state,
                queue_depth: 0,
                current_turn_id: None,
                latest_sequence: previous.map_or(0, |previous| previous.latest_sequence),
                last_activity_at: previous.and_then(|previous| previous.last_activity_at),
                expires_at: previous.and_then(|previous| previous.expires_at),
            };
            thread_statuses.insert(session.id, status.clone());
            status
        };

        self.inner
            .events
            .publish(&ServerEvent::SessionUpserted(CodingSessionResponse::new(
                session,
                Some(status),
            )));
    }

    fn record_satellite_status(&self, status: SatelliteStatus) {
        let mut statuses = self.inner.satellite_statuses.write().expect(LOCK_POISONED);
        let previous = statuses.get(&status.satellite_id);
        if previous == Some(&status) {
            return;
        }

        if previous.is_none_or(|previous| previous.reachable != status.reachable) {
            event!(
                name: "fleet.satellite.reachability",
                Level::INFO,
                satellite.id = %status.satellite_id,
                satellite.reachable = status.reachable,
                error.message = status.error.as_deref().unwrap_or(""),
                "satellite reachability changed",
            );
        }
        statuses.insert(status.satellite_id, status.clone());
        drop(statuses);
        self.inner
            .events
            .publish(&ServerEvent::SatelliteStatus(status));
    }

    /// Follows a thread's events until the stream drops, the thread ends, or the
    /// watcher is cancelled. `resume_after` is the last sequence delivered.
    async fn follow_thread(
        &self,
        session: &CodingSession,
        resume_after: &mut Option<u64>,
        needs_resync: &mut bool,
        cancel: &CancellationToken,
    ) -> FollowOutcome {
        let interrupted = |reason: String| FollowOutcome::Interrupted {
            reason,
            delivered: false,
        };

        let client = match self.client(session.satellite_id).await {
            Ok(client) => client,
            Err(error) => return interrupted(error.to_string()),
        };
        let handle = match client.threads().attach(session.thread_id.as_str()).await {
            Ok(handle) => handle,
            Err(error) if error.is_gone() || error.is_not_found() => {
                // Only an explicit expiry reads as expired; a thread the satellite no
                // longer knows at all (its store was replaced) is as good as destroyed.
                let final_state = if error.code() == Some(ErrorCode::ThreadExpired) {
                    ThreadState::Expired
                } else {
                    ThreadState::Destroyed
                };
                return FollowOutcome::ThreadGone(final_state);
            }
            Err(error) => return interrupted(error.to_string()),
        };
        let from_sequence = match *resume_after {
            Some(sequence) => sequence,
            None => match handle.get().await {
                Ok(thread) => thread.latest_sequence,
                Err(error) => return interrupted(error.to_string()),
            },
        };
        let mut stream = match handle.events_from(from_sequence).await {
            Ok(stream) => stream,
            Err(error) => return interrupted(error.to_string()),
        };
        if *needs_resync {
            *needs_resync = false;
            self.inner
                .events
                .publish(&ServerEvent::SessionResync { id: session.id });
        }

        let mut delivered = false;
        loop {
            let next = tokio::select! {
                () = cancel.cancelled() => return FollowOutcome::Cancelled,
                next = stream.next() => next,
            };
            match next {
                Some(Ok(thread_event)) => {
                    delivered = true;
                    *resume_after = Some(thread_event.sequence);
                    let view = SessionEvent::new(session.id, thread_event);
                    self.inner
                        .events
                        .publish(&ServerEvent::SessionEvent(Box::new(view)));
                }
                Some(Err(error)) => {
                    return FollowOutcome::Interrupted {
                        reason: error.to_string(),
                        delivered,
                    };
                }
                None => {
                    return FollowOutcome::Interrupted {
                        reason: "the stream ended".to_owned(),
                        delivered,
                    };
                }
            }
        }
    }
}

async fn run_satellite_watcher(fleet: Fleet, satellite_id: Uuid, cancel: CancellationToken) {
    loop {
        tokio::select! {
            () = cancel.cancelled() => return,
            () = fleet.poll_satellite(satellite_id) => {}
        }
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(STATUS_POLL_INTERVAL) => {}
        }
    }
}

async fn run_session_watcher(fleet: Fleet, session: CodingSession, cancel: CancellationToken) {
    let mut resume_after = None;
    let mut needs_resync = false;
    let mut backoff = RECONNECT_BACKOFF_INITIAL;

    loop {
        match fleet
            .follow_thread(&session, &mut resume_after, &mut needs_resync, &cancel)
            .await
        {
            FollowOutcome::Cancelled => return,
            FollowOutcome::ThreadGone(final_state) => {
                event!(
                    name: "fleet.session.thread_gone",
                    Level::INFO,
                    session.id = %session.id,
                    "thread no longer exists; stopped following it",
                );
                fleet.record_thread_ended(session, final_state);
                return;
            }
            FollowOutcome::Interrupted { reason, delivered } => {
                event!(
                    name: "fleet.session.stream.interrupted",
                    Level::DEBUG,
                    session.id = %session.id,
                    error.message = %reason,
                    "thread stream interrupted; reconnecting",
                );
                if delivered {
                    backoff = RECONNECT_BACKOFF_INITIAL;
                } else if resume_after.is_some() {
                    // The resume point may have aged out of the satellite's retained
                    // history, which refuses the stream on every attempt. Tail from the
                    // latest event instead, and once that stream opens, tell clients to
                    // refetch what they missed.
                    resume_after = None;
                    needs_resync = true;
                }
            }
        }

        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(RECONNECT_BACKOFF_MAX);
    }
}

/// Connects to a satellite with its decrypted secret.
///
/// # Errors
/// Fails when the secret cannot be decrypted or the satellite refuses the connection.
pub async fn connect(
    satellite: &Satellite,
    cipher: &Cipher,
) -> Result<SatelliteClient, ClientError> {
    let secret = satellite
        .secret(cipher)
        .context("the satellite secret cannot be decrypted")?;
    Ok(SatelliteClient::connect(satellite.url.as_str(), secret.expose_secret()).await?)
}

fn unreachable(satellite_id: Uuid, error: &impl ToString) -> SatelliteStatus {
    SatelliteStatus {
        satellite_id,
        reachable: false,
        version: None,
        running_threads: None,
        max_concurrent_threads: None,
        error: Some(error.to_string()),
    }
}
