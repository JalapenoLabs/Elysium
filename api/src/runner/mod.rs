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
//!   configuration repository the agent settings name ([`Runner::fetch_configuration`]).
//!   Whoever runs the runner decides when: the engine's keeper (`crate::agent_configuration`)
//!   for the shared workspace, and each worker for its own container.
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
pub mod files;
#[cfg(test)]
mod live_tests;
pub mod socket;
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

use self::configuration::{ConfigurationSource, Fetched};
#[cfg(test)]
use self::credentials::ClaudeCredential;
use self::credentials::LoadedCredentials;
pub use self::events::{HarnessSessionId, RunnerEvent, TurnOutcome, TurnStatus};
use self::workspace::WorkspaceContainer;
use crate::activity::Activities;
use crate::claude_accounts::TurnSignal;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::model_setting::Effort;

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
pub struct ConversationId(Conversation);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Conversation {
    /// A conversation its owner names, such as Elysia's.
    Named(Uuid),
    /// A coding or Studio session's, named by the session's number.
    Session(i64),
}

impl ConversationId {
    pub const fn new(id: Uuid) -> Self {
        Self(Conversation::Named(id))
    }

    /// The conversation of coding session `session_id`.
    pub const fn for_session(session_id: i64) -> Self {
        Self(Conversation::Session(session_id))
    }
}

impl fmt::Display for ConversationId {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Conversation::Named(id) => id.fmt(formatter),
            Conversation::Session(session_id) => write!(formatter, "session-{session_id}"),
        }
    }
}

/// A directory inside the workspace, relative to [`WORKSPACE_ROOT`], such as `sessions/12`.
/// Serialized as the relative path, and checked again when read back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct WorkspacePath(String);

impl From<WorkspacePath> for String {
    fn from(path: WorkspacePath) -> Self {
        path.0
    }
}

impl TryFrom<String> for WorkspacePath {
    type Error = InvalidWorkspacePath;

    fn try_from(relative: String) -> Result<Self, Self::Error> {
        Self::new(relative)
    }
}

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
    /// `None` leaves the choice to Claude Code and its settings. Every agent's turn passes the
    /// model and effort its setting names, resolved by `crate::agent_models`.
    pub model: Option<String>,
    /// How hard the model thinks; `None` leaves it to Claude Code and its settings.
    pub effort: Option<Effort>,
    pub mode: TurnMode,
}

/// What kind of Claude Code a turn runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnMode {
    /// An agent: every built-in tool, the workspace's `~/.claude` (settings, skills, CLAUDE.md),
    /// and a session the next turn can resume.
    Agent,
    /// Text in, text out, for work such as summarizing a turn: no built-in tools, no settings
    /// or MCP servers beyond the request's own, `system_prompt` in place of Claude Code's,
    /// and nothing kept to resume. An agent's configuration must not steer it, and a session
    /// nobody resumes would only fill the transcripts directory.
    Text { system_prompt: String },
    /// A text turn whose session is kept, so the next turn of the same conversation resumes it
    /// (`TurnRequest::resume`): no built-in tools, no settings or MCP servers beyond the request's
    /// own, `system_prompt` in place of Claude Code's, as for [`Self::Text`]. For an agent that
    /// hears one conversation over many turns, such as the one handling a person's messages, and
    /// answers each in text. Its session is found again by the turn's working directory, so every
    /// turn of one session runs in the same one.
    ResumableText { system_prompt: String },
}

/// A submitted turn.
#[derive(Debug)]
pub struct TurnTicket {
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
        /// Tells the Claude account keeper a turn finished, so it reads usage then.
        turn_signal: TurnSignal,
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
    async fn load(&self) -> anyhow::Result<LoadedCredentials> {
        match self {
            Self::Database {
                database, cipher, ..
            } => {
                let mut connection = database.get().await?;
                Ok(credentials::load(&mut connection, cipher, chrono::Utc::now()).await?)
            }
            #[cfg(test)]
            Self::Fixed(credentials) => Ok(LoadedCredentials {
                usable: credentials.clone(),
                limited_until: None,
            }),
        }
    }

    fn turn_finished(&self) {
        match self {
            Self::Database { turn_signal, .. } => turn_signal.turn_finished(),
            #[cfg(test)]
            Self::Fixed(_credentials) => {}
        }
    }
}

impl Runner {
    /// A runner whose turns run in `workspace`, with credentials and the configuration
    /// repository read from `database`. Each finished turn rings `turn_signal`. Nothing reaches
    /// Docker until [`Self::prepare`] or the first turn.
    pub fn new(
        workspace: WorkspaceContainer,
        database: Pool,
        cipher: Arc<Cipher>,
        activity: Activities,
        turn_signal: TurnSignal,
        shutdown: &CancellationToken,
    ) -> Self {
        Self::with_parts(
            Executor::Workspace(workspace),
            CredentialSource::Database {
                database: database.clone(),
                cipher: Arc::clone(&cipher),
                turn_signal,
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
        self.submit_as(Uuid::now_v7(), request)
    }

    /// [`Runner::submit`] under the id `turn_id`, for a caller whose turn already has one,
    /// such as a session's queued turn: its events, its cancellation, and the runner's logs
    /// then all name the same turn.
    ///
    /// # Errors
    /// [`ShuttingDown`] once Elysium has begun to stop.
    pub fn submit_as(
        &self,
        turn_id: Uuid,
        request: TurnRequest,
    ) -> Result<TurnTicket, ShuttingDown> {
        if self.inner.shutdown.is_cancelled() {
            return Err(ShuttingDown);
        }
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
        Ok(TurnTicket { events: receiver })
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

    /// Brings the workspace up and kills turns a previous run left behind. The first turn does
    /// this itself; calling it at startup moves an image pull out of that turn's way. Once it
    /// has succeeded it does nothing. It never fetches `~/.claude`, which
    /// [`Self::fetch_configuration`] does when its caller decides.
    ///
    /// # Errors
    /// Describes what failed, for the turn that needed it.
    pub async fn prepare(&self) -> anyhow::Result<()> {
        let mut prepared = self.inner.prepared.lock().await;
        if *prepared {
            return Ok(());
        }
        self.inner.executor.prepare().await?;
        *prepared = true;
        event!(name: "runner.workspace.ready", Level::INFO, "the workspace is ready for turns");
        Ok(())
    }

    /// Fetches `~/.claude` from the configuration repository the settings name, with its own
    /// token, into the workspace, preparing the workspace first. Running turns keep the files
    /// they started with until they read them again. A token that cannot be opened, a workspace
    /// that does not come up, a fetch git refuses, and a checkout that cannot be adapted are all
    /// the attempt's outcome, so its caller records every one of them.
    ///
    /// # Errors
    /// Propagates a database error reading the settings.
    pub async fn fetch_configuration(&self) -> anyhow::Result<Fetched> {
        let Some(loaded) = self.inner.configuration.load().await? else {
            event!(
                name: "runner.configuration.unset",
                Level::INFO,
                "no configuration repository is set; nothing to fetch",
            );
            return Ok(Fetched::Unset);
        };
        let outcome = match loaded.repository {
            Ok(repository) => async {
                self.prepare().await?;
                self.inner
                    .executor
                    .provision_configuration(&repository)
                    .await
            }
            .await
            .map_err(|error| format!("{error:#}")),
            Err(error) => Err(error),
        };
        Ok(Fetched::Attempted {
            source: loaded.source,
            outcome,
        })
    }

    /// The commit of the configuration repository checked out in the workspace, if any.
    ///
    /// # Errors
    /// Propagates Docker's refusal.
    pub async fn configuration_commit(&self) -> anyhow::Result<Option<String>> {
        match &self.inner.executor {
            Executor::Workspace(workspace) => configuration::checked_out(workspace).await,
            #[cfg(test)]
            Executor::Fake(_fake) => Ok(None),
        }
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
                        workspace::SCRIPT_TIMEOUT,
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
        repository: &configuration::ConfigurationRepository,
    ) -> anyhow::Result<configuration::Checkout> {
        match self {
            Self::Workspace(workspace) => configuration::provision(workspace, repository).await,
            #[cfg(test)]
            Self::Fake(_fake) => anyhow::bail!("the fake workspace fetches no configuration"),
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
