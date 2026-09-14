// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/satellites/{id}`: forget a satellite and the sessions recorded on it.
//!
//! Threads on the satellite are left alone and expire on their idle TTL. Destroying
//! them here would make deleting an unreachable satellite impossible.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::satellite;
use crate::realtime::ServerEvent;
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

    satellite::delete(&mut connection, id).await?;

    state.fleet.forget_satellite(id);
    state.events.publish(&ServerEvent::SatelliteDeleted { id });

    Ok(StatusCode::NO_CONTENT)
}
