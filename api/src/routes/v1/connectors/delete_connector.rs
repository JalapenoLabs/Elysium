// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/connectors/{id}`: remove a connector and the account it owns, which signs it
//! out: its token is forgotten, or its browser is removed with the profile its person signed in
//! on. What it recorded stays on the timeline.
//!
//! `?deleteMeetings=true` also deletes the meetings it listed, softly, as the person, each
//! recorded as removed with it (`crate::meetings::deletion`), so they can be restored from the
//! Meetings page like any other. Which meetings those are is decided once, by
//! `meeting::listed_by`, which also answers how many the removal offers to delete
//! (`count_meetings`).
//!
//! The connector's removal reaches outside Postgres (a browser's container goes with it), so it and
//! the meetings cannot share one transaction. The meetings go first, in parts of at most
//! `deletion::MAX_MEETINGS`, each one change committed and announced on its own, while the
//! connector still names them; then the connector goes. A part that fails, or a removal that
//! fails, restores the meetings this request deleted before answering the error, so a removal that
//! fails leaves the connector and its meetings as they were, and can be tried again.

use anyhow::Context;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use tracing::{Level, event};
use uuid::Uuid;

use super::visible;
use crate::auth::CurrentUser;
use crate::connectors::kinds;
use crate::errors::ApiError;
use crate::meetings::deletion::{self, BulkChange, MAX_MEETINGS};
use crate::models::meeting;
use crate::models::user::User;
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    /// Whether the meetings it listed go with it. Off unless asked.
    #[serde(default)]
    delete_meetings: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<Options>, QueryRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let Query(options) = query?;
    let stored = visible(&state, id, current.id()).await?;
    let deleted = if options.delete_meetings {
        delete_meetings_of(&state, id, &current.0).await?
    } else {
        Vec::new()
    };

    if let Err(error) = kinds::remove(&state, &stored).await {
        restore(&state, &deleted, &current.0).await;
        return Err(error);
    }
    state.events.publish(&ServerEvent::ConnectorDeleted { id });
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes the meetings connector `connector_id` listed, as `person`, and answers the ids of those
/// deleted. One refused was changed meanwhile, such as merged into another; it stays, and can be
/// deleted like any other.
///
/// # Errors
/// Any database error, once the meetings deleted before it are restored.
async fn delete_meetings_of(
    state: &AppState,
    connector_id: Uuid,
    person: &User,
) -> Result<Vec<Uuid>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let listed = meeting::listed_by(&mut connection, connector_id, person.id).await?;
    let mut deleted = Vec::with_capacity(listed.len());
    for part in listed.chunks(MAX_MEETINGS) {
        let changed = deletion::change_many(
            &mut connection,
            part,
            BulkChange::Delete {},
            person,
            Utc::now(),
            Some(connector_id),
        )
        .await;
        let outcome = match changed {
            Ok(outcome) => outcome,
            Err(error) => {
                restore(state, &deleted, person).await;
                return Err(error.into());
            }
        };
        if !outcome.refused.is_empty() {
            event!(
                name: "connectors.delete.meetings_refused",
                Level::INFO,
                connector.id = %connector_id,
                meetings.refused = outcome.refused.len(),
                "some of a removed connector's meetings changed meanwhile and were left",
            );
        }
        deleted.extend(outcome.changed.iter().map(|changed| changed.meeting.id));
        deletion::announce(&state.events, &mut connection, outcome.changed).await;
    }
    Ok(deleted)
}

/// Restores `meeting_ids`, deleted for a removal that then failed, as `person`, so the removal
/// changes nothing. A restore that fails is logged: those meetings stay deleted, restorable from
/// the Meetings page, and the removal's own error is what the person is answered.
async fn restore(state: &AppState, meeting_ids: &[Uuid], person: &User) {
    if meeting_ids.is_empty() {
        return;
    }
    let restored = async {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        for part in meeting_ids.chunks(MAX_MEETINGS) {
            let outcome = deletion::change_many(
                &mut connection,
                part,
                BulkChange::Restore {},
                person,
                Utc::now(),
                None,
            )
            .await?;
            deletion::announce(&state.events, &mut connection, outcome.changed).await;
        }
        anyhow::Ok(())
    }
    .await;
    if let Err(error) = restored {
        event!(
            name: "connectors.delete.meetings_not_restored",
            Level::WARN,
            meetings.count = meeting_ids.len(),
            error.message = %format!("{error:#}"),
            "a connector's removal failed after its meetings were deleted, and they could not be \
             restored; they stay deleted, restorable from the Meetings page",
        );
    }
}
