// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/connectors/{id}/sync`: read a connector now, in the loop that reads its kind,
//! which is also how a failed sync is retried. Answers `202` at once; the connector shows as
//! syncing until it is done, and `409` when it is paused or already syncing.

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::visible;
use crate::auth::CurrentUser;
use crate::connectors::kinds;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let stored = visible(&state, id, current.id()).await?;
    kinds::sync(&state, &stored).await?;
    Ok(StatusCode::ACCEPTED)
}
