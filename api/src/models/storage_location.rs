// Copyright © 2026 Jalapeno Labs

//! External locations Elysium saves files to.
//!
//! Each location has a provider, which decides where files go and how Elysium signs in:
//! a Bunny Storage zone, or an S3 bucket on AWS or Google Cloud. The access key never exists in Postgres as plaintext. It
//! is sealed here, bound to the row's id, before the insert or update leaves the process.
//! Callers hand in a [`SecretString`] and only ever get one back from
//! [`StorageLocation::access_key`].

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::{storage_location_projects, storage_locations};
use crate::models::project::ProjectScope;

/// Which provider holds a location's files.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::StorageLocationKind"]
#[serde(rename_all = "kebab-case")]
pub enum StorageLocationKind {
    Bunny,
    S3,
}

/// A service Elysium reaches over the S3 API. Each has a fixed endpoint in code, so no
/// request can point Elysium at an arbitrary host.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::S3Service"]
#[serde(rename_all = "kebab-case")]
pub enum S3Service {
    Aws,
    GoogleCloud,
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
    S3 {
        service: S3Service,
        bucket: String,
        /// The bucket's region, such as `us-east-1`, for `aws`; `None` for `google-cloud`.
        region: Option<String>,
        /// Names the key; the secret half is the location's access key.
        access_key_id: String,
    },
}

impl StorageProvider {
    /// Whether a location moving from `stored` to these settings may keep its access key.
    ///
    /// Bunny passwords belong to one zone, and an S3 secret to one access key id on one
    /// service, so changing either needs the matching secret too.
    pub fn keeps_access_key_of(&self, stored: &Self) -> bool {
        match (self, stored) {
            (
                Self::Bunny { zone, .. },
                Self::Bunny {
                    zone: stored_zone, ..
                },
            ) => zone == stored_zone,
            (
                Self::S3 {
                    service,
                    access_key_id,
                    ..
                },
                Self::S3 {
                    service: stored_service,
                    access_key_id: stored_access_key_id,
                    ..
                },
            ) => service == stored_service && access_key_id == stored_access_key_id,
            _ => false,
        }
    }
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
    /// Elysium's cap on what it stores here; `None` for no limit.
    pub storage_limit_bytes: Option<i64>,
    pub access_key_encrypted: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Every project saves files here. Otherwise only the projects linked in
    /// `storage_location_projects` do.
    pub all_projects: bool,
    pub s3_service: Option<S3Service>,
    pub s3_bucket: Option<String>,
    pub s3_region: Option<String>,
    pub s3_access_key_id: Option<String>,
}

/// Fields for a new location, with the access key still in plaintext.
#[derive(Debug)]
pub struct NewStorageLocation {
    pub name: String,
    pub provider: StorageProvider,
    pub path_prefix: String,
    /// `None` for no limit.
    pub storage_limit_bytes: Option<i64>,
    pub access_key: SecretString,
    pub projects: ProjectScope,
}

/// A partial update. `None` leaves a column untouched; a provider replaces all of its
/// settings together.
#[derive(Debug, Default)]
pub struct StorageLocationChanges {
    pub name: Option<String>,
    pub provider: Option<StorageProvider>,
    pub path_prefix: Option<String>,
    #[expect(
        clippy::option_option,
        reason = "absent, no limit, and a limit are three distinct requests"
    )]
    pub storage_limit_bytes: Option<Option<i64>>,
    pub access_key: Option<SecretString>,
    /// Replaces the projects that save files here.
    pub projects: Option<ProjectScope>,
}

impl StorageLocationChanges {
    /// True when applying these changes would not touch any column.
    pub const fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.provider.is_none()
            && self.path_prefix.is_none()
            && self.storage_limit_bytes.is_none()
            && self.access_key.is_none()
            && self.projects.is_none()
    }
}

/// Row-shaped insert, holding the already sealed access key.
#[derive(Insertable)]
#[diesel(table_name = storage_locations)]
struct StorageLocationRow<'a> {
    id: Uuid,
    name: &'a str,
    #[diesel(embed)]
    provider: ProviderColumns,
    path_prefix: &'a str,
    storage_limit_bytes: Option<i64>,
    access_key_encrypted: Vec<u8>,
    all_projects: bool,
}

/// Row-shaped update, holding the already sealed access key if one was supplied.
#[derive(AsChangeset)]
#[diesel(table_name = storage_locations)]
struct StorageLocationChangeset<'a> {
    name: Option<&'a str>,
    path_prefix: Option<&'a str>,
    #[expect(
        clippy::option_option,
        reason = "Diesel's changeset encoding: outer None skips, Some(None) writes NULL"
    )]
    storage_limit_bytes: Option<Option<i64>>,
    access_key_encrypted: Option<Vec<u8>>,
    all_projects: Option<bool>,
}

/// One link between a location and a project that saves files to it.
#[derive(Insertable)]
#[diesel(table_name = storage_location_projects)]
struct ProjectLinkRow {
    storage_location_id: Uuid,
    project_id: Uuid,
}

/// Every provider's columns. Writing them for one provider clears the others', so a
/// location that changes provider keeps no settings from the old one.
#[derive(Insertable, AsChangeset)]
#[diesel(table_name = storage_locations, treat_none_as_null = true)]
struct ProviderColumns {
    kind: StorageLocationKind,
    bunny_zone: Option<String>,
    bunny_region: Option<BunnyStorageRegion>,
    s3_service: Option<S3Service>,
    s3_bucket: Option<String>,
    s3_region: Option<String>,
    s3_access_key_id: Option<String>,
}

fn provider_columns(provider: &StorageProvider) -> ProviderColumns {
    match provider {
        StorageProvider::Bunny { zone, region } => ProviderColumns {
            kind: StorageLocationKind::Bunny,
            bunny_zone: Some(zone.clone()),
            bunny_region: Some(*region),
            s3_service: None,
            s3_bucket: None,
            s3_region: None,
            s3_access_key_id: None,
        },
        StorageProvider::S3 {
            service,
            bucket,
            region,
            access_key_id,
        } => ProviderColumns {
            kind: StorageLocationKind::S3,
            bunny_zone: None,
            bunny_region: None,
            s3_service: Some(*service),
            s3_bucket: Some(bucket.clone()),
            s3_region: region.clone(),
            s3_access_key_id: Some(access_key_id.clone()),
        },
    }
}

/// Replaces the location's project links with `projects`. Every project needs no links:
/// the location's `all_projects` column says so instead.
async fn replace_project_links(
    connection: &mut AsyncPgConnection,
    location_id: Uuid,
    projects: &ProjectScope,
) -> QueryResult<()> {
    diesel::delete(
        storage_location_projects::table
            .filter(storage_location_projects::storage_location_id.eq(location_id)),
    )
    .execute(connection)
    .await?;

    let ProjectScope::Only(project_ids) = projects else {
        return Ok(());
    };
    let links: Vec<ProjectLinkRow> = project_ids
        .iter()
        .map(|&project_id| ProjectLinkRow {
            storage_location_id: location_id,
            project_id,
        })
        .collect();
    diesel::insert_into(storage_location_projects::table)
        .values(links)
        .execute(connection)
        .await?;
    Ok(())
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
    /// `storage_locations_bunny_fields` and `storage_locations_s3_fields` constraints rule
    /// out.
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
            StorageLocationKind::S3 => StorageProvider::S3 {
                service: self
                    .s3_service
                    .expect("storage_locations_s3_fields requires a service"),
                bucket: self
                    .s3_bucket
                    .clone()
                    .expect("storage_locations_s3_fields requires a bucket"),
                region: self.s3_region.clone(),
                access_key_id: self
                    .s3_access_key_id
                    .clone()
                    .expect("storage_locations_s3_fields requires an access key id"),
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

/// Every location alphabetically, each with the projects that save files to it.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
) -> QueryResult<Vec<(StorageLocation, ProjectScope)>> {
    let locations: Vec<StorageLocation> = storage_locations::table
        .order(storage_locations::name.asc())
        .select(StorageLocation::as_select())
        .load(connection)
        .await?;
    let links: Vec<(Uuid, Uuid)> = storage_location_projects::table
        .select((
            storage_location_projects::storage_location_id,
            storage_location_projects::project_id,
        ))
        .order((
            storage_location_projects::storage_location_id,
            storage_location_projects::project_id,
        ))
        .load(connection)
        .await?;

    let mut project_ids_by_location: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (location_id, project_id) in links {
        project_ids_by_location
            .entry(location_id)
            .or_default()
            .push(project_id);
    }

    Ok(locations
        .into_iter()
        .map(|location| {
            let projects = if location.all_projects {
                ProjectScope::All
            } else {
                ProjectScope::Only(
                    project_ids_by_location
                        .remove(&location.id)
                        .unwrap_or_default(),
                )
            };
            (location, projects)
        })
        .collect())
}

/// The projects that save files to `location`.
///
/// # Errors
/// Propagates any database error.
pub async fn projects_of(
    connection: &mut AsyncPgConnection,
    location: &StorageLocation,
) -> QueryResult<ProjectScope> {
    if location.all_projects {
        return Ok(ProjectScope::All);
    }
    let project_ids = storage_location_projects::table
        .filter(storage_location_projects::storage_location_id.eq(location.id))
        .select(storage_location_projects::project_id)
        .order(storage_location_projects::project_id)
        .load(connection)
        .await?;
    Ok(ProjectScope::Only(project_ids))
}

/// Every location a project saves files to, alphabetically: those for every project, and
/// those linked to this one.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_project(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Vec<StorageLocation>> {
    let linked = storage_location_projects::table
        .filter(storage_location_projects::project_id.eq(project_id))
        .select(storage_location_projects::storage_location_id);
    storage_locations::table
        .filter(storage_locations::all_projects.or(storage_locations::id.eq_any(linked)))
        .order(storage_locations::name.asc())
        .select(StorageLocation::as_select())
        .load(connection)
        .await
}

/// One location by id, only if the project saves files to it.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id, or the location is
/// neither for every project nor linked to this one.
pub async fn find_for_project(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    project_id: Uuid,
) -> QueryResult<StorageLocation> {
    let linked = storage_location_projects::table
        .filter(storage_location_projects::project_id.eq(project_id))
        .select(storage_location_projects::storage_location_id);
    storage_locations::table
        .find(id)
        .filter(storage_locations::all_projects.or(storage_locations::id.eq_any(linked)))
        .select(StorageLocation::as_select())
        .first(connection)
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

/// Seals the access key and inserts the location with a new `UUIDv7` id, linked to its
/// projects.
///
/// # Errors
/// Propagates database errors, including a unique violation on `name` and a foreign key
/// violation for a project that does not exist.
pub async fn create(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    new_location: &NewStorageLocation,
) -> QueryResult<StorageLocation> {
    let id = Uuid::now_v7();
    let row = StorageLocationRow {
        id,
        name: &new_location.name,
        provider: provider_columns(&new_location.provider),
        path_prefix: &new_location.path_prefix,
        storage_limit_bytes: new_location.storage_limit_bytes,
        access_key_encrypted: cipher.seal(
            new_location.access_key.expose_secret().as_bytes(),
            &access_key_context(id),
        ),
        all_projects: new_location.projects == ProjectScope::All,
    };

    connection
        .transaction(async move |connection| {
            let location = diesel::insert_into(storage_locations::table)
                .values(row)
                .returning(StorageLocation::as_returning())
                .get_result(connection)
                .await?;
            replace_project_links(connection, id, &new_location.projects).await?;
            Ok(location)
        })
        .await
}

/// Applies `changes`, re-sealing the access key under this row's id if one is given and
/// replacing the project links if projects are.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id,
/// [`diesel::result::Error::QueryBuilderError`] when `changes` is empty, and any
/// other database error, including a unique violation on `name` and a foreign key
/// violation for a project that does not exist.
pub async fn update(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    id: Uuid,
    changes: &StorageLocationChanges,
) -> QueryResult<StorageLocation> {
    let changeset = StorageLocationChangeset {
        name: changes.name.as_deref(),
        path_prefix: changes.path_prefix.as_deref(),
        storage_limit_bytes: changes.storage_limit_bytes,
        access_key_encrypted: changes.access_key.as_ref().map(|access_key| {
            cipher.seal(
                access_key.expose_secret().as_bytes(),
                &access_key_context(id),
            )
        }),
        all_projects: changes
            .projects
            .as_ref()
            .map(|projects| *projects == ProjectScope::All),
    };

    connection
        .transaction(async move |connection| {
            let location = diesel::update(storage_locations::table.find(id))
                .set((changeset, changes.provider.as_ref().map(provider_columns)))
                .returning(StorageLocation::as_returning())
                .get_result(connection)
                .await?;
            if let Some(projects) = &changes.projects {
                replace_project_links(connection, id, projects).await?;
            }
            Ok(location)
        })
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
            storage_limit_bytes: Some(50_000_000_000),
            access_key: SecretString::from(format!("zone-password-{name}")),
            projects: ProjectScope::Only(Vec::new()),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn projects_are_linked_replaced_widened_and_forgotten_with_the_project() {
        use crate::models::project::{self, NewProject};

        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let mut project_ids = Vec::new();
        for name in ["alpha", "beta"] {
            let created = project::create(
                &mut connection,
                &NewProject {
                    name: name.to_owned(),
                    description: String::new(),
                },
            )
            .await
            .expect("project");
            project_ids.push(created.id);
        }
        project_ids.sort_unstable();

        let mut linked = new_location("linked");
        linked.projects = ProjectScope::Only(project_ids.clone());
        let location = create(&mut connection, &cipher, &linked)
            .await
            .expect("insert");
        assert_eq!(
            projects_of(&mut connection, &location)
                .await
                .expect("links"),
            ProjectScope::Only(project_ids.clone())
        );

        let widen = StorageLocationChanges {
            projects: Some(ProjectScope::All),
            ..StorageLocationChanges::default()
        };
        let widened = update(&mut connection, &cipher, location.id, &widen)
            .await
            .expect("update");
        assert!(widened.all_projects);
        let listed = list(&mut connection).await.expect("list");
        assert_eq!(listed[0].1, ProjectScope::All);

        let narrow = StorageLocationChanges {
            projects: Some(ProjectScope::Only(vec![project_ids[0]])),
            ..StorageLocationChanges::default()
        };
        let narrowed = update(&mut connection, &cipher, location.id, &narrow)
            .await
            .expect("update");
        assert!(!narrowed.all_projects);

        project::delete(&mut connection, project_ids[0])
            .await
            .expect("a project with storage links can be deleted");
        assert_eq!(
            projects_of(&mut connection, &narrowed)
                .await
                .expect("links"),
            ProjectScope::Only(Vec::new()),
            "deleting a project removes its links"
        );

        let mut unknown = new_location("unknown");
        unknown.projects = ProjectScope::Only(vec![Uuid::now_v7()]);
        let error = create(&mut connection, &cipher, &unknown)
            .await
            .expect_err("an unknown project is refused");
        assert!(matches!(
            error,
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::ForeignKeyViolation,
                _
            )
        ));
        assert_eq!(
            list(&mut connection).await.expect("list").len(),
            1,
            "the refused insert rolled back"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn projects_reach_only_locations_for_every_project_or_linked_to_them() {
        use crate::models::project::{self, NewProject};

        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let mut projects = Vec::new();
        for name in ["reaching", "elsewhere"] {
            let created = project::create(
                &mut connection,
                &NewProject {
                    name: name.to_owned(),
                    description: String::new(),
                },
            )
            .await
            .expect("project");
            projects.push(created.id);
        }
        let (reaching, elsewhere) = (projects[0], projects[1]);

        let mut everyone = new_location("everyone");
        everyone.projects = ProjectScope::All;
        let everyone = create(&mut connection, &cipher, &everyone)
            .await
            .expect("insert");
        let mut linked = new_location("linked");
        linked.projects = ProjectScope::Only(vec![reaching]);
        let linked = create(&mut connection, &cipher, &linked)
            .await
            .expect("insert");
        let mut other = new_location("other");
        other.projects = ProjectScope::Only(vec![elsewhere]);
        let other = create(&mut connection, &cipher, &other)
            .await
            .expect("insert");

        let names: Vec<String> = list_for_project(&mut connection, reaching)
            .await
            .expect("list")
            .into_iter()
            .map(|location| location.name)
            .collect();
        assert_eq!(names, ["everyone", "linked"]);

        for reachable in [everyone.id, linked.id] {
            find_for_project(&mut connection, reachable, reaching)
                .await
                .expect("a location for every project or linked to this one");
        }
        let unlinked = find_for_project(&mut connection, other.id, reaching)
            .await
            .expect_err("a location linked only to another project");
        assert!(matches!(unlinked, diesel::result::Error::NotFound));

        delete(&mut connection, linked.id).await.expect("delete");
        let deleted = find_for_project(&mut connection, linked.id, reaching)
            .await
            .expect_err("a deleted location");
        assert!(matches!(deleted, diesel::result::Error::NotFound));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_location_moved_to_s3_keeps_no_bunny_settings_and_back() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let location = create(&mut connection, &cipher, &new_location("moving"))
            .await
            .expect("insert");

        let aws = StorageProvider::S3 {
            service: S3Service::Aws,
            bucket: "elysium-files".to_owned(),
            region: Some("eu-west-2".to_owned()),
            access_key_id: "AKIAEXAMPLE".to_owned(),
        };
        let to_s3 = StorageLocationChanges {
            provider: Some(aws.clone()),
            access_key: Some(SecretString::from("aws-secret")),
            ..StorageLocationChanges::default()
        };
        let moved = update(&mut connection, &cipher, location.id, &to_s3)
            .await
            .expect("update");
        assert_eq!(moved.provider(), aws);
        assert_eq!((moved.bunny_zone, moved.bunny_region), (None, None));

        let google = StorageProvider::S3 {
            service: S3Service::GoogleCloud,
            bucket: "elysium_files".to_owned(),
            region: None,
            access_key_id: "GOOG1EXAMPLE".to_owned(),
        };
        let to_google = StorageLocationChanges {
            provider: Some(google.clone()),
            ..StorageLocationChanges::default()
        };
        let regionless = update(&mut connection, &cipher, location.id, &to_google)
            .await
            .expect("update");
        assert_eq!(regionless.provider(), google);
        assert_eq!(regionless.s3_region, None, "the AWS region is cleared");

        let mut regionless_aws = new_location("regionless");
        regionless_aws.provider = StorageProvider::S3 {
            service: S3Service::Aws,
            bucket: "elysium-files".to_owned(),
            region: None,
            access_key_id: "AKIAEXAMPLE".to_owned(),
        };
        create(&mut connection, &cipher, &regionless_aws)
            .await
            .expect_err("storage_locations_s3_region requires a region on AWS");
    }

    #[test]
    fn access_keys_carry_over_only_within_one_zone_or_access_key_id() {
        let bunny = |zone: &str| StorageProvider::Bunny {
            zone: zone.to_owned(),
            region: BunnyStorageRegion::London,
        };
        let s3 = |service, key: &str| StorageProvider::S3 {
            service,
            bucket: "elysium-files".to_owned(),
            region: None,
            access_key_id: key.to_owned(),
        };

        assert!(bunny("files").keeps_access_key_of(&bunny("files")));
        assert!(!bunny("other").keeps_access_key_of(&bunny("files")));
        assert!(s3(S3Service::Aws, "AKIA1").keeps_access_key_of(&s3(S3Service::Aws, "AKIA1")));
        assert!(!s3(S3Service::Aws, "AKIA2").keeps_access_key_of(&s3(S3Service::Aws, "AKIA1")));
        assert!(
            !s3(S3Service::GoogleCloud, "AKIA1").keeps_access_key_of(&s3(S3Service::Aws, "AKIA1"))
        );
        assert!(!bunny("files").keeps_access_key_of(&s3(S3Service::Aws, "AKIA1")));
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
    async fn prefixes_are_checked_and_limits_are_positive_or_absent() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let mut slashed = new_location("slashed");
        slashed.path_prefix = "/uploads/".to_owned();
        create(&mut connection, &cipher, &slashed)
            .await
            .expect_err("storage_locations_path_prefix_shape rejects surrounding slashes");

        let mut unlimited = new_location("unlimited");
        unlimited.storage_limit_bytes = None;
        let created = create(&mut connection, &cipher, &unlimited)
            .await
            .expect("a location may have no limit");
        assert_eq!(created.storage_limit_bytes, None);

        let mut empty = new_location("empty");
        empty.storage_limit_bytes = Some(0);
        create(&mut connection, &cipher, &empty)
            .await
            .expect_err("storage_locations_storage_limit_positive rejects a zero limit");

        let lift = StorageLocationChanges {
            storage_limit_bytes: Some(None),
            ..StorageLocationChanges::default()
        };
        let original = create(&mut connection, &cipher, &new_location("lifted"))
            .await
            .expect("insert");
        let lifted = update(&mut connection, &cipher, original.id, &lift)
            .await
            .expect("update");
        assert_eq!(
            lifted.storage_limit_bytes, None,
            "Some(None) removes the limit"
        );
    }
}
