// Copyright © 2026 Jalapeno Labs

//! The engine's half of sessions: running the turns the API queued.
//!
//! [`TurnQueue::start`] looks at `session_turns` whenever [`Signal::SessionTurns`] arrives, and
//! every [`LOOK_INTERVAL`] in case one was lost. Each look:
//!
//! 1. cancels every turn it runs that someone asked to stop, or whose session was deleted;
//! 2. takes the oldest queued turn of every session with none running, so a session's turns run
//!    one after another and each resumes the Claude session the one before it reported;
//! 3. builds each one's request on the runner (its environment, tool grant, Blender, desktop)
//!    and follows it (`super::turn`) until it finishes, which marks the row finished and looks
//!    again for the session's next turn.
//!
//! Before its first look it settles what an earlier engine left: a turn whose CLI had started is
//! finished, since that engine's stop ended it, and one still waiting for a slot is queued again.
//! The tool grants those turns held are left to lapse (`crate::tools::grants`): every loop that
//! runs turns mints its own as the engine starts, and settling never touches them.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::Context;
use secrecy::{ExposeSecret, SecretString};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Level, event};
use uuid::Uuid;

use super::scrub::Scrubber;
use super::turn::Follower;
use super::{Sessions, desktop};
use crate::agent_models::{self, Agent};
use crate::models::coding_session::{self, CodingSession};
use crate::models::environment_variable::{self, TurnVariable};
use crate::models::session_turn::{self, SessionTurn};
use crate::models::storage_location;
use crate::passwords::Passwords;
use crate::realtime::Signal;
use crate::routes::v1::coding_sessions::github_token;
use crate::runner::{ConversationId, EnvironmentEntry, McpServer, Runner, TurnMode, TurnRequest};
use crate::storage::Storage;
use crate::tools::grants::Grants;
use crate::tools::{self, CallScope, SessionScope, ToolServer};

/// How often the queue is looked at without a signal. Signals are at most once, so this bounds
/// how long a turn whose signal was lost waits.
const LOOK_INTERVAL: Duration = Duration::from_secs(30);

/// The MCP server name every turn's Blender answers as.
pub(crate) const BLENDER_SERVER: &str = "blender";

/// The per-turn Blender launcher the workspace image installs (`workspace/blender`).
pub(crate) const BLENDER_COMMAND: &str = "elysium-blender-mcp";

/// Runs sessions' turns. Cheap to clone; every clone is the same queue.
#[derive(Debug, Clone)]
pub struct TurnQueue {
    pub(super) inner: Arc<Inner>,
}

pub(super) struct Inner {
    pub sessions: Sessions,
    pub runner: Runner,
    pub grants: Grants,
    pub storage: Storage,
    pub passwords: Passwords,
    /// The turns this engine runs, by id. Every one is on the runner under that same id.
    held: Mutex<HashSet<Uuid>>,
    /// The last sequence handed out per session, read from the database on first use. Only
    /// the engine records session events, and one engine runs.
    pub sequences: tokio::sync::Mutex<HashMap<i64, u64>>,
    tracker: TaskTracker,
}

impl std::fmt::Debug for Inner {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TurnQueue")
            .field("runner", &self.runner)
            .finish_non_exhaustive()
    }
}

/// What the queue is built from.
#[derive(Clone)]
pub struct TurnQueueDeps {
    pub sessions: Sessions,
    pub runner: Runner,
    pub grants: Grants,
    pub storage: Storage,
    /// The password vault: its tools are granted while it is ready, and every turn's output is
    /// scrubbed of what it handed out.
    pub passwords: Passwords,
}

impl std::fmt::Debug for TurnQueueDeps {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TurnQueueDeps")
            .field("runner", &self.runner)
            .finish_non_exhaustive()
    }
}

impl TurnQueue {
    pub fn new(deps: TurnQueueDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                sessions: deps.sessions,
                runner: deps.runner,
                grants: deps.grants,
                storage: deps.storage,
                passwords: deps.passwords,
                held: Mutex::new(HashSet::new()),
                sequences: tokio::sync::Mutex::new(HashMap::new()),
                tracker: TaskTracker::new(),
            }),
        }
    }

    /// Settles what an earlier engine left, then runs the queue until `shutdown`.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let queue = self.clone();
        let mut wake = self
            .inner
            .sessions
            .inner
            .events
            .wake_on(|signal| *signal == Signal::SessionTurns);
        tokio::spawn(async move {
            queue.settle_after_restart().await;
            loop {
                queue.look().await;
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    () = wake.woken() => {}
                    () = tokio::time::sleep(LOOK_INTERVAL) => {}
                }
            }
            event!(name: "sessions.queue.stopped", Level::INFO, "the turn queue stopped");
        })
    }

    /// Waits briefly for turn followers to record what their turns reported as they stop. The
    /// runner has already been told to stop every turn.
    pub async fn shutdown(&self) {
        self.inner.tracker.close();
        if tokio::time::timeout(Duration::from_secs(10), self.inner.tracker.wait())
            .await
            .is_err()
        {
            event!(
                name: "sessions.shutdown.timeout",
                Level::WARN,
                "turn followers were still recording when the grace period ended",
            );
        }
    }

    /// Forgets turn `turn_id` once its follower finished it.
    pub(super) fn release(&self, turn_id: Uuid) {
        lock(&self.inner.held).remove(&turn_id);
    }

    async fn settle_after_restart(&self) {
        let settled = async {
            let mut connection = self.inner.sessions.inner.database.get().await?;
            anyhow::Ok(session_turn::settle_after_restart(&mut connection).await?)
        };
        match settled.await {
            Ok(sessions) => {
                for session_id in sessions {
                    self.inner.sessions.announce(session_id).await;
                }
            }
            Err(error) => event!(
                name: "sessions.queue.settle_failure",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "could not settle the turns an earlier engine left; they wait until it can",
            ),
        }
    }

    /// One look at the queue: cancels what should stop, then starts what may start.
    async fn look(&self) {
        if let Err(error) = self.cancel_unwanted().await {
            event!(
                name: "sessions.queue.cancel_failure",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "could not check which turns were asked to stop; looking again shortly",
            );
        }
        let taken = async {
            let mut connection = self.inner.sessions.inner.database.get().await?;
            anyhow::Ok(session_turn::take_next(&mut connection).await?)
        };
        let turns = match taken.await {
            Ok(turns) => turns,
            Err(error) => {
                event!(
                    name: "sessions.queue.take_failure",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "could not take queued turns; looking again shortly",
                );
                return;
            }
        };
        for turn in turns {
            let turn_id = turn.id;
            let session_id = turn.session_id;
            if let Err(error) = self.run(turn).await {
                event!(
                    name: "sessions.turn.start_failure",
                    Level::ERROR,
                    session.id = session_id,
                    turn.id = %turn_id,
                    error.message = %format!("{error:#}"),
                    "a queued turn could not start; it is finished without running",
                );
                self.abandon(session_id, turn_id).await;
            }
        }
    }

    /// Cancels every turn this engine runs that was asked to stop, or whose session is gone.
    async fn cancel_unwanted(&self) -> anyhow::Result<()> {
        let held: Vec<Uuid> = lock(&self.inner.held).iter().copied().collect();
        if held.is_empty() {
            return Ok(());
        }
        let mut connection = self.inner.sessions.inner.database.get().await?;
        for turn_id in session_turn::to_cancel(&mut connection, &held).await? {
            self.inner.runner.cancel(turn_id);
        }
        Ok(())
    }

    /// Builds `turn`'s request, submits it to the runner, and follows it.
    async fn run(&self, turn: SessionTurn) -> anyhow::Result<()> {
        let sessions = &self.inner.sessions;
        let mut connection = sessions.inner.database.get().await?;
        let session = coding_session::find(&mut connection, turn.session_id)
            .await
            .context("cannot read the turn's session")?;
        let variables =
            environment_variable::turn_environment(&mut connection, &sessions.inner.cipher)
                .await
                .context("cannot read the workspace's environment variables")?;
        let github = github_token::open_for_session(
            &mut connection,
            &sessions.inner.cipher,
            session.github_credential_id,
        )
        .await
        .map_err(|error| anyhow::anyhow!("cannot open the session's GitHub token: {error}"))?;
        let has_storage = !storage_location::list_for_project(&mut connection, session.project_id)
            .await
            .context("cannot read the project's storage locations")?
            .is_empty();
        // Read as the turn starts, so a change applies from the session's next turn.
        let model =
            agent_models::for_agent(&mut connection, agent_of(&session), session.project_id)
                .await
                .context("cannot read the session's model setting")?
                .choice;
        drop(connection);

        let (environment, secrets) = turn_environment(variables, github.as_ref());
        let directory = Sessions::directory(session.id);
        let grant = self
            .inner
            .grants
            .grant(
                &CallScope::Session(SessionScope {
                    session_id: session.id,
                    project_id: session.project_id,
                    action_item_id: session.action_item_id,
                    directory: directory.clone(),
                }),
                &tool_servers(ToolAccess {
                    has_project: session.project_id.is_some(),
                    has_storage,
                    has_passwords: self.inner.passwords.is_ready(),
                }),
            )
            .await?;
        let mut mcp_servers = grant.mcp_servers();
        mcp_servers.push(McpServer::Stdio {
            name: BLENDER_SERVER.to_owned(),
            command: BLENDER_COMMAND.to_owned(),
            arguments: Vec::new(),
            environment: Vec::new(),
        });
        mcp_servers.push(desktop::server(session.id));

        let request = TurnRequest {
            conversation: ConversationId::for_session(session.id),
            label: format!("session:{}", session.id),
            prompt: turn.prompt,
            working_directory: directory,
            resume: resume_of(&session),
            mcp_servers,
            allowed_tools: None,
            system_prompt_append: turn.system_prompt_append,
            environment,
            model: Some(model.model),
            effort: Some(model.effort),
            mode: TurnMode::Agent,
        };
        let ticket = self
            .inner
            .runner
            .submit_as(turn.id, request)
            .map_err(|_shutting_down| anyhow::anyhow!("the engine is shutting down"))?;
        lock(&self.inner.held).insert(turn.id);

        let follower = Follower {
            queue: self.clone(),
            session_id: session.id,
            studio_item_id: session.studio_item_id,
            project_id: session.project_id,
            turn_id: turn.id,
            scrubber: Scrubber::new(secrets).with_revealed(self.inner.passwords.revealed()),
            grant,
        };
        self.inner
            .tracker
            .spawn(async move { follower.follow(ticket.events).await });
        Ok(())
    }

    /// Finishes a turn that could not start, so its session's next turn is not held behind it.
    async fn abandon(&self, session_id: i64, turn_id: Uuid) {
        let finished = async {
            let mut connection = self.inner.sessions.inner.database.get().await?;
            anyhow::Ok(session_turn::finish(&mut connection, turn_id).await?)
        };
        if let Err(error) = finished.await {
            event!(
                name: "sessions.turn.abandon_failure",
                Level::ERROR,
                turn.id = %turn_id,
                error.message = %format!("{error:#}"),
                "could not finish a turn that never started; the next engine start does",
            );
        }
        self.inner.sessions.announce(session_id).await;
    }
}

/// The Claude session a session's next turn resumes: the one its last turn reported.
fn resume_of(session: &CodingSession) -> Option<crate::runner::HarnessSessionId> {
    session
        .harness_session_id
        .as_deref()
        .and_then(|id| id.parse().ok())
}

/// A turn's environment: every workspace variable, then the session's GitHub token with the
/// git configuration that routes git through `gh`. Also answers the secrets among them, which
/// the turn's output is scrubbed of.
pub(crate) fn turn_environment(
    variables: Vec<TurnVariable>,
    github: Option<&SecretString>,
) -> (Vec<EnvironmentEntry>, Vec<SecretString>) {
    let mut secrets = Vec::new();
    let mut environment = Vec::with_capacity(variables.len() + 6);
    for variable in variables {
        if variable.is_secret {
            secrets.push(SecretString::from(
                variable.value.expose_secret().to_owned(),
            ));
        }
        environment.push(EnvironmentEntry {
            key: variable.key,
            value: variable.value,
        });
    }
    if let Some(token) = github {
        environment.extend(github_token::environment(token));
        secrets.push(SecretString::from(token.expose_secret().to_owned()));
    }
    (environment, secrets)
}

/// The agent whose model setting `session`'s turns run on: a Studio item's session makes one
/// asset, which an admin may want on another model than multi-repository coding work.
fn agent_of(session: &CodingSession) -> Agent {
    if session.studio_item_id.is_some() {
        return Agent::StudioSession;
    }
    Agent::CodingSession
}

/// Which of Elysium's tool servers a turn may use.
#[derive(Debug, Clone, Copy)]
struct ToolAccess {
    /// Work tools need a project to scope items and initiatives to.
    has_project: bool,
    /// Storage tools need a location the project may use.
    has_storage: bool,
    /// Password tools need a vault that is up; one set up mid-turn is granted from the next.
    has_passwords: bool,
}

/// The tool servers a turn may call: the knowledge base's always, the work tools when the
/// session has a project, the storage tools when that project reaches a storage location, and
/// the password tools while the vault is up.
fn tool_servers(access: ToolAccess) -> Vec<&'static ToolServer> {
    let mut servers = Vec::with_capacity(4);
    servers.push(&tools::knowledge::SERVER);
    if access.has_project {
        servers.push(&tools::work::SERVER);
    }
    if access.has_storage {
        servers.push(&tools::storage::SERVER);
    }
    if access.has_passwords {
        servers.push(&tools::passwords::SERVER);
    }
    servers
}

/// Every critical section leaves the map whole, so a poisoned lock holds nothing half done.
fn lock<Value>(mutex: &Mutex<Value>) -> std::sync::MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_studio_items_session_runs_on_studios_setting_and_every_other_on_codings() {
        let mut session = CodingSession {
            id: 12,
            title: "Retry the uploads".to_owned(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            project_id: Some(Uuid::now_v7()),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
            created_by: Uuid::now_v7(),
            harness_session_id: None,
        };
        assert_eq!(agent_of(&session), Agent::CodingSession);

        session.studio_item_id = Some(Uuid::now_v7());
        assert_eq!(agent_of(&session), Agent::StudioSession);
    }

    #[test]
    fn each_tool_server_is_granted_when_what_it_needs_is_there() {
        let names = |has_project, has_storage, has_passwords| -> Vec<&'static str> {
            tool_servers(ToolAccess {
                has_project,
                has_storage,
                has_passwords,
            })
            .iter()
            .map(|server| server.name)
            .collect()
        };
        assert_eq!(
            names(true, true, true),
            [
                "elysium_knowledge",
                "elysium_work",
                "elysium_storage",
                "elysium_passwords"
            ]
        );
        assert_eq!(
            names(true, false, false),
            ["elysium_knowledge", "elysium_work"]
        );
        assert_eq!(
            names(false, false, false),
            ["elysium_knowledge"],
            "every turn reads the company doc"
        );
        assert_eq!(
            names(false, true, false),
            ["elysium_knowledge", "elysium_storage"]
        );
        assert_eq!(
            names(false, false, true),
            ["elysium_knowledge", "elysium_passwords"],
            "a session without a project still signs in to things"
        );
    }
}
