// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/connectors/{id}`: remove a connector and the account it owns, which signs it
//! out: its token is forgotten, or its browser is removed with the profile its person signed in
//! on. What it recorded stays on the timeline.
//!
//! `?deleteMeetings=true` also deletes the meetings it listed, softly, as the person, each
//! recorded as removed with it (`crate::meetings::deletion`), so they can be restored from the
//! Meetings page like any other. Which meetings those are is decided once, by
//! `meeting::listed_by`, which also answers how many the removal offers to delete
//! (`count_meetings`). They are named before the connector goes, since removing it leaves their
//! captures naming no connector, and deleted once it went, so a removal that fails deletes
//! nothing.

use anyhow::Context;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use super::visible;
use crate::auth::CurrentUser;
use crate::connectors::kinds;
use crate::errors::ApiError;
use crate::meetings::deletion::{self, BulkChange};
use crate::models::meeting;
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
    let meeting_ids = if options.delete_meetings {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        meeting::listed_by(&mut connection, id, current.id()).await?
    } else {
        Vec::new()
    };

    kinds::remove(&state, &stored).await?;
    state.events.publish(&ServerEvent::ConnectorDeleted { id });

    if !meeting_ids.is_empty() {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        let outcome = deletion::change_many(
            &mut connection,
            &meeting_ids,
            BulkChange::Delete {},
            current.id(),
            Utc::now(),
            Some(id),
        )
        .await?;
        // A meeting refused was changed meanwhile, such as merged into another; it stays, and
        // can be deleted like any other.
        if !outcome.refused.is_empty() {
            tracing::event!(
                name: "connectors.delete.meetings_refused",
                tracing::Level::INFO,
                connector.id = %id,
                meetings.refused = outcome.refused.len(),
                "some of a removed connector's meetings changed meanwhile and were left",
            );
        }
        deletion::announce(&state.events, &outcome);
    }
    Ok(StatusCode::NO_CONTENT)
}
