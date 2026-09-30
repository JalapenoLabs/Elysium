// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/storage-locations/{id}`: forget a location.
//!
//! Files already written there are left with the provider; Elysium only stops using it. A
//! location that Studio items keep files in cannot be deleted: their files would be orphaned
//! and the items unreadable, so those items are deleted permanently first.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::storage_location;
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

    storage_location::delete(&mut connection, id)
        .await
        .map_err(|error| match error {
            DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _) => {
                ApiError::Conflict(
                    "Studio items keep files in this location; delete them permanently first",
                )
            }
            other => other.into(),
        })?;

    state
        .events
        .publish(&ServerEvent::StorageLocationDeleted { id });

    Ok(StatusCode::NO_CONTENT)
}
