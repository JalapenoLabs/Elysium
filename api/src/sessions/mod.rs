// Copyright © 2026 Jalapeno Labs

//! Coding and Studio sessions, run on the runner.
//!
//! A session is a directory in the workspace (`sessions/<id>`), the Claude session its turns
//! continue, and the events its turns reported. Two halves share it:
//!
//! - [`Sessions`], which the API holds, is everything a request does: **preparing** a new
//!   session's directory by cloning its repositories into `repos/<name>`, **queueing a turn**
//!   ([`Sessions::start_turn`]) as a `session_turns` row, **stopping** a session's turns, reading
//!   **what a session is doing** from those rows ([`Sessions::activity`]), and **removing** a
//!   session's directory when it is deleted.
//! - [`queue::TurnQueue`], which the engine runs, takes each session's next queued turn once the
//!   one before it finished, and runs it: the prompt, the session's environment (workspace
//!   variables, then the GitHub token and the git config that uses it), the tool servers it may
//!   call under a per-turn grant (`crate::tools::grants`), a Blender and a desktop of its own,
//!   and the Claude session to resume. [`turn`] follows it: every event is scrubbed of the turn's
//!   secrets, numbered, recorded in `session_events`, and published, in that order.
//!
//! The two meet only in Postgres and on the bus ([`Signal::SessionTurns`]), so either process
//! can restart without the other losing a turn: a queued turn waits for the engine, and an
//! engine that stopped mid-turn finishes that turn when it starts again.
//!
//! Nothing here matches on whether a session is Coding or Studio beyond the instructions
//! Studio's agents get and the collection after a Studio turn.

pub mod desktop;
#[cfg(test)]
mod live_tests;
pub mod queue;
#[cfg(test)]
mod restart_tests;
pub mod scrub;
mod summary;
mod turn;
pub mod views;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use secrecy::ExposeSecret;
use tracing::{Level, event};
use uuid::Uuid;

use self::scrub::Scrubber;
use self::views::{SessionActivity, TurnState, TurnView};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::coding_session::CodingSession;
use crate::models::session_turn::{self, NewSessionTurn};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::coding_sessions::github_token;
use crate::runner::WorkspacePath;
use crate::runner::workspace::{AGENT_USER, WorkspaceContainer};

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

/// Sessions as the API sees them. Cheap to clone; every clone is the same service.
#[derive(Debug, Clone)]
pub struct Sessions {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
    workspace: WorkspaceContainer,
}

impl std::fmt::Debug for Inner {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Sessions").finish_non_exhaustive()
    }
}

/// The services sessions are built from.
#[derive(Clone)]
pub struct SessionsDeps {
    pub database: Pool,
    pub cipher: Arc<Cipher>,
    pub events: EventBus,
    pub workspace: WorkspaceContainer,
}

impl std::fmt::Debug for SessionsDeps {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The pool and cipher have nothing useful to print.
        formatter
            .debug_struct("SessionsDeps")
            .finish_non_exhaustive()
    }
}

impl Sessions {
    pub fn new(deps: SessionsDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                cipher: deps.cipher,
                events: deps.events,
                workspace: deps.workspace,
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

    /// What session `session_id` is doing now.
    ///
    /// # Errors
    /// Propagates database errors.
    pub async fn activity(&self, session_id: i64) -> Result<SessionActivity, ApiError> {
        Ok(self
            .activities(&[session_id])
            .await?
            .remove(&session_id)
            .unwrap_or_default())
    }

    /// What each of `session_ids` is doing now. A session missing from the answer is idle.
    ///
    /// # Errors
    /// Propagates database errors.
    pub async fn activities(
        &self,
        session_ids: &[i64],
    ) -> Result<HashMap<i64, SessionActivity>, ApiError> {
        let mut connection = self.connection().await?;
        Ok(session_turn::activities(&mut connection, session_ids)
            .await
            .context("cannot read what sessions are doing")?)
    }

    /// Readies a new session's directory and clones `repositories` into it, in order, with
    /// the session's GitHub token.
    ///
    /// # Errors
    /// `502` when the workspace is not running, and `400` naming a repository that would not
    /// clone, with git's own message. The directory is removed again on failure.
    pub async fn prepare(
        &self,
        session: &CodingSession,
        repositories: &[RepositoryClone],
    ) -> Result<(), ApiError> {
        if repositories.is_empty() {
            return Ok(());
        }
        // The engine creates the workspace and keeps it up; the API only works in it.
        let running = self.inner.workspace.is_running().await.map_err(|error| {
            ApiError::BadGateway(format!("the workspace is not available: {error}"))
        })?;
        if !running {
            return Err(ApiError::BadGateway(
                "the workspace is not running yet; the engine starts it, so try again shortly"
                    .to_owned(),
            ));
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

    /// Queues `prompt`, from `by`, as `session`'s next turn, behind any it has unfinished. The
    /// engine runs it.
    ///
    /// # Errors
    /// Propagates database errors.
    pub async fn start_turn(
        &self,
        session: &CodingSession,
        prompt: String,
        extras: TurnExtras,
        by: Uuid,
    ) -> Result<TurnView, ApiError> {
        let mut connection = self.connection().await?;
        let turn = session_turn::queue(
            &mut connection,
            &NewSessionTurn {
                id: Uuid::now_v7(),
                session_id: session.id,
                prompt: &prompt,
                system_prompt_append: extras.system_prompt_append.as_deref(),
                created_by: by,
            },
        )
        .await
        .context("cannot queue the turn")?;
        drop(connection);

        self.inner.events.signal(&Signal::SessionTurns);
        self.announce(session.id).await;
        Ok(TurnView {
            turn_id: turn.id,
            status: TurnState::Queued,
            prompt,
            queued_at: turn.queued_at,
        })
    }

    /// Stops every unfinished turn of session `session_id`, keeping its directory, such as for
    /// a Studio item that is hidden and may be restored. A queued turn never runs; the engine
    /// cancels one it runs.
    pub async fn stop(&self, session_id: i64) {
        let requested = async {
            let mut connection = self.inner.database.get().await?;
            anyhow::Ok(session_turn::request_stop(&mut connection, session_id).await?)
        };
        match requested.await {
            Ok(held_by_engine) => {
                if held_by_engine {
                    self.inner.events.signal(&Signal::SessionTurns);
                }
                self.announce(session_id).await;
            }
            Err(error) => event!(
                name: "sessions.stop.failure",
                Level::ERROR,
                session.id = session_id,
                error.message = %format!("{error:#}"),
                "could not stop a session's turns",
            ),
        }
    }

    /// Stops every unfinished turn of session `session_id` and removes its directory, for a
    /// session being deleted. A directory that will not go is logged and left.
    pub async fn remove(&self, session_id: i64) {
        self.stop(session_id).await;
        self.remove_directory(session_id).await;
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

    /// Tells every client what the session is doing now.
    pub(crate) async fn announce(&self, session_id: i64) {
        let found = async {
            let mut connection = self.inner.database.get().await?;
            let session = crate::models::coding_session::find(&mut connection, session_id).await?;
            let activity = session_turn::activities(&mut connection, &[session_id])
                .await?
                .remove(&session_id)
                .unwrap_or_default();
            anyhow::Ok((session, activity))
        };
        match found.await {
            Ok((session, activity)) => {
                let response = crate::routes::v1::coding_sessions::CodingSessionResponse::new(
                    session, activity,
                );
                self.inner
                    .events
                    .publish(&ServerEvent::SessionUpserted(response));
            }
            Err(error) => event!(
                name: "sessions.announce.failure",
                Level::DEBUG,
                session.id = session_id,
                error.message = %format!("{error:#}"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_works_in_its_own_directory() {
        assert_eq!(Sessions::directory(12).absolute(), "/workspace/sessions/12");
    }
}
