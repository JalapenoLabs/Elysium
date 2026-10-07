// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/satellites/{id}`: forget a satellite.
//!
//! Its sessions stay, with their kept history, and lose only the pointer to the satellite;
//! a Studio item's continues on another satellite with its next prompt. Threads on the
//! satellite are left alone and expire on their idle TTL. Destroying them here would make
//! deleting an unreachable satellite impossible.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::{coding_session, satellite};
use crate::realtime::ServerEvent;
use crate::routes::v1::coding_sessions::CodingSessionResponse;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let sessions = coding_session::list_for_satellite(&mut connection, id).await?;
    satellite::delete(&mut connection, id).await?;

    state.fleet.forget_satellite(id);
    state.events.publish(&ServerEvent::SatelliteDeleted { id });
    // The database cleared each session's satellite; clients hear it the same way.
    for mut session in sessions {
        session.satellite_id = None;
        state
            .events
            .publish(&ServerEvent::SessionUpserted(CodingSessionResponse::new(
                session, None,
            )));
    }

    Ok(StatusCode::NO_CONTENT)
}
