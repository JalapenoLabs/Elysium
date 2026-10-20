// Copyright © 2026 Jalapeno Labs

//! Runs agent turns: the official `claude` CLI, headless, in Elysium's workspace container.
//!
//! Everything that asks an agent to work (a coding session, a Studio item, Elysia) submits a
//! [`TurnRequest`] and reads the turn's [`RunnerEvent`]s from the [`TurnTicket`] it gets back.
//! The runner owns how a turn runs and nothing about what it means: it keeps no history, so
//! each caller records the events and the [`HarnessSessionId`] it needs, and passes that id
//! back as the next turn's `resume` to continue the conversation.
//!
//! - **Where.** Every turn is a `docker exec` in the workspace container
//!   ([`workspace`]), never in the API's own container, as the [`workspace::AGENT_USER`]
//!   account. `/workspace` is a volume, so repositories, sessions, and Claude's own
//!   transcripts outlive the container. `~/.claude` is `/workspace/.claude`, fetched from the
//!   configuration repository the agent settings name ([`configuration`]).
//! - **Order.** Turns of one conversation run one after another, in the order submitted;
//!   turns of different conversations run side by side, at most [`MAX_CONCURRENT_TURNS`] at
//!   once.
//! - **Credentials.** A turn runs on the highest priority usable Claude credential. One that
//!   is spent or refused hands the turn to the next, which resumes the same Claude session
//!   ([`turn`]). Only when every one is spent does the turn finish as usage limited.
//! - **Liveness.** A turn that prints nothing for [`HEARTBEAT_TIMEOUT`] is killed and finishes
//!   interrupted, as does every turn when Elysium shuts down. Turns a previous run of the API
//!   left behind are killed when the workspace is first prepared.
//! - **Every turn finishes.** Its events end with exactly one [`RunnerEvent::Finished`], also
//!   when it fails before it starts.
//! - **Busy.** A running turn holds an activity (`crate::activity`), so auto-update never
//!   restarts the stack under it.
//!
//! Only the official CLI runs, with the credential in its environment. The Agent SDK is
//! never driven with subscription credentials.

pub mod command;
pub mod configuration;
pub mod credentials;
pub mod events;
#[cfg(test)]
mod live_tests;
mod turn;
pub mod workspace;

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Level, event};
use url::Url;
use uuid::Uuid;

use self::configuration::ConfigurationSource;
use self::credentials::ClaudeCredential;
pub use self::events::{HarnessSessionId, RunnerEvent, TurnOutcome, TurnStatus};
use self::workspace::WorkspaceContainer;
use crate::activity::Activities;
use crate::crypto::Cipher;
use crate::database::Pool;

/// Where an agent in the workspace reaches the API: the API's own listener, on the
/// `elysium-agents` network only the API and the workspace join. nginx is not on it, so
/// nothing from the public side can reach it.
pub const AGENT_API_URL: &str = "http://api:8080";

/// The workspace's volume, which every [`WorkspacePath`] is relative to.
pub const WORKSPACE_ROOT: &str = "/workspace";

/// Where each running turn keeps its process id and MCP configuration, removed as it ends.
const TURNS_DIRECTORY: &str = "/workspace/.elysium/turns";

/// Most turns running at once across every conversation. Each is a `claude` process with
/// its own tools; past this many a shared host slows every one of them. Further turns wait
/// for a slot in the order they were submitted.
pub const MAX_CONCURRENT_TURNS: usize = 8;

/// How long a turn may go without printing anything before it is presumed dead. A model
/// call or a quiet build can take minutes; twenty without one line is a hang.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_mins(20);

/// How many events a ticket buffers before a slow reader holds the turn back.
const EVENT_BUFFER: usize = 1_024;

/// How long shutdown waits for turns to stop after killing them.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(15);

/// Names one conversation: the run of turns that share a Claude session and queue behind
/// each other. The caller chooses and keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConversationId(Uuid);

impl ConversationId {
    pub const fn new(id: Uuid) -> Self {
        Self(id)
    }
}

impl fmt::Display for ConversationId {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A directory inside the workspace, relative to [`WORKSPACE_ROOT`], such as `sessions/12`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePath(String);

/// A [`WorkspacePath`] was absolute, empty, or tried to leave the workspace.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a directory inside the workspace")]
pub struct InvalidWorkspacePath(String);

impl WorkspacePath {
    /// Accepts a relative path of plain components.
    ///
    /// # Errors
    /// Refuses an empty or absolute path, and any `.` or `..` component, so no path names a
    /// directory outside the workspace.
    pub fn new(relative: impl Into<String>) -> Result<Self, InvalidWorkspacePath> {
        let relative = relative.into();
        let is_valid = !relative.is_empty()
            && !relative.starts_with('/')
            && relative
                .split('/')
                .all(|component| !matches!(component, "" | "." | ".."));
        if !is_valid {
            return Err(InvalidWorkspacePath(relative));
        }
        Ok(Self(relative))
    }

    /// The path as the workspace sees it.
    pub fn absolute(&self) -> String {
        format!("{WORKSPACE_ROOT}/{}", self.0)
    }
}

/// An MCP server a turn's agent can use.
#[derive(Debug, Clone)]
pub enum McpServer {
    /// Streamable HTTP, such as an Elysium route at [`AGENT_API_URL`].
    Http {
        name: String,
        url: Url,
        /// Sent with every request, such as `Authorization`.
        headers: Vec<(String, SecretString)>,
    },
    /// A program the CLI starts inside the workspace for the turn.
    Stdio {
        name: String,
        command: String,
        arguments: Vec<String>,
        environment: Vec<EnvironmentEntry>,
    },
}

/// One environment variable. The value is kept secret because most are.
#[derive(Debug, Clone)]
pub struct EnvironmentEntry {
    pub key: String,
    pub value: SecretString,
}

/// A turn to run.
#[derive(Debug, Clone)]
pub struct TurnRequest {
    pub conversation: ConversationId,
    /// Names the turn in logs, such as `session:12`.
    pub label: String,
    pub prompt: String,
    /// Created if it does not exist.
    pub working_directory: WorkspacePath,
    /// The Claude session to continue; `None` starts a new one.
    pub resume: Option<HarnessSessionId>,
    pub mcp_servers: Vec<McpServer>,
    /// `None` allows every tool.
    pub allowed_tools: Option<Vec<String>>,
    pub system_prompt_append: Option<String>,
    /// For this turn only, beneath the credential Elysium sets.
    pub environment: Vec<EnvironmentEntry>,
    /// `None` leaves the choice to Claude Code and its settings.
    pub model: Option<String>,
}

/// A submitted turn.
#[derive(Debug)]
pub struct TurnTicket {
    pub turn_id: Uuid,
    /// Ends after exactly one [`RunnerEvent::Finished`].
    pub events: mpsc::Receiver<RunnerEvent>,
}

/// The runner is shutting down and takes no more turns.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("the runner is shutting down")]
pub struct ShuttingDown;

/// Runs agent turns. Cheap to clone; every clone is the same runner.
#[derive(Debug, Clone)]
pub struct Runner {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    executor: Executor,
    credentials: CredentialSource,
    configuration: ConfigurationSource,
    /// Where each running turn says the stack is busy.
    activity: Activities,
    shutdown: CancellationToken,
    slots: Semaphore,
    tracker: TaskTracker,
    /// Turns waiting, per conversation. A conversation has an entry exactly while a worker
    /// is draining it.
    queues: Mutex<HashMap<ConversationId, VecDeque<QueuedTurn>>>,
    /// Every turn not yet finished, queued or running, so it can be cancelled.
    cancellations: Mutex<HashMap<Uuid, CancellationToken>>,
    /// Whether the workspace is up, old turns are reaped, and `~/.claude` is fetched. Held
    /// while preparing, so concurrent first turns prepare once.
    prepared: tokio::sync::Mutex<bool>,
}

#[derive(Debug)]
struct QueuedTurn {
    turn_id: Uuid,
    request: TurnRequest,
    events: mpsc::Sender<RunnerEvent>,
    cancellation: CancellationToken,
}

/// Where turns run. Tests run against a scripted workspace instead of Docker.
#[derive(Debug)]
enum Executor {
    Workspace(WorkspaceContainer),
    #[cfg(test)]
    Fake(turn::fake::FakeWorkspace),
}

/// Where a turn's credentials come from.
enum CredentialSource {
    Database {
        database: Pool,
        cipher: Arc<Cipher>,
    },
    #[cfg(test)]
    Fixed(Vec<ClaudeCredential>),
}

impl fmt::Debug for CredentialSource {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database { .. } => formatter.write_str("CredentialSource::Database"),
            #[cfg(test)]
            Self::Fixed(credentials) => formatter
                .debug_tuple("CredentialSource::Fixed")
                .field(&credentials.len())
                .finish(),
        }
    }
}

impl CredentialSource {
    async fn load(&self) -> anyhow::Result<Vec<ClaudeCredential>> {
        match self {
            Self::Database { database, cipher } => {
                let mut connection = database.get().await?;
                Ok(credentials::load(&mut connection, cipher, chrono::Utc::now()).await?)
            }
            #[cfg(test)]
            Self::Fixed(credentials) => Ok(credentials.clone()),
        }
    }
}

impl Runner {
    /// A runner whose turns run in `workspace`, with credentials and the configuration
    /// repository read from `database`. Nothing reaches Docker until [`Self::prepare`] or the
    /// first turn.
    pub fn new(
        workspace: WorkspaceContainer,
        database: Pool,
        cipher: Arc<Cipher>,
        activity: Activities,
        shutdown: &CancellationToken,
    ) -> Self {
        Self::with_parts(
            Executor::Workspace(workspace),
            CredentialSource::Database {
                database: database.clone(),
                cipher: Arc::clone(&cipher),
            },
            ConfigurationSource::Database { database, cipher },
            activity,
            shutdown.child_token(),
        )
    }

    fn with_parts(
        executor: Executor,
        credentials: CredentialSource,
        configuration: ConfigurationSource,
        activity: Activities,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                executor,
                credentials,
                configuration,
                activity,
                shutdown,
                slots: Semaphore::new(MAX_CONCURRENT_TURNS),
                tracker: TaskTracker::new(),
                queues: Mutex::new(HashMap::new()),
                cancellations: Mutex::new(HashMap::new()),
                prepared: tokio::sync::Mutex::new(false),
            }),
        }
    }

    /// Queues `request` behind any unfinished turn of its conversation.
    ///
    /// # Errors
    /// [`ShuttingDown`] once Elysium has begun to stop.
    pub fn submit(&self, request: TurnRequest) -> Result<TurnTicket, ShuttingDown> {
        if self.inner.shutdown.is_cancelled() {
            return Err(ShuttingDown);
        }
        let turn_id = Uuid::now_v7();
        let (sender, receiver) = mpsc::channel(EVENT_BUFFER);
        let cancellation = self.inner.shutdown.child_token();
        lock(&self.inner.cancellations).insert(turn_id, cancellation.clone());

        let conversation = request.conversation;
        let turn = QueuedTurn {
            turn_id,
            request,
            events: sender,
            cancellation,
        };
        let starts_worker = {
            let mut queues = lock(&self.inner.queues);
            if let Some(behind) = queues.get_mut(&conversation) {
                behind.push_back(turn);
                false
            } else {
                queues.insert(conversation, VecDeque::from([turn]));
                true
            }
        };
        if starts_worker {
            let runner = self.clone();
            self.inner
                .tracker
                .spawn(async move { runner.drain(conversation).await });
        }
        Ok(TurnTicket {
            turn_id,
            events: receiver,
        })
    }

    /// Cancels a queued or running turn. It finishes as cancelled; `false` means no such
    /// turn is unfinished.
    pub fn cancel(&self, turn_id: Uuid) -> bool {
        let Some(cancellation) = lock(&self.inner.cancellations).get(&turn_id).cloned() else {
            event!(
                name: "runner.turn.cancel_unknown",
                Level::DEBUG,
                turn.id = %turn_id,
                "asked to cancel a turn that is not queued or running",
            );
            return false;
        };
        cancellation.cancel();
        true
    }

    /// Brings the workspace up, kills turns a previous run left behind, and fetches
    /// `~/.claude`. The first turn does this itself; calling it at startup moves an image
    /// pull out of that turn's way. Once it has succeeded it does nothing.
    ///
    /// # Errors
    /// Describes what failed, for the turn that needed it.
    pub async fn prepare(&self) -> anyhow::Result<()> {
        let mut prepared = self.inner.prepared.lock().await;
        if *prepared {
            return Ok(());
        }
        self.inner.executor.prepare().await?;
        let repository = self.inner.configuration.load().await?;
        self.inner
            .executor
            .provision_configuration(repository)
            .await?;
        *prepared = true;
        event!(name: "runner.workspace.ready", Level::INFO, "the workspace is ready for turns");
        Ok(())
    }

    /// Fetches `~/.claude` again, after the configuration repository was changed. Running
    /// turns keep the files they started with until they read them again.
    ///
    /// # Errors
    /// Describes what failed.
    pub async fn refresh_configuration(&self) -> anyhow::Result<()> {
        let prepared = self.inner.prepared.lock().await;
        if !*prepared {
            // The next turn prepares, which fetches the new repository.
            return Ok(());
        }
        let repository = self.inner.configuration.load().await?;
        self.inner
            .executor
            .provision_configuration(repository)
            .await
    }

    /// Kills every turn and waits briefly for them to report it. Called once, by the server
    /// as it stops.
    pub async fn shutdown(&self) {
        self.inner.shutdown.cancel();
        self.inner.tracker.close();
        if tokio::time::timeout(SHUTDOWN_GRACE, self.inner.tracker.wait())
            .await
            .is_err()
        {
            event!(
                name: "runner.shutdown.timeout",
                Level::WARN,
                "turns were still stopping when the grace period ended",
            );
        }
    }

    /// Runs `conversation`'s turns in order until none are left.
    async fn drain(&self, conversation: ConversationId) {
        loop {
            let next = {
                let mut queues = lock(&self.inner.queues);
                let next = queues.get_mut(&conversation).and_then(VecDeque::pop_front);
                if next.is_none() {
                    queues.remove(&conversation);
                }
                next
            };
            let Some(queued) = next else {
                return;
            };
            let turn_id = queued.turn_id;
            turn::run(self, queued).await;
            lock(&self.inner.cancellations).remove(&turn_id);
        }
    }
}

impl Executor {
    async fn prepare(&self) -> anyhow::Result<()> {
        match self {
            Self::Workspace(workspace) => {
                workspace.ensure_running().await?;
                let reaped = workspace
                    .run_script(
                        workspace::AGENT_USER,
                        command::REAP_SCRIPT,
                        &[TURNS_DIRECTORY],
                        Vec::new(),
                    )
                    .await?;
                if reaped.exit_code != Some(0) {
                    anyhow::bail!("could not clear old turns: {}", reaped.output);
                }
                Ok(())
            }
            #[cfg(test)]
            Self::Fake(fake) => fake.prepare(),
        }
    }

    async fn provision_configuration(
        &self,
        repository: Option<configuration::ConfigurationRepository>,
    ) -> anyhow::Result<()> {
        match self {
            Self::Workspace(workspace) => configuration::provision(workspace, repository).await,
            #[cfg(test)]
            Self::Fake(_fake) => Ok(()),
        }
    }
}

/// Locks `mutex`, carrying on past a panic elsewhere: every critical section here leaves
/// the maps consistent, so a poisoned lock holds nothing half done.
fn lock<Value>(mutex: &Mutex<Value>) -> std::sync::MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_paths_stay_inside_the_workspace() {
        let path = WorkspacePath::new("sessions/12/repos").expect("a relative path");
        assert_eq!(path.absolute(), "/workspace/sessions/12/repos");
        for bad in [
            "",
            "/etc",
            "sessions/../..",
            "./sessions",
            "sessions//12",
            "sessions/",
        ] {
            assert!(WorkspacePath::new(bad).is_err(), "{bad:?} is refused");
        }
    }
}
