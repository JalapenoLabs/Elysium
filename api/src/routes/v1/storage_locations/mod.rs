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
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;
use validator::ValidationError;

use crate::models::storage_location::{StorageLocation, StorageProvider};
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
    storage_limit_bytes: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl StorageLocationResponse {
    pub fn new(location: StorageLocation) -> Self {
        Self {
            id: location.id,
            provider: location.provider(),
            name: location.name,
            path_prefix: location.path_prefix,
            storage_limit_bytes: location.storage_limit_bytes,
            created_at: location.created_at,
            updated_at: location.updated_at,
        }
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
    }
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
