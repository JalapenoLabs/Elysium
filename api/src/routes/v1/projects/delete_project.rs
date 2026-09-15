// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/projects/{id}`: delete a project that no session belongs to.
//!
//! Deleting a project never takes its sessions with it. Each session owns a thread on
//! a satellite, and those are destroyed deliberately, one session at a time.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::project;
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

    match project::delete(&mut connection, id).await {
        Ok(()) => {}
        Err(DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _)) => {
            return Err(ApiError::Conflict(
                "the project still has coding sessions; delete them first",
            ));
        }
        Err(other) => return Err(other.into()),
    }

    state.events.publish(&ServerEvent::ProjectDeleted { id });

    Ok(StatusCode::NO_CONTENT)
}
