// Copyright © 2026 Jalapeno Labs

//! External locations Elysium saves files to.
//!
//! Each location has a provider, which decides where files go and how Elysium signs in:
//! today a Bunny Storage zone. The access key never exists in Postgres as plaintext. It
//! is sealed here, bound to the row's id, before the insert or update leaves the process.
//! Callers hand in a [`SecretString`] and only ever get one back from
//! [`StorageLocation::access_key`].

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::storage_locations;

/// Which provider holds a location's files.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::StorageLocationKind"]
#[serde(rename_all = "kebab-case")]
pub enum StorageLocationKind {
    Bunny,
}

/// A Bunny Storage region. A zone lives in one, and answers only on that region's endpoint.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::BunnyStorageRegion"]
#[serde(rename_all = "kebab-case")]
pub enum BunnyStorageRegion {
    Frankfurt,
    London,
    NewYork,
    LosAngeles,
    Singapore,
    Stockholm,
    SaoPaulo,
    Johannesburg,
    Sydney,
}

/// Where a location's files go, with the settings only that provider has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum StorageProvider {
    Bunny {
        /// The storage zone's name.
        zone: String,
        region: BunnyStorageRegion,
    },
}

/// A stored location. `access_key_encrypted` is an envelope from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = storage_locations, check_for_backend(diesel::pg::Pg))]
pub struct StorageLocation {
    pub id: Uuid,
    pub name: String,
    pub kind: StorageLocationKind,
    pub bunny_zone: Option<String>,
    pub bunny_region: Option<BunnyStorageRegion>,
    /// The directory Elysium writes under, without leading or trailing slashes.
    pub path_prefix: String,
    pub storage_limit_bytes: i64,
    pub access_key_encrypted: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields for a new location, with the access key still in plaintext.
#[derive(Debug)]
pub struct NewStorageLocation {
    pub name: String,
    pub provider: StorageProvider,
    pub path_prefix: String,
    pub storage_limit_bytes: i64,
    pub access_key: SecretString,
}

/// A partial update. `None` leaves a column untouched; a provider replaces all of its
/// settings together.
#[derive(Debug, Default)]
pub struct StorageLocationChanges {
    pub name: Option<String>,
    pub provider: Option<StorageProvider>,
    pub path_prefix: Option<String>,
    pub storage_limit_bytes: Option<i64>,
    pub access_key: Option<SecretString>,
}

impl StorageLocationChanges {
    /// True when applying these changes would not touch any column.
    pub const fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.provider.is_none()
            && self.path_prefix.is_none()
            && self.storage_limit_bytes.is_none()
            && self.access_key.is_none()
    }
}

/// Row-shaped insert, holding the already sealed access key.
#[derive(Insertable)]
#[diesel(table_name = storage_locations)]
struct StorageLocationRow<'a> {
    id: Uuid,
    name: &'a str,
    kind: StorageLocationKind,
    bunny_zone: Option<&'a str>,
    bunny_region: Option<BunnyStorageRegion>,
    path_prefix: &'a str,
    storage_limit_bytes: i64,
    access_key_encrypted: Vec<u8>,
}

/// Row-shaped update, holding the already sealed access key if one was supplied.
#[derive(AsChangeset)]
#[diesel(table_name = storage_locations)]
struct StorageLocationChangeset<'a> {
    name: Option<&'a str>,
    kind: Option<StorageLocationKind>,
    bunny_zone: Option<&'a str>,
    bunny_region: Option<BunnyStorageRegion>,
    path_prefix: Option<&'a str>,
    storage_limit_bytes: Option<i64>,
    access_key_encrypted: Option<Vec<u8>>,
}

/// The provider's columns: its kind, then Bunny's zone and region.
const fn provider_columns(
    provider: &StorageProvider,
) -> (
    StorageLocationKind,
    Option<&str>,
    Option<BunnyStorageRegion>,
) {
    match provider {
        StorageProvider::Bunny { zone, region } => (
            StorageLocationKind::Bunny,
            Some(zone.as_str()),
            Some(*region),
        ),
    }
}

/// Associated data binding a sealed access key to its row, so a ciphertext copied into
/// another row refuses to open.
fn access_key_context(id: Uuid) -> Vec<u8> {
    let mut context = b"storage_locations.access_key:".to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

impl StorageLocation {
    /// Where this location's files go.
    ///
    /// # Panics
    /// Panics if the provider's columns are missing, which the
    /// `storage_locations_bunny_fields` constraint rules out.
    pub fn provider(&self) -> StorageProvider {
        match self.kind {
            StorageLocationKind::Bunny => StorageProvider::Bunny {
                zone: self
                    .bunny_zone
                    .clone()
                    .expect("storage_locations_bunny_fields requires a zone"),
                region: self
                    .bunny_region
                    .expect("storage_locations_bunny_fields requires a region"),
            },
        }
    }

    /// Decrypts this location's access key.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or
    /// copied from another row.
    pub fn access_key(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext = cipher.open(&self.access_key_encrypted, &access_key_context(self.id))?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }
}

/// Every location, alphabetically.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<StorageLocation>> {
    storage_locations::table
        .order(storage_locations::name.asc())
        .select(StorageLocation::as_select())
        .load(connection)
        .await
}

/// One location by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<StorageLocation> {
    storage_locations::table
        .find(id)
        .select(StorageLocation::as_select())
        .first(connection)
        .await
}

/// Seals the access key and inserts the location with a new `UUIDv7` id.
///
/// # Errors
/// Propagates database errors, including a unique violation on `name`.
pub async fn create(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    new_location: &NewStorageLocation,
) -> QueryResult<StorageLocation> {
    let id = Uuid::now_v7();
    let (kind, bunny_zone, bunny_region) = provider_columns(&new_location.provider);
    let row = StorageLocationRow {
        id,
        name: &new_location.name,
        kind,
        bunny_zone,
        bunny_region,
        path_prefix: &new_location.path_prefix,
        storage_limit_bytes: new_location.storage_limit_bytes,
        access_key_encrypted: cipher.seal(
            new_location.access_key.expose_secret().as_bytes(),
            &access_key_context(id),
        ),
    };

    diesel::insert_into(storage_locations::table)
        .values(row)
        .returning(StorageLocation::as_returning())
        .get_result(connection)
        .await
}

/// Applies `changes`, re-sealing the access key under this row's id if one is given.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id,
/// [`diesel::result::Error::QueryBuilderError`] when `changes` is empty, and any
/// other database error, including a unique violation on `name`.
pub async fn update(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    id: Uuid,
    changes: &StorageLocationChanges,
) -> QueryResult<StorageLocation> {
    let (kind, bunny_zone, bunny_region) = match changes.provider.as_ref().map(provider_columns) {
        Some((kind, bunny_zone, bunny_region)) => (Some(kind), bunny_zone, bunny_region),
        None => (None, None, None),
    };
    let changeset = StorageLocationChangeset {
        name: changes.name.as_deref(),
        kind,
        bunny_zone,
        bunny_region,
        path_prefix: changes.path_prefix.as_deref(),
        storage_limit_bytes: changes.storage_limit_bytes,
        access_key_encrypted: changes.access_key.as_ref().map(|access_key| {
            cipher.seal(
                access_key.expose_secret().as_bytes(),
                &access_key_context(id),
            )
        }),
    };

    diesel::update(storage_locations::table.find(id))
        .set(changeset)
        .returning(StorageLocation::as_returning())
        .get_result(connection)
        .await
}

/// Deletes one location. Files already written there stay with the provider.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(storage_locations::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::ApiError;
    use crate::test_support::{cipher, migrated_database};

    fn new_location(name: &str) -> NewStorageLocation {
        NewStorageLocation {
            name: name.to_owned(),
            provider: StorageProvider::Bunny {
                zone: "elysium-files".to_owned(),
                region: BunnyStorageRegion::NewYork,
            },
            path_prefix: "uploads/2026".to_owned(),
            storage_limit_bytes: 50_000_000_000,
            access_key: SecretString::from(format!("zone-password-{name}")),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn created_locations_store_only_ciphertext_and_decrypt_back() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let created = create(&mut connection, &cipher, &new_location("media"))
            .await
            .expect("insert");
        let fetched = find(&mut connection, created.id).await.expect("select");

        assert_eq!(fetched.id.get_version_num(), 7);
        assert_eq!(fetched.provider(), new_location("media").provider);
        assert_eq!(fetched.path_prefix, "uploads/2026");
        let plaintext = b"zone-password-media";
        assert!(
            !fetched
                .access_key_encrypted
                .windows(plaintext.len())
                .any(|window| window == plaintext)
        );
        assert_eq!(
            fetched
                .access_key(&cipher)
                .expect("decrypts")
                .expose_secret(),
            "zone-password-media"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_access_key_copied_into_another_row_refuses_to_open() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let victim = create(&mut connection, &cipher, &new_location("victim"))
            .await
            .expect("insert");
        let mut attacker = create(&mut connection, &cipher, &new_location("attacker"))
            .await
            .expect("insert");

        attacker
            .access_key_encrypted
            .clone_from(&victim.access_key_encrypted);
        assert_eq!(
            attacker.access_key(&cipher).unwrap_err(),
            OpenError::Inauthentic
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn update_reseals_keys_replaces_providers_and_names_stay_unique() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let original = create(&mut connection, &cipher, &new_location("rotating"))
            .await
            .expect("insert");
        create(&mut connection, &cipher, &new_location("taken"))
            .await
            .expect("insert");

        let moved = StorageProvider::Bunny {
            zone: "elysium-archive".to_owned(),
            region: BunnyStorageRegion::Frankfurt,
        };
        let changes = StorageLocationChanges {
            provider: Some(moved.clone()),
            access_key: Some(SecretString::from("zone-password-rotated")),
            ..StorageLocationChanges::default()
        };
        let updated = update(&mut connection, &cipher, original.id, &changes)
            .await
            .expect("update");
        assert_eq!(updated.provider(), moved);
        assert_eq!(
            updated.storage_limit_bytes, original.storage_limit_bytes,
            "absent fields are untouched"
        );
        assert_eq!(
            updated
                .access_key(&cipher)
                .expect("decrypts")
                .expose_secret(),
            "zone-password-rotated"
        );

        let rename = StorageLocationChanges {
            name: Some("taken".to_owned()),
            ..StorageLocationChanges::default()
        };
        let duplicate = update(&mut connection, &cipher, original.id, &rename)
            .await
            .unwrap_err();
        assert!(matches!(ApiError::from(duplicate), ApiError::Conflict(_)));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_database_refuses_malformed_prefixes_and_limits() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let mut slashed = new_location("slashed");
        slashed.path_prefix = "/uploads/".to_owned();
        create(&mut connection, &cipher, &slashed)
            .await
            .expect_err("storage_locations_path_prefix_shape rejects surrounding slashes");

        let mut unlimited = new_location("unlimited");
        unlimited.storage_limit_bytes = 0;
        create(&mut connection, &cipher, &unlimited)
            .await
            .expect_err("storage_locations_storage_limit_positive rejects a zero limit");
    }
}
