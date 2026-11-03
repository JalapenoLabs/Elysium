// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/backups`: Back up now. Answers `202` as soon as the backup begins; how it ends
//! is published as `backups.status`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::auth::AdminUser;
use crate::backups::{BeginError, DatabaseBackupView};
use crate::errors::ApiError;
use crate::models::database_backup::DatabaseBackupTrigger;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let backup = state
        .backups
        .begin(DatabaseBackupTrigger::Admin, admin.id())
        .await
        .map_err(|error| match error {
            BeginError::AlreadyRunning => ApiError::CodedConflict {
                code: "backup_running",
                message: "a backup is already running",
            },
            BeginError::NoDestination => ApiError::CodedConflict {
                code: "no_backup_destination",
                message: "choose a storage location for backups first",
            },
            BeginError::Internal(error) => ApiError::Internal(error),
        })?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "backup": DatabaseBackupView::from(backup) })),
    ))
}
