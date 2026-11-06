// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/page-attachments/destination`: choose the storage location new attachments go to, or
//! `null` to stop taking them.
//!
//! The location is reached with its saved settings before it is chosen, as Test does, so a wrong key
//! is found now rather than by whoever attaches the next file. What is already attached stays where
//! it landed and stays served; while chosen, the location cannot be deleted.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use diesel::result::Error as DieselError;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use super::PolicyResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::storage_location;
use crate::models::user::{self, WorkspaceSettingsChanges};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// A location's id, or `null` to stop taking attachments. Required, so a body missing it never
    /// turns attachments off.
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
        ApiError::BadRequest("storageLocationId is required; null stops attachments".to_owned())
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
        page_attachment_storage_location_id: Some(chosen),
        ..WorkspaceSettingsChanges::default()
    };
    let settings = user::update_workspace_settings(&mut connection, changes, admin.id()).await?;
    drop(connection);

    event!(
        name: "pages.attachments.destination.changed",
        Level::INFO,
        storage.location.id = chosen.map(|id| id.to_string()).unwrap_or_default(),
        "chose where page attachments are kept",
    );
    state.events.publish(&ServerEvent::PageAttachmentsChanged);
    Ok(Json(json!({ "policy": PolicyResponse::from(settings) })))
}
