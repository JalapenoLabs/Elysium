// Copyright © 2026 Jalapeno Labs

//! Coding and Studio sessions, run on the runner.
//!
//! A session is a directory in the workspace (`sessions/<id>`), the Claude session its turns
//! continue, and the events its turns reported. This service is everything between a session's
//! row and the runner:
//!
//! - **Preparing** a new session's directory: cloning its repositories into `repos/<name>`.
//! - **Starting a turn** ([`Sessions::start_turn`]): the prompt, the session's environment
//!   (workspace variables, then the GitHub token and the git config that uses it), the tool
//!   servers it may call under a per-turn grant (`crate::tools::grants`), a Blender of its own,
//!   and the Claude session to resume.
//! - **Following the turn** ([`turn`]): every event is scrubbed of the turn's secrets,
//!   numbered, recorded in `session_events`, and published, in that order. When the turn ends,
//!   the Claude session id it reported is kept on the row, a Studio session's deliverables are
//!   collected, and the grant is dropped.
//! - **What a session is doing now** ([`Sessions::activity`]), kept in memory and published as
//!   it changes.
//! - **Removing** a session's turns and directory when it is deleted.
//!
//! Nothing here matches on whether a session is Coding or Studio beyond the instructions
//! Studio's agents get and the collection after a Studio turn.

pub mod scrub;
mod turn;
pub mod views;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use tokio_util::task::TaskTracker;
use tracing::{Level, event};
use uuid::Uuid;

use self::scrub::Scrubber;
use self::views::{ActivityState, SessionActivity, TurnState, TurnView};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::coding_session::CodingSession;
use crate::models::{environment_variable, storage_location};
use crate::realtime::EventBus;
use crate::routes::v1::coding_sessions::github_token;
use crate::runner::workspace::{AGENT_USER, WorkspaceContainer};
use crate::runner::{
    ConversationId, EnvironmentEntry, McpServer, Runner, TurnRequest, WorkspacePath,
};
use crate::storage::Storage;
use crate::tools::grants::Grants;
use crate::tools::{self, CallScope, SessionScope, ToolServer};

/// The MCP server name every turn's Blender answers as.
const BLENDER_SERVER: &str = "blender";

/// The per-turn Blender launcher the workspace image installs (`workspace/blender`).
const BLENDER_COMMAND: &str = "elysium-blender-mcp";

/// How long cloning a session's repositories may take, all of them together. A large monorepo
/// over a slow link takes minutes; past this the session is refused rather than left half
/// cloned.
const CLONE_TIMEOUT: Duration = Duration::from_mins(30);

/// Clones one repository. `$1` is the session's directory, `$2` the directory name under
/// `repos/`, `$3` the URL, and `$4` the branch, or empty for the remote's default. The
/// session's environment is the exec's, so `gh` authenticates github.com.
const CLONE_SCRIPT: &str = r#"
set -u
mkdir -p -- "$1/repos" || exit 70
cd -- "$1/repos" || exit 70
if [ -n "$4" ]; then
  git clone --quiet --branch "$4" -- "$3" "$2" 2>&1
else
  git clone --quiet -- "$3" "$2" 2>&1
fi
"#;

/// One repository a new session clones.
#[derive(Debug, Clone)]
pub struct RepositoryClone {
    /// The directory under `repos/`, unique within the session ignoring case.
    pub directory: String,
    pub url: String,
    pub base_branch: Option<String>,
}

/// What a turn adds to its prompt.
#[derive(Debug, Clone, Default)]
pub struct TurnExtras {
    /// Appended to the agent's system prompt for this turn, such as Studio's instructions.
    pub system_prompt_append: Option<String>,
}

/// Runs sessions' turns. Cheap to clone; every clone is the same service.
#[derive(Debug, Clone)]
pub struct Sessions {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
    runner: Runner,
    workspace: WorkspaceContainer,
    grants: Grants,
    storage: Storage,
    /// What each session is doing, and its unfinished turns, for sessions that have had a turn
    /// since the API started. A session not here is idle.
    live: Mutex<HashMap<i64, Live>>,
    /// The last sequence handed out per session, read from the database on first use.
    sequences: tokio::sync::Mutex<HashMap<i64, u64>>,
    tracker: TaskTracker,
}

impl std::fmt::Debug for Inner {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Sessions")
            .field("runner", &self.runner)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct Live {
    activity: SessionActivity,
    /// Turns submitted and not finished, oldest first.
    unfinished: Vec<Uuid>,
}

/// The services sessions are built from.
#[derive(Debug, Clone)]
pub struct SessionsDeps {
    pub database: Pool,
    pub cipher: Arc<Cipher>,
    pub events: EventBus,
    pub runner: Runner,
    pub workspace: WorkspaceContainer,
    pub grants: Grants,
    pub storage: Storage,
}

impl Sessions {
    pub fn new(deps: SessionsDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                cipher: deps.cipher,
                events: deps.events,
                runner: deps.runner,
                workspace: deps.workspace,
                grants: deps.grants,
                storage: deps.storage,
                live: Mutex::new(HashMap::new()),
                sequences: tokio::sync::Mutex::new(HashMap::new()),
                tracker: TaskTracker::new(),
            }),
        }
    }

    /// The directory session `session_id` works in.
    pub fn directory(session_id: i64) -> WorkspacePath {
        WorkspacePath::new(format!("sessions/{session_id}"))
            .expect("a session number is a plain path component")
    }

    /// The workspace sessions work in, for the tools that move their files.
    pub fn workspace(&self) -> &WorkspaceContainer {
        &self.inner.workspace
    }

    /// The live tool grants of every running turn.
    pub fn grants(&self) -> &Grants {
        &self.inner.grants
    }

    /// What session `session_id` is doing now.
    pub fn activity(&self, session_id: i64) -> SessionActivity {
        lock(&self.inner.live)
            .get(&session_id)
            .map(|live| live.activity)
            .unwrap_or_default()
    }

    /// Readies a new session's directory and clones `repositories` into it, in order, with
    /// the session's GitHub token.
    ///
    /// # Errors
    /// `502` when the workspace is not available, and `400` naming a repository that would not
    /// clone, with git's own message. The directory is removed again on failure.
    pub async fn prepare(
        &self,
        session: &CodingSession,
        repositories: &[RepositoryClone],
    ) -> Result<(), ApiError> {
        self.inner
            .runner
            .prepare()
            .await
            .map_err(|error| ApiError::BadGateway(format!("the workspace is not available: {error:#}")))?;
        if repositories.is_empty() {
            return Ok(());
        }

        let github = {
            let mut connection = self.connection().await?;
            github_token::open_for_session(
                &mut connection,
                &self.inner.cipher,
                session.github_credential_id,
            )
            .await?
        };
        let environment: Vec<String> = github
            .as_ref()
            .map(github_token::environment)
            .unwrap_or_default()
            .iter()
            .map(|entry| format!("{}={}", entry.key, entry.value.expose_secret()))
            .collect();
        let scrubber = Scrubber::new(github);
        let directory = Self::directory(session.id).absolute();

        for repository in repositories {
            let cloned = self
                .inner
                .workspace
                .run_script(
                    AGENT_USER,
                    CLONE_SCRIPT,
                    &[
                        &directory,
                        &repository.directory,
                        &repository.url,
                        repository.base_branch.as_deref().unwrap_or(""),
                    ],
                    environment.clone(),
                    CLONE_TIMEOUT,
                )
                .await;
            let failure = match cloned {
                Ok(output) if output.exit_code == Some(0) => continue,
                Ok(output) => output.output,
                Err(error) => error.to_string(),
            };
            self.remove_directory(session.id).await;
            return Err(ApiError::BadRequest(format!(
                "{} could not be cloned: {}",
                repository.url,
                scrubber.scrub_text(failure.trim())
            )));
        }
        Ok(())
    }

    /// Starts `prompt` as `session`'s next turn, behind any it has unfinished.
    ///
    /// # Errors
    /// `503` while Elysium shuts down, and any failure reading the session's environment.
    pub async fn start_turn(
        &self,
        session: &CodingSession,
        prompt: String,
        extras: TurnExtras,
    ) -> Result<TurnView, ApiError> {
        let mut connection = self.connection().await?;
        let variables =
            environment_variable::thread_environment(&mut connection, &self.inner.cipher).await?;
        let github = github_token::open_for_session(
            &mut connection,
            &self.inner.cipher,
            session.github_credential_id,
        )
        .await?;
        let has_storage = !storage_location::list_for_project(&mut connection, session.project_id)
            .await?
            .is_empty();
        drop(connection);

        let mut secrets = Vec::new();
        let mut environment = Vec::with_capacity(variables.len() + 6);
        for variable in variables {
            if variable.is_secret {
                secrets.push(SecretString::from(variable.value.expose_secret().to_owned()));
            }
            environment.push(EnvironmentEntry {
                key: variable.key,
                value: variable.value,
            });
        }
        if let Some(token) = &github {
            environment.extend(github_token::environment(token));
            secrets.push(SecretString::from(token.expose_secret().to_owned()));
        }

        let directory = Self::directory(session.id);
        let grant = self.inner.grants.grant(
            CallScope::Session(SessionScope {
                session_id: session.id,
                project_id: session.project_id,
                action_item_id: session.action_item_id,
                directory: directory.clone(),
            }),
            &tool_servers(session.project_id.is_some(), has_storage),
        );
        let mut mcp_servers = grant.mcp_servers();
        mcp_servers.push(McpServer::Stdio {
            name: BLENDER_SERVER.to_owned(),
            command: BLENDER_COMMAND.to_owned(),
            arguments: Vec::new(),
            environment: Vec::new(),
        });

        let resume = session
            .harness_session_id
            .as_deref()
            .and_then(|id| id.parse().ok());
        let request = TurnRequest {
            conversation: ConversationId::for_session(session.id),
            label: format!("session:{}", session.id),
            prompt: prompt.clone(),
            working_directory: directory,
            resume,
            mcp_servers,
            allowed_tools: None,
            system_prompt_append: extras.system_prompt_append,
            environment,
            model: None,
        };
        let ticket = self
            .inner
            .runner
            .submit(request)
            .map_err(|_shutting_down| ApiError::Unavailable("Elysium is shutting down"))?;

        let queued_at = Utc::now();
        self.update(session.id, |live| {
            live.unfinished.push(ticket.turn_id);
            live.activity.queue_depth += 1;
            if live.activity.state == ActivityState::Idle {
                live.activity.state = ActivityState::Queued;
            }
            live.activity.last_activity_at = Some(queued_at);
        });
        self.announce(session.id).await;

        let follower = turn::Follower {
            sessions: self.clone(),
            session_id: session.id,
            studio_item_id: session.studio_item_id,
            turn_id: ticket.turn_id,
            scrubber: Scrubber::new(secrets),
            grant,
        };
        self.inner
            .tracker
            .spawn(async move { follower.follow(ticket.events).await });

        Ok(TurnView {
            turn_id: ticket.turn_id,
            status: TurnState::Queued,
            prompt,
            queued_at,
        })
    }

    /// Cancels every unfinished turn of session `session_id`, keeping its directory, such as
    /// for a Studio item that is hidden and may be restored.
    pub fn stop(&self, session_id: i64) {
        let unfinished = lock(&self.inner.live)
            .get(&session_id)
            .map(|live| live.unfinished.clone())
            .unwrap_or_default();
        for turn_id in unfinished {
            self.inner.runner.cancel(turn_id);
        }
    }

    /// Cancels every unfinished turn of session `session_id` and removes its directory, for a
    /// session being deleted. A directory that will not go is logged and left.
    pub async fn remove(&self, session_id: i64) {
        self.stop(session_id);
        lock(&self.inner.live).remove(&session_id);
        self.remove_directory(session_id).await;
    }

    /// Waits briefly for turn followers to record what their turns reported as they stop.
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

    async fn remove_directory(&self, session_id: i64) {
        if let Err(error) = self
            .inner
            .workspace
            .remove_directory(&Self::directory(session_id))
            .await
        {
            event!(
                name: "sessions.directory.remove_failure",
                Level::WARN,
                session.id = session_id,
                error.message = %error,
                "could not remove a session's directory; it stays in the workspace",
            );
        }
    }

    fn update(&self, session_id: i64, change: impl FnOnce(&mut Live)) {
        let mut live = lock(&self.inner.live);
        change(live.entry(session_id).or_default());
    }

    /// Tells every client what the session is doing now.
    async fn announce(&self, session_id: i64) {
        let found = match self.inner.database.get().await {
            Ok(mut connection) => crate::models::coding_session::find(&mut connection, session_id)
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        match found {
            Ok(session) => {
                let response = crate::routes::v1::coding_sessions::CodingSessionResponse::new(
                    session,
                    self.activity(session_id),
                );
                self.inner
                    .events
                    .publish(&crate::realtime::ServerEvent::SessionUpserted(response));
            }
            Err(message) => event!(
                name: "sessions.announce.failure",
                Level::DEBUG,
                session.id = session_id,
                error.message = %message,
                "could not announce a session's activity; it may have been deleted",
            ),
        }
    }

    async fn connection(
        &self,
    ) -> Result<
        diesel_async::pooled_connection::deadpool::Object<diesel_async::AsyncPgConnection>,
        ApiError,
    > {
        Ok(self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?)
    }
}

/// The tool servers a turn may call: the work tools when the session has a project, the
/// storage tools when that project reaches a storage location.
fn tool_servers(has_project: bool, has_storage: bool) -> Vec<&'static ToolServer> {
    let mut servers = Vec::with_capacity(2);
    if has_project {
        servers.push(&tools::work::SERVER);
    }
    if has_storage {
        servers.push(&tools::storage::SERVER);
    }
    servers
}

/// Every critical section leaves the maps whole, so a poisoned lock holds nothing half done.
fn lock<Value>(mutex: &Mutex<Value>) -> std::sync::MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_works_in_its_own_directory() {
        assert_eq!(Sessions::directory(12).absolute(), "/workspace/sessions/12");
    }

    #[test]
    fn work_tools_need_a_project_and_storage_tools_a_location() {
        let names = |servers: Vec<&'static ToolServer>| -> Vec<&'static str> {
            servers.iter().map(|server| server.name).collect()
        };
        assert_eq!(names(tool_servers(true, true)), ["elysium_work", "elysium_storage"]);
        assert_eq!(names(tool_servers(true, false)), ["elysium_work"]);
        assert!(names(tool_servers(false, false)).is_empty());
        assert_eq!(names(tool_servers(false, true)), ["elysium_storage"]);
    }
}
