// Copyright © 2026 Jalapeno Labs

//! `/internal/backups`: what the deployment's own tooling calls to back up the databases before
//! it changes the stack. See `crate::backups`.
//!
//! The caller is a shell script on the host, with `curl` and no JSON parser, so both routes
//! speak one line of plain text, and both check the updater's key: the tooling that deploys
//! holds the same encryption key the updater derives it from. A backup outlasts any one
//! request, so the script begins one and then asks how it stands until it ends.
//!
//! - `POST /internal/backups` begins one and answers `202` with `running <id>`; `409` with
//!   `already-running` or `no-destination` when it cannot begin.
//! - `GET /internal/backups/{id}` answers `running`, `succeeded`, or `failed <reason>`.

use axum::Router;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use uuid::Uuid;

use super::updater::require_updater_key;
use crate::backups::BeginError;
use crate::errors::ApiError;
use crate::models::database_backup::{DatabaseBackupStatus, DatabaseBackupTrigger};
use crate::models::user::SYSTEM_USER_ID;
use crate::state::AppState;

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/", post(begin))
        .route("/{id}", get(outcome))
        .route_layer(from_fn_with_state(state.clone(), require_updater_key))
}

async fn begin(State(state): State<AppState>) -> Result<(StatusCode, String), ApiError> {
    match state
        .backups
        .begin(DatabaseBackupTrigger::Deployment, SYSTEM_USER_ID)
        .await
    {
        Ok(backup) => Ok((StatusCode::ACCEPTED, format!("running {}", backup.id))),
        Err(BeginError::AlreadyRunning) => Ok((StatusCode::CONFLICT, "already-running".to_owned())),
        Err(BeginError::NoDestination) => Ok((StatusCode::CONFLICT, "no-destination".to_owned())),
        Err(BeginError::Internal(error)) => Err(ApiError::Internal(error)),
    }
}

async fn outcome(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<String, ApiError> {
    let Path(id) = path?;
    let backup = state.backups.find(id).await?.ok_or(ApiError::NotFound)?;
    let line = match backup.status {
        DatabaseBackupStatus::Running => "running".to_owned(),
        DatabaseBackupStatus::Succeeded => "succeeded".to_owned(),
        DatabaseBackupStatus::Failed => {
            format!("failed {}", backup.error.unwrap_or_default())
        }
    };
    Ok(line)
}
