// Copyright © 2026 Jalapeno Labs

//! Files agents produced for Studio items, kept in the items' storage locations.
//!
//! Each row is one version of one artifact: the path the agent wrote it at under
//! `artifacts/`, its content hash, and where Elysium keeps the bytes. A render the next turn
//! overwrites arrives as a new row with the same path, so every version stays. See
//! `docs/studio.md`, Assets.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::on_constraint;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::schema::studio_assets;

/// How Studio shows a file.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::StudioAssetKind"]
#[serde(rename_all = "kebab-case")]
pub enum StudioAssetKind {
    /// Shown in the stage, and can be the item's thumbnail.
    Image,
    /// A 3D model; its `.glb` is shown in the viewer.
    Model,
    /// Anything else, offered as a download.
    File,
}

/// Extensions by kind, lowercase. Anything not listed is a [`StudioAssetKind::File`].
const KIND_BY_EXTENSION: [(&str, StudioAssetKind); 16] = [
    ("png", StudioAssetKind::Image),
    ("jpg", StudioAssetKind::Image),
    ("jpeg", StudioAssetKind::Image),
    ("webp", StudioAssetKind::Image),
    ("gif", StudioAssetKind::Image),
    ("svg", StudioAssetKind::Image),
    ("glb", StudioAssetKind::Model),
    ("gltf", StudioAssetKind::Model),
    ("blend", StudioAssetKind::Model),
    ("fbx", StudioAssetKind::Model),
    ("obj", StudioAssetKind::Model),
    ("stl", StudioAssetKind::Model),
    ("usd", StudioAssetKind::Model),
    ("usda", StudioAssetKind::Model),
    ("usdc", StudioAssetKind::Model),
    ("usdz", StudioAssetKind::Model),
];

/// The lowercase extension of a path's last segment, when it has a plain one: 1 to 10 ASCII
/// letters or digits. Anything else is treated as no extension, so a stored file's name never
/// carries characters an agent chose.
pub fn extension_of(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let (stem, extension) = name.rsplit_once('.')?;
    let is_plain = (1..=10).contains(&extension.len())
        && extension
            .chars()
            .all(|character| character.is_ascii_alphanumeric());
    if stem.is_empty() || !is_plain {
        return None;
    }
    Some(extension.to_ascii_lowercase())
}

/// How Studio shows the file at `artifact_path`, decided by its extension.
pub fn kind_of(artifact_path: &str) -> StudioAssetKind {
    let Some(extension) = extension_of(artifact_path) else {
        return StudioAssetKind::File;
    };
    KIND_BY_EXTENSION
        .iter()
        .find(|(known, _kind)| *known == extension)
        .map_or(StudioAssetKind::File, |(_known, kind)| *kind)
}

/// Where an item's file is kept, relative to its storage location's directory. Files are
/// named by content, so the same bytes are stored once however many paths or versions name
/// them.
pub fn storage_path(studio_item_id: Uuid, sha256: &str, artifact_path: &str) -> String {
    match extension_of(artifact_path) {
        Some(extension) => format!("{}/{sha256}.{extension}", item_directory(studio_item_id)),
        None => format!("{}/{sha256}", item_directory(studio_item_id)),
    }
}

/// The directory every file of one item is kept under, which a permanent delete empties.
pub fn item_directory(studio_item_id: Uuid) -> String {
    format!("studio/{studio_item_id}")
}

/// A stored file.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = studio_assets, check_for_backend(diesel::pg::Pg))]
pub struct StudioAsset {
    pub id: Uuid,
    pub studio_item_id: Uuid,
    /// The session whose workspace it came from.
    pub session_id: Option<i64>,
    pub kind: StudioAssetKind,
    /// Where the agent wrote it, relative to `artifacts/`.
    pub artifact_path: String,
    pub content_type: Option<String>,
    pub size_bytes: i64,
    pub sha256: String,
    /// Where it is kept, relative to the location's directory.
    pub storage_path: String,
    pub created_at: DateTime<Utc>,
}

/// A file that has just been stored.
#[derive(Debug, Insertable)]
#[diesel(table_name = studio_assets)]
pub struct NewStudioAsset {
    pub id: Uuid,
    pub studio_item_id: Uuid,
    pub session_id: Option<i64>,
    pub kind: StudioAssetKind,
    pub artifact_path: String,
    pub content_type: Option<String>,
    pub size_bytes: i64,
    pub sha256: String,
    pub storage_path: String,
}

/// Whether the item already holds this version of this path.
///
/// # Errors
/// Propagates any database error.
pub async fn has_version(
    connection: &mut AsyncPgConnection,
    studio_item_id: Uuid,
    artifact_path: &str,
    sha256: &str,
) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        studio_assets::table
            .filter(studio_assets::studio_item_id.eq(studio_item_id))
            .filter(studio_assets::artifact_path.eq(artifact_path))
            .filter(studio_assets::sha256.eq(sha256)),
    ))
    .get_result(connection)
    .await
}

/// Whether the item already keeps a file at `storage_path`. Files are named by content, so a
/// path whose bytes another path or version already has needs nothing uploaded.
///
/// # Errors
/// Propagates any database error.
pub async fn is_stored(
    connection: &mut AsyncPgConnection,
    studio_item_id: Uuid,
    storage_path: &str,
) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        studio_assets::table
            .filter(studio_assets::studio_item_id.eq(studio_item_id))
            .filter(studio_assets::storage_path.eq(storage_path)),
    ))
    .get_result(connection)
    .await
}

/// Records a stored file, or returns `None` when the same version was recorded meanwhile, as
/// when a replayed event and a reconcile pull the same file at once.
///
/// # Errors
/// Propagates any database error.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_asset: &NewStudioAsset,
) -> QueryResult<Option<StudioAsset>> {
    diesel::insert_into(studio_assets::table)
        .values(new_asset)
        .on_conflict(on_constraint("studio_assets_version_unique"))
        .do_nothing()
        .returning(StudioAsset::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// Every file of the given items, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_items(
    connection: &mut AsyncPgConnection,
    studio_item_ids: &[Uuid],
) -> QueryResult<Vec<StudioAsset>> {
    studio_assets::table
        .filter(studio_assets::studio_item_id.eq_any(studio_item_ids))
        .order((studio_assets::created_at.desc(), studio_assets::id.desc()))
        .select(StudioAsset::as_select())
        .load(connection)
        .await
}

/// One file of an item.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the item holds no file with that id.
pub async fn find_for_item(
    connection: &mut AsyncPgConnection,
    studio_item_id: Uuid,
    id: Uuid,
) -> QueryResult<StudioAsset> {
    studio_assets::table
        .find(id)
        .filter(studio_assets::studio_item_id.eq(studio_item_id))
        .select(StudioAsset::as_select())
        .first(connection)
        .await
}

/// The newest version of every path an item holds, which is what a continued item's new
/// workspace is seeded with.
///
/// # Errors
/// Propagates any database error.
pub async fn latest_per_path(
    connection: &mut AsyncPgConnection,
    studio_item_id: Uuid,
) -> QueryResult<Vec<StudioAsset>> {
    studio_assets::table
        .filter(studio_assets::studio_item_id.eq(studio_item_id))
        .distinct_on(studio_assets::artifact_path)
        .order((
            studio_assets::artifact_path,
            studio_assets::created_at.desc(),
            studio_assets::id.desc(),
        ))
        .select(StudioAsset::as_select())
        .load(connection)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_the_extension_ignoring_case() {
        assert_eq!(kind_of("renders/banana-hero.png"), StudioAssetKind::Image);
        assert_eq!(kind_of("banana.SVG"), StudioAssetKind::Image);
        assert_eq!(kind_of("banana.glb"), StudioAssetKind::Model);
        assert_eq!(kind_of("models/banana.blend"), StudioAssetKind::Model);
        assert_eq!(kind_of("notes.md"), StudioAssetKind::File);
        assert_eq!(kind_of("README"), StudioAssetKind::File);
        assert_eq!(
            kind_of(".png"),
            StudioAssetKind::File,
            "a dotfile has no extension"
        );
    }

    #[test]
    fn extensions_are_plain_or_absent() {
        assert_eq!(extension_of("a/b/c.PNG").as_deref(), Some("png"));
        assert_eq!(extension_of("archive.tar.gz").as_deref(), Some("gz"));
        assert_eq!(extension_of("dir.v2/file"), None);
        assert_eq!(extension_of("weird.p$g"), None);
        assert_eq!(extension_of("long.abcdefghijk"), None);
    }

    #[test]
    fn files_are_kept_under_the_item_by_content() {
        let item = Uuid::nil();
        let sha = "a".repeat(64);
        assert_eq!(
            storage_path(item, &sha, "renders/banana-hero.PNG"),
            format!("studio/{item}/{sha}.png")
        );
        assert_eq!(
            storage_path(item, &sha, "LICENSE"),
            format!("studio/{item}/{sha}")
        );
        assert!(storage_path(item, &sha, "x.png").starts_with(&item_directory(item)));
    }
}
