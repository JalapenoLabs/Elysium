// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/backups/destination`: choose the storage location the databases are backed up
//! to, or `null` to stop backing them up.
//!
//! The location is reached with its saved settings before it is chosen, as Test does, so a
//! wrong key is found now rather than at night. While chosen, it cannot be deleted.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::storage_location;
use crate::models::user::{self, WorkspaceSettingsChanges};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// A location's id, or `null` to stop backing up. Required, so a body missing it never
    /// turns backups off.
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and an id are three distinct requests"
    )]
    storage_location_id: Option<Option<Uuid>>,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    let chosen = body.storage_location_id.ok_or_else(|| {
        ApiError::BadRequest("storageLocationId is required; null stops backups".to_owned())
    })?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    if let Some(id) = chosen {
        let location = storage_location::find(&mut connection, id)
            .await
            .map_err(|error| match error {
                DieselError::NotFound => {
                    ApiError::BadRequest("no storage location has that id".to_owned())
                }
                other => other.into(),
            })?;
        let access_key = location
            .access_key(&state.cipher)
            .context("the stored access key cannot be decrypted")?;
        // The provider may take a while to answer; no connection is held while it does.
        drop(connection);
        state.storage.check(&location, &access_key).await?;
        connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
    }

    let changes = WorkspaceSettingsChanges {
        backup_storage_location_id: Some(chosen),
        ..WorkspaceSettingsChanges::default()
    };
    user::update_workspace_settings(&mut connection, changes, admin.id()).await?;
    drop(connection);

    event!(
        name: "backups.destination.changed",
        Level::INFO,
        storage.location.id = chosen.map(|id| id.to_string()).unwrap_or_default(),
        "chose where the databases are backed up",
    );
    state.backups.publish_status().await;
    let status = state.backups.status().await?;
    Ok(Json(json!({ "backups": status })))
}
