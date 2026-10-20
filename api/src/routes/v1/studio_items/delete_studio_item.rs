// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/studio-items/{id}`: hide an item until it is restored, or with
//! `?permanently=true`, remove it for good.
//!
//! Either way its live thread is destroyed first, which stops its cost and frees the
//! satellite's disk. A thread that already ended, or whose satellite was deleted or
//! deactivated, is past destroying; any other satellite failure stops the delete, so an item
//! never disappears while its thread still runs.
//!
//! A permanent delete removes every file of the item from its storage location before any
//! row, and stops at the first file the provider refuses, so no rows vanish while their files
//! linger in a bucket. See `docs/studio.md`, Deleting.

use std::collections::BTreeSet;

use anyhow::Context;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::fleet::ClientError;
use crate::models::coding_session::{self, CodingSession};
use crate::models::studio_item::{self, StudioItem};
use crate::models::{storage_location, studio_asset, studio_feedback};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteQuery {
    #[serde(default)]
    permanently: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<DeleteQuery>, QueryRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let Query(query) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let item = studio_item::find(&mut connection, id).await?;
    let sessions = coding_session::list_for_studio_items(&mut connection, &[id]).await?;
    drop(connection);

    for session in &sessions {
        destroy_thread(&state, session).await?;
    }

    if !query.permanently {
        let mut connection = state
            .database
            .get()
            .await
            .context("no database connection available")?;
        studio_item::set_deleted_at(&mut connection, id, Some(Utc::now())).await?;
        state.events.publish(&ServerEvent::StudioItemDeleted { id });
        return Ok(StatusCode::NO_CONTENT);
    }

    delete_files(&state, &item).await?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    studio_item::delete(&mut connection, id).await?;
    drop(connection);

    for session in sessions {
        state.fleet.forget_session(session.id);
        state
            .events
            .publish(&ServerEvent::SessionDeleted { id: session.id });
    }
    state.events.publish(&ServerEvent::StudioItemDeleted { id });
    Ok(StatusCode::NO_CONTENT)
}

/// Destroys a session's thread, if it still has one to destroy.
///
/// # Errors
/// Answers as the satellite does when it refuses for any reason but the thread being gone.
async fn destroy_thread(state: &AppState, session: &CodingSession) -> Result<(), ApiError> {
    let Some(satellite_id) = session.satellite_id else {
        return Ok(());
    };
    let client = match state.fleet.client(satellite_id).await {
        Ok(client) => client,
        // Elysium reaches no thread on an inactive satellite; it expires on its idle TTL.
        Err(ClientError::Inactive) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let destroyed = match client.threads().attach(session.thread_id.as_str()).await {
        Ok(handle) => handle.destroy().await.map(drop),
        Err(error) => Err(error),
    };
    match destroyed {
        Err(error) if !(error.is_gone() || error.is_not_found()) => Err(error.into()),
        _ => Ok(()),
    }
}

/// Removes every file an item keeps in its storage location: its assets and both images of
/// every drawn prompt. Deleting a file that is already gone succeeds, so a retried delete
/// picks up where a refused one stopped.
///
/// # Errors
/// Answers `502` naming the file the provider refused.
async fn delete_files(state: &AppState, item: &StudioItem) -> Result<(), ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let location = storage_location::find(&mut connection, item.storage_location_id).await?;
    let assets = studio_asset::list_for_items(&mut connection, &[item.id]).await?;
    let feedback = studio_feedback::list_for_item(&mut connection, item.id).await?;
    drop(connection);

    // Files are named by content, so versions can share one; each is deleted once.
    let mut paths = BTreeSet::new();
    for asset in assets {
        paths.insert(asset.storage_path);
    }
    for drawn in feedback {
        paths.insert(drawn.annotated_storage_path);
        paths.extend(drawn.capture_storage_path);
    }

    let access_key = location
        .access_key(&state.cipher)
        .context("the storage location's access key cannot be decrypted")?;
    for path in paths {
        state
            .storage
            .delete(&location, &access_key, &path)
            .await
            .map_err(|error| {
                ApiError::BadGateway(format!(
                    "the item was kept because its storage location refused to delete {path}: {error}"
                ))
            })?;
    }
    Ok(())
}
