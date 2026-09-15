// Copyright © 2026 Jalapeno Labs

//! `/api/v1/storage-locations`: external locations Elysium saves files to. Access keys
//! are write-only over HTTP.

mod create_storage_location;
mod delete_storage_location;
mod get_storage_location;
mod list_storage_locations;
mod test_storage_location;
mod update_storage_location;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;
use validator::ValidationError;

use crate::errors::ApiError;
use crate::models::project::ProjectScope;
use crate::models::storage_location::{S3Service, StorageLocation, StorageProvider};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_storage_locations::handle).post(create_storage_location::handle),
        )
        .route(
            "/{id}",
            get(get_storage_location::handle)
                .patch(update_storage_location::handle)
                .delete(delete_storage_location::handle),
        )
        .route("/{id}/test", post(test_storage_location::handle))
}

/// Upper bound on a stored access key. Bunny's zone passwords are about 80 characters;
/// this leaves room for other providers without letting one row carry an arbitrary blob.
const ACCESS_KEY_MAX_BYTES: usize = 1024;

/// Upper bound on a directory prefix, matching `storage_locations_path_prefix_shape`.
const PATH_PREFIX_MAX_CHARACTERS: usize = 1024;

/// The largest storage limit, in bytes: the largest integer a browser's JSON parser
/// holds exactly (2^53 - 1, about 9 petabytes).
const STORAGE_LIMIT_MAX_BYTES: i64 = 9_007_199_254_740_991;

/// A location as clients see it. The access key is never included, sealed or not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageLocationResponse {
    id: Uuid,
    name: String,
    provider: StorageProvider,
    path_prefix: String,
    /// `None` for no limit.
    storage_limit_bytes: Option<i64>,
    /// `"*"` for every project, or the ids of the projects that save files here.
    projects: ProjectScope,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl StorageLocationResponse {
    pub fn new(location: StorageLocation, projects: ProjectScope) -> Self {
        Self {
            id: location.id,
            provider: location.provider(),
            name: location.name,
            path_prefix: location.path_prefix,
            storage_limit_bytes: location.storage_limit_bytes,
            projects,
            created_at: location.created_at,
            updated_at: location.updated_at,
        }
    }
}

/// A write that links a project that does not exist is the client's mistake.
fn refuse_unknown_projects(error: DieselError) -> ApiError {
    match error {
        DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _) => {
            ApiError::BadRequest("projects lists a project that does not exist".to_owned())
        }
        other => other.into(),
    }
}

/// Rejects names that are only whitespace; `length` alone would accept `"   "`.
fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}

/// Checks each provider's own settings.
fn validate_provider(provider: &StorageProvider) -> Result<(), ValidationError> {
    match provider {
        // Bunny names zones with letters, digits, and hyphens.
        StorageProvider::Bunny { zone, .. } => {
            let is_zone_name = (1..=64).contains(&zone.len())
                && zone
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-');
            if is_zone_name {
                return Ok(());
            }
            Err(ValidationError::new("zone")
                .with_message("the zone name must be 1 to 64 letters, digits, or hyphens".into()))
        }
        StorageProvider::S3 {
            service,
            bucket,
            region,
            access_key_id,
        } => {
            validate_bucket(bucket)?;
            match (service, region) {
                (S3Service::Aws, Some(region)) => validate_region(region)?,
                (S3Service::Aws, None) => {
                    return Err(ValidationError::new("region")
                        .with_message("an Amazon S3 bucket needs its region".into()));
                }
                (S3Service::GoogleCloud, None) => {}
                (S3Service::GoogleCloud, Some(_)) => {
                    return Err(ValidationError::new("region")
                        .with_message("Google Cloud Storage buckets take no region".into()));
                }
            }
            let is_key_id = (1..=256).contains(&access_key_id.len())
                && access_key_id
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric());
            if is_key_id {
                return Ok(());
            }
            Err(ValidationError::new("access_key_id")
                .with_message("the access key id must be 1 to 256 letters or digits".into()))
        }
    }
}

/// Bucket names both AWS and Google Cloud accept: 3 to 222 lowercase letters, digits,
/// dots, hyphens, or underscores, starting and ending with a letter or digit. Each service
/// narrows this further and answers for itself when a name breaks its own rules.
fn validate_bucket(bucket: &str) -> Result<(), ValidationError> {
    let bytes = bucket.as_bytes();
    let is_edge = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let is_bucket = (3..=222).contains(&bytes.len())
        && bytes.first().is_some_and(is_edge)
        && bytes.last().is_some_and(is_edge)
        && bytes
            .iter()
            .all(|byte| is_edge(byte) || matches!(byte, b'.' | b'-' | b'_'));
    if is_bucket {
        return Ok(());
    }
    Err(ValidationError::new("bucket").with_message(
        "the bucket name must be 3 to 222 lowercase letters, digits, dots, hyphens, or underscores"
            .into(),
    ))
}

/// An AWS region code, such as `us-east-1`. It becomes part of the endpoint's host name.
fn validate_region(region: &str) -> Result<(), ValidationError> {
    let is_region = (1..=64).contains(&region.len())
        && region
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if is_region {
        return Ok(());
    }
    Err(ValidationError::new("region")
        .with_message("the region must be an AWS region code, such as us-east-1".into()))
}

/// A directory inside the location. Surrounding slashes are ignored, since handlers
/// store the prefix without them; what remains must be plain directory names.
fn validate_path_prefix(value: &str) -> Result<(), ValidationError> {
    let prefix = value.trim_matches('/');
    if prefix.is_empty() {
        return Ok(());
    }

    if prefix.chars().count() > PATH_PREFIX_MAX_CHARACTERS {
        return Err(ValidationError::new("length")
            .with_message("the directory must be 1,024 characters or fewer".into()));
    }
    let is_plain = prefix.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment
                .chars()
                .any(|character| character.is_control() || character == '\\')
    });
    if is_plain {
        return Ok(());
    }
    Err(ValidationError::new("path").with_message(
        "the directory must be names separated by single slashes, without . or .. or backslashes"
            .into(),
    ))
}

/// An access key from a request body, checked as it is parsed.
///
/// Validation happens during deserialization rather than through `validator`,
/// because a validator error report would need to serialize the value itself.
#[derive(Debug)]
pub struct StorageAccessKey(pub SecretString);

impl<'de> Deserialize<'de> for StorageAccessKey {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        let access_key = SecretString::from(String::deserialize(deserializer)?);

        if access_key.expose_secret().trim().is_empty() {
            return Err(serde::de::Error::custom("access key must not be blank"));
        }
        if access_key.expose_secret().len() > ACCESS_KEY_MAX_BYTES {
            return Err(serde::de::Error::custom("access key exceeds 1 KiB"));
        }

        Ok(Self(access_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::storage_location::BunnyStorageRegion;

    #[test]
    fn prefixes_ignore_surrounding_slashes_and_refuse_traversal() {
        for accepted in ["", "/", "uploads", "/uploads/2026/", "team docs/Q3"] {
            validate_path_prefix(accepted).expect(accepted);
        }
        for refused in [
            "uploads//2026",
            "../secrets",
            "uploads/./x",
            "a\\b",
            "tab\there",
        ] {
            assert!(
                validate_path_prefix(refused).is_err(),
                "{refused} is refused"
            );
        }
        assert!(validate_path_prefix(&"x".repeat(1025)).is_err());
    }

    #[test]
    fn s3_buckets_need_a_valid_name_key_id_and_a_region_only_on_aws() {
        let provider =
            |service, bucket: &str, region: Option<&str>, key: &str| StorageProvider::S3 {
                service,
                bucket: bucket.to_owned(),
                region: region.map(str::to_owned),
                access_key_id: key.to_owned(),
            };

        validate_provider(&provider(
            S3Service::Aws,
            "elysium-files",
            Some("us-east-1"),
            "AKIAEXAMPLE",
        ))
        .expect("an AWS bucket");
        validate_provider(&provider(
            S3Service::GoogleCloud,
            "elysium_files",
            None,
            "GOOG1EXAMPLE",
        ))
        .expect("a Google Cloud bucket");

        for refused in [
            provider(S3Service::Aws, "elysium-files", None, "AKIAEXAMPLE"),
            provider(
                S3Service::GoogleCloud,
                "elysium-files",
                Some("us"),
                "GOOG1EXAMPLE",
            ),
            provider(S3Service::Aws, "Elysium", Some("us-east-1"), "AKIAEXAMPLE"),
            provider(S3Service::Aws, "-files", Some("us-east-1"), "AKIAEXAMPLE"),
            provider(
                S3Service::Aws,
                "elysium-files",
                Some("evil.com/"),
                "AKIAEXAMPLE",
            ),
            provider(
                S3Service::Aws,
                "elysium-files",
                Some("us-east-1"),
                "not a key",
            ),
        ] {
            assert!(
                validate_provider(&refused).is_err(),
                "{refused:?} is refused"
            );
        }
    }

    #[test]
    fn bunny_zone_names_are_letters_digits_and_hyphens() {
        let provider = |zone: &str| StorageProvider::Bunny {
            zone: zone.to_owned(),
            region: BunnyStorageRegion::Frankfurt,
        };
        validate_provider(&provider("elysium-files-01")).expect("a plain zone name");
        assert!(validate_provider(&provider("")).is_err());
        assert!(validate_provider(&provider("files/../other")).is_err());
        assert!(validate_provider(&provider(&"z".repeat(65))).is_err());
    }
}
