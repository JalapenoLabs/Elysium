// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/storage-locations/{id}`: forget a location.
//!
//! Files already written there are left with the provider; Elysium only stops using it.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
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

    storage_location::delete(&mut connection, id).await?;

    state
        .events
        .publish(&ServerEvent::StorageLocationDeleted { id });

    Ok(StatusCode::NO_CONTENT)
}
