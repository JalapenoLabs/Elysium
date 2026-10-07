// Copyright © 2026 Jalapeno Labs

//! How an agent's deliverables reach its Studio item.
//!
//! At the end of every turn the satellite scans the workspace's `artifacts/` and announces
//! each file that is new or changed (`ArtifactCreated`). The session watcher hands each
//! announcement here, and [`keep`] streams the file out of the workspace into the item's
//! storage location, records it, and tells clients. Nothing is held in memory.
//!
//! The satellite hashed the file when it scanned, but the agent owns the workspace and may
//! change the file before Elysium reads it. So the bytes are hashed again as they stream,
//! and a file that no longer matches its announcement is dropped from storage and not
//! recorded: the change is announced at the end of the turn that made it.
//!
//! A file that cannot be kept (over the location's storage limit, refused by the provider)
//! is recorded on the item as its `pull_error`, which the item page shows, and the next file
//! that is kept clears it. See `docs/studio.md`, Assets.

use std::sync::{Arc, Mutex};

use arsox_sdk::client::ThreadHandle;
use arsox_sdk::proto::artifact::v1::Artifact;
use bytes::Bytes;
use futures_util::StreamExt as _;
use sha2::{Digest as _, Sha256};
use tracing::{Level, event};
use uuid::Uuid;

use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::storage_location;
use crate::models::studio_asset::{self, NewStudioAsset, StudioAsset};
use crate::models::studio_item;
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::studio_items::{StudioAssetResponse, StudioItemResponse};
use crate::storage::{Storage, StorageError};

/// Hasher locks are held only to feed one chunk or read the digest, never across an await.
const HASHER_POISONED: &str = "artifact hasher lock poisoned by an earlier panic";

/// What keeping a file uses. Borrowed from the fleet, which owns them.
#[derive(Clone, Copy)]
pub struct Services<'services> {
    pub database: &'services Pool,
    pub cipher: &'services Cipher,
    pub storage: &'services Storage,
    pub events: &'services EventBus,
}

impl std::fmt::Debug for Services<'_> {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The pool and cipher have nothing useful to print.
        formatter
            .debug_struct("Services")
            .field("storage", &self.storage)
            .finish_non_exhaustive()
    }
}

/// Which session's workspace a file comes from, and the item it belongs to.
#[derive(Debug, Clone, Copy)]
pub struct Source<'source> {
    pub session_id: i64,
    pub studio_item_id: Uuid,
    pub workspace: &'source ThreadHandle,
}

/// How keeping one file went.
#[derive(Debug)]
pub enum Kept {
    /// Stored and recorded.
    New(StudioAsset),
    /// The item already holds this version of this path.
    AlreadyKept,
    /// The file changed after the satellite announced it, so it was not kept.
    ChangedSinceAnnounced,
}

/// Why a file could not be kept. `Display` is what the item page shows.
#[derive(Debug, thiserror::Error)]
pub enum KeepError {
    #[error(
        "{path} was not kept: it is {size_bytes} bytes and the storage location's limit leaves \
         {remaining_bytes}"
    )]
    OverLimit {
        path: String,
        size_bytes: u64,
        remaining_bytes: u64,
    },
    #[error("{path} could not be read from the workspace: {message}")]
    Workspace { path: String, message: String },
    #[error("{path} could not be stored: {source}")]
    Storage {
        path: String,
        #[source]
        source: StorageError,
    },
    /// Elysium itself failed; logged in full, shown to the user as a fixed message.
    #[error("{path} could not be kept because Elysium failed; see the API logs")]
    Internal {
        path: String,
        #[source]
        source: anyhow::Error,
    },
}

/// Keeps one announced file for its item, and records the outcome on the item: a failure
/// becomes its `pull_error`, and a kept file clears it.
pub async fn keep_and_report(services: Services<'_>, source: Source<'_>, artifact: &Artifact) {
    let outcome = keep(services, source, artifact).await;
    let pull_error = match &outcome {
        Ok(Kept::New(_)) => None,
        Ok(Kept::AlreadyKept | Kept::ChangedSinceAnnounced) => return,
        Err(error) => {
            event!(
                name: "studio.artifact.keep_failure",
                Level::WARN,
                session.id = source.session_id,
                studio_item.id = %source.studio_item_id,
                file.path = %artifact.path,
                error.message = %error,
                "an agent's file could not be kept",
            );
            if let KeepError::Internal { source: cause, .. } = error {
                event!(
                    name: "studio.artifact.internal_failure",
                    Level::ERROR,
                    studio_item.id = %source.studio_item_id,
                    error.message = ?cause,
                    "keeping an agent's file failed inside Elysium",
                );
            }
            Some(error.to_string())
        }
    };
    if let Err(error) = report(
        services,
        source.studio_item_id,
        outcome.as_ref().ok(),
        pull_error,
    )
    .await
    {
        event!(
            name: "studio.artifact.report_failure",
            Level::ERROR,
            studio_item.id = %source.studio_item_id,
            error.message = %error,
            "could not record how keeping a file went",
        );
    }
}

/// Streams one announced file from the workspace into the item's storage location and
/// records it.
///
/// # Errors
/// Returns [`KeepError`] when the file would pass the location's limit, cannot be read or
/// stored, or Elysium fails.
pub async fn keep(
    services: Services<'_>,
    source: Source<'_>,
    artifact: &Artifact,
) -> Result<Kept, KeepError> {
    let path = artifact.path.clone();
    let internal = |cause: anyhow::Error| KeepError::Internal {
        path: path.clone(),
        source: cause,
    };

    let mut connection = services
        .database
        .get()
        .await
        .map_err(|error| internal(error.into()))?;
    let already_kept = studio_asset::has_version(
        &mut connection,
        source.studio_item_id,
        &artifact.path,
        &artifact.sha256,
    )
    .await
    .map_err(|error| internal(error.into()))?;
    if already_kept {
        return Ok(Kept::AlreadyKept);
    }

    // Another path or version already holds these bytes: the object is kept and verified, so it
    // is recorded for this path without being read or uploaded again. Uploading would overwrite
    // a kept object, and a mismatch would then delete it out from under its other rows.
    let storage_path =
        studio_asset::storage_path(source.studio_item_id, &artifact.sha256, &artifact.path);
    let bytes_stored =
        studio_asset::is_stored(&mut connection, source.studio_item_id, &storage_path)
            .await
            .map_err(|error| internal(error.into()))?;
    if bytes_stored {
        let new_asset = NewStudioAsset {
            id: Uuid::now_v7(),
            studio_item_id: source.studio_item_id,
            session_id: Some(source.session_id),
            kind: studio_asset::kind_of(&artifact.path),
            artifact_path: artifact.path.clone(),
            content_type: artifact.content_type.clone(),
            size_bytes: i64::try_from(artifact.size_bytes).unwrap_or(i64::MAX),
            sha256: artifact.sha256.clone(),
            storage_path,
        };
        let recorded = studio_asset::create(&mut connection, &new_asset)
            .await
            .map_err(|error| internal(error.into()))?;
        return Ok(recorded.map_or(Kept::AlreadyKept, Kept::New));
    }

    let item = studio_item::find(&mut connection, source.studio_item_id)
        .await
        .map_err(|error| internal(error.into()))?;
    let location = storage_location::find(&mut connection, item.storage_location_id)
        .await
        .map_err(|error| internal(error.into()))?;
    if let Some(limit) = location.storage_limit_bytes {
        let used = studio_item::bytes_in_location(&mut connection, location.id)
            .await
            .map_err(|error| internal(error.into()))?;
        let remaining = u64::try_from(limit.saturating_sub(used)).unwrap_or_default();
        if artifact.size_bytes > remaining {
            return Err(KeepError::OverLimit {
                path,
                size_bytes: artifact.size_bytes,
                remaining_bytes: remaining,
            });
        }
    }
    drop(connection);

    let access_key = location.access_key(services.cipher).map_err(|error| {
        internal(anyhow::anyhow!(
            "the access key cannot be decrypted: {error}"
        ))
    })?;
    let file = source
        .workspace
        .read_file(&format!("artifacts/{}", artifact.path))
        .await
        .map_err(|error| KeepError::Workspace {
            path: path.clone(),
            message: error.to_string(),
        })?;
    let content_length = file.content_length();
    let content_type = artifact
        .content_type
        .clone()
        .or_else(|| file.content_type().map(ToOwned::to_owned));

    // Hash the bytes as they stream past, to learn whether they are the ones announced.
    let hasher = Arc::new(Mutex::new(Sha256::new()));
    let streamed_hasher = Arc::clone(&hasher);
    let body = file.into_body().map(move |chunk| {
        if let Ok(bytes) = &chunk {
            streamed_hasher.lock().expect(HASHER_POISONED).update(bytes);
        }
        chunk.map(|bytes: Bytes| bytes)
    });

    services
        .storage
        .upload(
            &location,
            &access_key,
            &storage_path,
            body,
            content_length,
            content_type.as_deref(),
        )
        .await
        .map_err(|error| KeepError::Storage {
            path: path.clone(),
            source: error,
        })?;

    let digest = hex::encode(hasher.lock().expect(HASHER_POISONED).clone().finalize());
    if digest != artifact.sha256 {
        // The object is named by the announced hash but holds other bytes; it must not stay. No
        // other row names it: an object already kept is never uploaded over, above.
        if let Err(error) = services
            .storage
            .delete(&location, &access_key, &storage_path)
            .await
        {
            event!(
                name: "studio.artifact.orphaned_file",
                Level::WARN,
                file.path = %storage_path,
                error.message = %error,
                "could not remove a file that changed while it was stored",
            );
        }
        return Ok(Kept::ChangedSinceAnnounced);
    }

    let new_asset = NewStudioAsset {
        id: Uuid::now_v7(),
        studio_item_id: source.studio_item_id,
        session_id: Some(source.session_id),
        kind: studio_asset::kind_of(&artifact.path),
        artifact_path: artifact.path.clone(),
        content_type,
        size_bytes: i64::try_from(content_length).unwrap_or(i64::MAX),
        sha256: artifact.sha256.clone(),
        storage_path,
    };
    let mut connection = services
        .database
        .get()
        .await
        .map_err(|error| internal(error.into()))?;
    let recorded = studio_asset::create(&mut connection, &new_asset)
        .await
        .map_err(|error| internal(error.into()))?;
    // A concurrent pull of the same version (a replayed event beside a reconcile) recorded it
    // first; the bytes are the same, so nothing more is needed.
    Ok(recorded.map_or(Kept::AlreadyKept, Kept::New))
}

/// Pulls every file the thread's `artifacts/` holds that its item has not kept yet.
///
/// Announcements can be missed: a stream that dropped past what the satellite still retains,
/// or a turn that ended while Elysium was down. So whenever the watcher attaches to a Studio
/// thread it lists the thread's artifacts and keeps each one. Keeping is idempotent, so a file
/// already kept costs one lookup. A listing that fails is logged; the next attach tries again.
pub async fn reconcile(services: Services<'_>, source: Source<'_>) {
    let artifacts = match source.workspace.artifacts().await {
        Ok(artifacts) => artifacts,
        Err(error) => {
            event!(
                name: "studio.artifact.reconcile_failure",
                Level::WARN,
                session.id = source.session_id,
                studio_item.id = %source.studio_item_id,
                error.message = %error,
                "could not list a Studio thread's artifacts to reconcile them",
            );
            return;
        }
    };
    for artifact in &artifacts {
        keep_and_report(services, source, artifact).await;
    }
}

/// Records how keeping a file went on its item, and tells clients: the new file, and the item,
/// whose counts, thumbnail, or `pull_error` it may have changed.
async fn report(
    services: Services<'_>,
    studio_item_id: Uuid,
    kept: Option<&Kept>,
    pull_error: Option<String>,
) -> anyhow::Result<()> {
    let mut connection = services.database.get().await?;
    let mut item = studio_item::find(&mut connection, studio_item_id).await?;
    if item.pull_error != pull_error {
        item = studio_item::set_pull_error(&mut connection, studio_item_id, pull_error.as_deref())
            .await?;
    }
    let assets = studio_asset::list_for_items(&mut connection, &[studio_item_id]).await?;
    drop(connection);

    if let Some(Kept::New(asset)) = kept {
        services
            .events
            .publish(&ServerEvent::StudioAssetCreated(StudioAssetResponse::from(
                asset.clone(),
            )));
    }
    let asset_references: Vec<_> = assets.iter().collect();
    services
        .events
        .publish(&ServerEvent::StudioItemUpserted(StudioItemResponse::new(
            item,
            &asset_references,
        )));
    Ok(())
}

#[cfg(test)]
mod tests;
