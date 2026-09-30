// Copyright © 2026 Jalapeno Labs

//! Opening a session's thread and recording it, for every route that starts one: a Coding
//! session, a Studio item, and a Studio item continued on a new thread.
//!
//! The session's number is reserved first, so the thread carries it in its metadata from
//! the moment it exists. A create that fails after the reservation leaves a gap in the
//! numbering, which is harmless.
//!
//! The satellite's idempotency key is a fresh `UUIDv7` per request, never the number: numbers
//! repeat across Elysium installs sharing a satellite and after a database reset, and a
//! repeated key would hand back another session's thread. The key makes the SDK's own
//! retries of this one request safe; a client that posts again opens a second thread.
//!
//! A session either comes out whole or not at all. If the row cannot be recorded, the thread
//! is destroyed. If what the caller does next (preparing the workspace, queuing the first
//! turn) fails, [`discard`] removes the thread, the row, and the watchers together.

use std::collections::BTreeMap;

use anyhow::Context;
use arsox_sdk::client::ThreadHandle;
use arsox_sdk::proto::settings::v1::ThreadSettings;
use tracing::{Level, event};
use uuid::Uuid;

use super::CodingSessionResponse;
use crate::errors::ApiError;
use crate::fleet::views::ThreadStatus;
use crate::fleet::{MANAGED_METADATA_KEY, SESSION_METADATA_KEY};
use crate::models::coding_session::{self, CodingSession, NewCodingSession};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// A session about to be opened: where its thread runs, how, and what the row records.
#[derive(Debug)]
pub struct SessionOpening {
    pub satellite_id: Uuid,
    pub settings: ThreadSettings,
    pub title: String,
    pub project_id: Option<Uuid>,
    pub github_credential_id: Option<Uuid>,
    pub action_item_id: Option<Uuid>,
    pub studio_item_id: Option<Uuid>,
}

/// A session whose thread is open and recorded, with its watchers started. Nothing has
/// announced it yet; see [`announce`].
#[derive(Debug)]
pub struct OpenedSession {
    pub session: CodingSession,
    pub handle: ThreadHandle,
    pub thread: ThreadStatus,
}

/// Opens the thread, records the session, and starts following it.
///
/// Watchers start before the caller queues a turn, so the tools the turn asks the agent to
/// call have a client answering them as early as Elysium can manage.
///
/// # Errors
/// Answers as the satellite does when it refuses the thread, and propagates the database's
/// refusal of the row, in which case the thread has been destroyed.
pub async fn open(state: &AppState, opening: SessionOpening) -> Result<OpenedSession, ApiError> {
    let client = state.fleet.client(opening.satellite_id).await?;
    // Reserved once there is a client for the satellite, so a create refused for an
    // inactive or unreachable satellite costs no number.
    let session_id = {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        coding_session::reserve_id(&mut connection).await?
    };
    let metadata = BTreeMap::from([
        (MANAGED_METADATA_KEY.to_owned(), "true".to_owned()),
        (SESSION_METADATA_KEY.to_owned(), session_id.to_string()),
    ]);
    let created = client
        .threads()
        .create_with(opening.settings, Some(Uuid::now_v7().to_string()), metadata)
        .await?;

    let new_session = NewCodingSession {
        id: session_id,
        project_id: opening.project_id,
        satellite_id: opening.satellite_id,
        thread_id: created.thread.thread_id.clone(),
        title: opening.title,
        github_credential_id: opening.github_credential_id,
        action_item_id: opening.action_item_id,
        studio_item_id: opening.studio_item_id,
    };
    let session = record_or_abandon(state, &created.handle, &new_session).await?;

    let thread = ThreadStatus::from(&created.thread);
    state.fleet.watch_session(session.clone());
    Ok(OpenedSession {
        session,
        handle: created.handle,
        thread,
    })
}

/// Tells every client about a session that is now complete.
pub fn announce(state: &AppState, opened: &OpenedSession) {
    state
        .events
        .publish(&ServerEvent::SessionUpserted(CodingSessionResponse::new(
            opened.session.clone(),
            Some(opened.thread.clone()),
        )));
}

/// Removes a session that could not be completed: its thread, its row, and its watchers,
/// then announces it gone. The route never announced the session, but the satellite poll
/// reads the row and may have; a `session.deleted` for a session a client never saw changes
/// nothing.
///
/// The row goes before the watchers, so a poll landing in between cannot record a status
/// for a session nothing would clear.
pub async fn discard(state: &AppState, opened: &OpenedSession) {
    let session = &opened.session;
    abandon_thread(&opened.handle, session.satellite_id).await;
    match state.database.get().await {
        Ok(mut connection) => {
            if let Err(database_error) = coding_session::delete(&mut connection, session.id).await {
                event!(
                    name: "coding_session.create.orphaned_row",
                    Level::ERROR,
                    session.id = %session.id,
                    error.message = %database_error,
                    "could not remove a session that was not completed; delete it by hand",
                );
            }
        }
        Err(pool_error) => event!(
            name: "coding_session.create.orphaned_row",
            Level::ERROR,
            session.id = %session.id,
            error.message = %pool_error,
            "could not remove a session that was not completed; delete it by hand",
        ),
    }
    state.fleet.forget_session(session.id);
    state
        .events
        .publish(&ServerEvent::SessionDeleted { id: session.id });
}

/// Records the session of a thread just opened, destroying the thread when the row cannot be
/// written, so no thread runs with nothing pointing at it.
///
/// # Errors
/// Propagates the database's refusal.
async fn record_or_abandon(
    state: &AppState,
    handle: &ThreadHandle,
    new_session: &NewCodingSession,
) -> Result<CodingSession, ApiError> {
    let recorded = match state.database.get().await {
        Ok(mut connection) => coding_session::create(&mut connection, new_session)
            .await
            .map_err(ApiError::from),
        Err(pool_error) => Err(anyhow::Error::from(pool_error)
            .context("no database connection available")
            .into()),
    };
    if recorded.is_err() {
        abandon_thread(handle, Some(new_session.satellite_id)).await;
    }
    recorded
}

/// Destroys a thread whose session could not be completed. A thread that will not go is
/// logged and left to expire on its idle TTL.
async fn abandon_thread(handle: &ThreadHandle, satellite_id: Option<Uuid>) {
    if let Err(destroy_error) = handle.destroy().await {
        event!(
            name: "coding_session.create.orphaned_thread",
            Level::ERROR,
            satellite.id = ?satellite_id,
            thread.id = %handle.id(),
            error.message = %destroy_error,
            "could not complete the session or destroy its thread; the thread expires on its idle TTL",
        );
    }
}
