// Copyright © 2026 Jalapeno Labs

//! How an agent's deliverables reach its Studio item.
//!
//! After every turn of a Studio session, [`collect`] exports a `.glb` beside every `.blend`
//! the agent left in `artifacts/`, then lists that directory with each file's size and hash,
//! and [`keep`] streams every file the item does not hold yet out of the workspace into the
//! item's storage location, records it, and tells clients. Nothing is held in memory.
//!
//! The agent owns the workspace and may change a file between the listing and the read, so
//! the bytes are hashed again as they stream, and a file that no longer matches its listing
//! is dropped from storage and not recorded: the next turn's listing finds it.
//!
//! A file that cannot be kept (over the location's storage limit, refused by the provider)
//! is recorded on the item as its `pull_error`, which the item page shows, and the next file
//! that is kept clears it. See `docs/studio.md`, Assets.

use std::sync::{Arc, Mutex};

use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt as _;
use sha2::{Digest as _, Sha256};
use tracing::{Level, event};
use uuid::Uuid;

use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::storage_location::{self, StorageLocation};
use crate::models::studio_asset::{self, NewStudioAsset, StudioAsset};
use crate::models::studio_item;
use crate::models::studio_reservation::{self, Room};
use crate::models::user::CODING_AGENT_USER_ID;
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::studio_items::{StudioAssetResponse, StudioItemResponse};
use crate::runner::WorkspacePath;
use crate::runner::files::ListedFile;
use crate::runner::workspace::{AGENT_USER, WorkspaceContainer};
use crate::storage::{Storage, StorageError};

/// The directory of a session's workspace whose files are the item's deliverables.
const ARTIFACTS_DIRECTORY: &str = "artifacts";

/// Exports every `.blend` in `artifacts/` to a `.glb` beside it (`/opt/elysium/bin/export-glb`
/// in the workspace image). `$1` is the session's directory.
const EXPORT_SCRIPT: &str = r#"cd -- "$1" && exec /opt/elysium/bin/export-glb"#;

/// How long exporting may take after a turn. The exporter allows each model ten minutes; a
/// turn that left several dense scenes is still bounded.
const EXPORT_TIMEOUT: Duration = Duration::from_mins(30);

/// Media types by extension, for files whose type the item's viewer and storage provider need.
/// Anything else is stored without one.
const CONTENT_TYPE_BY_EXTENSION: &[(&str, &str)] = &[
    ("glb", "model/gltf-binary"),
    ("gltf", "model/gltf+json"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("webp", "image/webp"),
    ("gif", "image/gif"),
    ("svg", "image/svg+xml"),
    ("blend", "application/x-blender"),
];

/// One file in `artifacts/`, as the listing found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Relative to `artifacts/`.
    pub path: String,
    pub size_bytes: u64,
    /// Lowercase hex.
    pub sha256: String,
    pub content_type: Option<String>,
}

impl From<ListedFile> for Artifact {
    fn from(listed: ListedFile) -> Self {
        let content_type = listed
            .path
            .rsplit_once('.')
            .map(|(_stem, extension)| extension.to_ascii_lowercase())
            .and_then(|extension| {
                CONTENT_TYPE_BY_EXTENSION
                    .iter()
                    .find(|(known, _type)| *known == extension)
                    .map(|(_known, content_type)| (*content_type).to_owned())
            });
        Self {
            path: listed.path,
            size_bytes: listed.size_bytes,
            sha256: listed.sha256,
            content_type,
        }
    }
}

/// Hasher locks are held only to feed one chunk or read the digest, never across an await.
const HASHER_POISONED: &str = "artifact hasher lock poisoned by an earlier panic";

/// What keeping a file uses. Borrowed from the sessions service, which owns them.
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

/// Which session's directory a file comes from, and the item it belongs to.
#[derive(Debug, Clone, Copy)]
pub struct Source<'source> {
    pub session_id: i64,
    pub studio_item_id: Uuid,
    pub workspace: &'source WorkspaceContainer,
    pub directory: &'source WorkspacePath,
}

/// How keeping one file went.
#[derive(Debug)]
pub enum Kept {
    /// Stored and recorded.
    New(StudioAsset),
    /// The item already holds this version of this path.
    AlreadyHeld,
    /// The file changed after it was listed, so it was not kept.
    ChangedSinceListed,
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

/// Keeps one listed file for its item, and records the outcome on the item: a failure
/// becomes its `pull_error`, and a kept file clears it.
pub async fn keep_and_report(services: Services<'_>, source: Source<'_>, artifact: &Artifact) {
    let outcome = keep(services, source, artifact).await;
    let pull_error = match &outcome {
        Ok(Kept::New(_)) => None,
        Ok(Kept::AlreadyHeld | Kept::ChangedSinceListed) => return,
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
            Some(studio_item::pull_error_from(&error.to_string()))
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

/// Streams one listed file from the workspace into the item's storage location and
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
    let mut connection = services
        .database
        .get()
        .await
        .map_err(|error| internal(artifact, error.into()))?;
    let already_held = studio_asset::has_version(
        &mut connection,
        source.studio_item_id,
        &artifact.path,
        &artifact.sha256,
    )
    .await
    .map_err(|error| internal(artifact, error.into()))?;
    if already_held {
        return Ok(Kept::AlreadyHeld);
    }

    // Another path or version already holds these bytes: the object is kept and verified, so it
    // is recorded for this path without being read or uploaded again. Uploading would overwrite
    // a kept object, and a mismatch would then delete it out from under its other rows.
    let storage_path =
        studio_asset::storage_path(source.studio_item_id, &artifact.sha256, &artifact.path);
    let bytes_stored =
        studio_asset::is_stored(&mut connection, source.studio_item_id, &storage_path)
            .await
            .map_err(|error| internal(artifact, error.into()))?;
    drop(connection);
    if bytes_stored {
        let stored = Stored {
            storage_path,
            size_bytes: artifact.size_bytes,
            content_type: artifact.content_type.clone(),
        };
        return record(services, source, artifact, stored).await;
    }

    let location = location_of(services, source, artifact).await?;
    let room = reserve_room(services, artifact, &location).await?;
    let kept = match stream_into_storage(services, source, artifact, &location, storage_path).await
    {
        Ok(Some(stored)) => record(services, source, artifact, stored).await,
        Ok(None) => Ok(Kept::ChangedSinceListed),
        Err(error) => Err(error),
    };
    release_room(services, room).await;
    kept
}

/// Where a file's bytes are kept, as recorded on its row.
#[derive(Debug)]
struct Stored {
    storage_path: String,
    size_bytes: u64,
    content_type: Option<String>,
}

fn internal(artifact: &Artifact, cause: anyhow::Error) -> KeepError {
    KeepError::Internal {
        path: artifact.path.clone(),
        source: cause,
    }
}

/// The item's storage location.
async fn location_of(
    services: Services<'_>,
    source: Source<'_>,
    artifact: &Artifact,
) -> Result<StorageLocation, KeepError> {
    let mut connection = services
        .database
        .get()
        .await
        .map_err(|error| internal(artifact, error.into()))?;
    let item = studio_item::find(&mut connection, source.studio_item_id)
        .await
        .map_err(|error| internal(artifact, error.into()))?;
    storage_location::find(&mut connection, item.storage_location_id)
        .await
        .map_err(|error| internal(artifact, error.into()))
}

/// Reserves the file's bytes in a location with a limit, so concurrent uploads cannot pass it
/// together. The coding agent delivered the file, so the reservation is made for it.
async fn reserve_room(
    services: Services<'_>,
    artifact: &Artifact,
    location: &StorageLocation,
) -> Result<Room, KeepError> {
    let mut connection = services
        .database
        .get()
        .await
        .map_err(|error| internal(artifact, error.into()))?;
    let room = studio_reservation::reserve(
        &mut connection,
        location,
        artifact.size_bytes,
        CODING_AGENT_USER_ID,
    )
    .await
    .map_err(|error| internal(artifact, error.into()))?;
    if let Room::Full { remaining_bytes } = room {
        return Err(KeepError::OverLimit {
            path: artifact.path.clone(),
            size_bytes: artifact.size_bytes,
            remaining_bytes,
        });
    }
    Ok(room)
}

/// Releases a reservation once its upload is recorded or refused. A release that fails is
/// logged; the reservation stops counting on its own within the hour.
async fn release_room(services: Services<'_>, room: Room) {
    let released = match services.database.get().await {
        Ok(mut connection) => studio_reservation::release(&mut connection, room)
            .await
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    if let Err(message) = released {
        event!(
            name: "studio.storage.reservation_unreleased",
            Level::WARN,
            error.message = %message,
            "could not release a storage reservation; it stops counting within the hour",
        );
    }
}

/// Streams the file from the workspace into storage at `storage_path`, hashing it as it
/// passes. Answers `None`, with nothing left in storage, when the bytes are not the ones
/// listed.
async fn stream_into_storage(
    services: Services<'_>,
    source: Source<'_>,
    artifact: &Artifact,
    location: &StorageLocation,
    storage_path: String,
) -> Result<Option<Stored>, KeepError> {
    let access_key = location.access_key(services.cipher).map_err(|error| {
        internal(
            artifact,
            anyhow::anyhow!("the access key cannot be decrypted: {error}"),
        )
    })?;
    let file = source
        .workspace
        .read_file(
            source.directory,
            &format!("{ARTIFACTS_DIRECTORY}/{}", artifact.path),
        )
        .await
        .map_err(|error| KeepError::Workspace {
            path: artifact.path.clone(),
            message: error.to_string(),
        })?;
    let content_length = file.content_length;
    let content_type = artifact.content_type.clone();

    // Hash the bytes as they stream past, to learn whether they are the ones listed.
    let hasher = Arc::new(Mutex::new(Sha256::new()));
    let streamed_hasher = Arc::clone(&hasher);
    let body = file.body.map(move |chunk| {
        if let Ok(bytes) = &chunk {
            streamed_hasher.lock().expect(HASHER_POISONED).update(bytes);
        }
        chunk.map(|bytes: Bytes| bytes)
    });

    services
        .storage
        .upload(
            location,
            &access_key,
            &storage_path,
            body,
            content_length,
            content_type.as_deref(),
        )
        .await
        .map_err(|error| KeepError::Storage {
            path: artifact.path.clone(),
            source: error,
        })?;

    let digest = hex::encode(hasher.lock().expect(HASHER_POISONED).clone().finalize());
    if digest == artifact.sha256 {
        return Ok(Some(Stored {
            storage_path,
            size_bytes: content_length,
            content_type,
        }));
    }

    // The object is named by the listed hash but holds other bytes; it must not stay. No
    // other row names it: an object already kept is never uploaded over (see `keep`).
    if let Err(error) = services
        .storage
        .delete(location, &access_key, &storage_path)
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
    Ok(None)
}

/// Records a kept file for the item.
async fn record(
    services: Services<'_>,
    source: Source<'_>,
    artifact: &Artifact,
    stored: Stored,
) -> Result<Kept, KeepError> {
    let new_asset = NewStudioAsset {
        id: Uuid::now_v7(),
        studio_item_id: source.studio_item_id,
        session_id: Some(source.session_id),
        kind: studio_asset::kind_of(&artifact.path),
        artifact_path: artifact.path.clone(),
        content_type: stored.content_type,
        size_bytes: i64::try_from(stored.size_bytes).unwrap_or(i64::MAX),
        sha256: artifact.sha256.clone(),
        storage_path: stored.storage_path,
    };
    let mut connection = services
        .database
        .get()
        .await
        .map_err(|error| internal(artifact, error.into()))?;
    let recorded = studio_asset::create(&mut connection, &new_asset)
        .await
        .map_err(|error| internal(artifact, error.into()))?;
    // A concurrent collection of the same version recorded it first; the bytes are the same,
    // so nothing more is needed.
    Ok(recorded.map_or(Kept::AlreadyHeld, Kept::New))
}

/// Collects what a turn delivered: exports a `.glb` for every `.blend`, then keeps every file
/// in `artifacts/` the item does not hold yet.
///
/// Keeping is idempotent, so a file already kept costs one lookup. An export or listing that
/// fails is logged; the next turn collects again.
pub async fn collect(services: Services<'_>, source: Source<'_>) {
    let exported = source
        .workspace
        .run_script(
            AGENT_USER,
            EXPORT_SCRIPT,
            &[&source.directory.absolute()],
            Vec::new(),
            EXPORT_TIMEOUT,
        )
        .await;
    match exported {
        Ok(output) if output.exit_code == Some(0) => {}
        Ok(output) => event!(
            name: "studio.artifact.export_failure",
            Level::WARN,
            session.id = source.session_id,
            studio_item.id = %source.studio_item_id,
            error.message = %output.output.trim(),
            "a model could not be exported to glTF; its .blend is still kept",
        ),
        Err(error) => event!(
            name: "studio.artifact.export_failure",
            Level::WARN,
            session.id = source.session_id,
            studio_item.id = %source.studio_item_id,
            error.message = %error,
            "the glTF export could not run; every .blend is still kept",
        ),
    }

    let listed = match source
        .workspace
        .list_files(source.directory, ARTIFACTS_DIRECTORY)
        .await
    {
        Ok(listed) => listed,
        Err(error) => {
            event!(
                name: "studio.artifact.listing_failure",
                Level::WARN,
                session.id = source.session_id,
                studio_item.id = %source.studio_item_id,
                error.message = %error,
                "could not list a Studio session's artifacts",
            );
            return;
        }
    };
    for listed_file in listed {
        keep_and_report(services, source, &Artifact::from(listed_file)).await;
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
mod tests {
    use super::*;

    #[test]
    fn a_listed_file_takes_its_media_type_from_its_extension() {
        let listed = |path: &str| {
            Artifact::from(ListedFile {
                path: path.to_owned(),
                size_bytes: 1,
                sha256: "abc".to_owned(),
            })
        };
        assert_eq!(
            listed("banana.GLB").content_type.as_deref(),
            Some("model/gltf-binary")
        );
        assert_eq!(
            listed("renders/banana-hero.png").content_type.as_deref(),
            Some("image/png")
        );
        assert_eq!(listed("notes").content_type, None);
        assert_eq!(listed("archive.tar.zst").content_type, None);
    }
}
