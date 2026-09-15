// Copyright © 2026 Jalapeno Labs

//! The providers behind storage locations, reached through one [`Storage`] handle.
//!
//! Handlers never match on a location's provider. They call [`Storage`], which picks
//! the provider's client here, so a new provider changes this module and the model, not
//! the routes.

pub mod bunny;
pub mod s3;

use secrecy::SecretString;
use serde::Serialize;

use crate::models::storage_location::{StorageLocation, StorageProvider};
use bunny::Bunny;
use s3::{S3, S3Target};

/// A provider refused a call or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The provider rejected the password or key, or does not know the location there. The
    /// message says what to check, in the provider's terms.
    #[error("{0}")]
    Unauthorized(&'static str),
    #[error("{0}")]
    Refused(String),
}

/// What a location's directory holds, directly inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// Files and subdirectories, up to the provider's page size.
    pub entries: usize,
    /// The directory holds more than `entries`; only one page was read.
    pub has_more: bool,
}

/// Clients for every storage provider. Cheap to clone; clones share one HTTP client.
#[derive(Debug, Clone)]
pub struct Storage {
    bunny: Bunny,
    s3: S3,
}

impl Storage {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            bunny: Bunny::new(http.clone()),
            s3: S3::new(http),
        }
    }

    /// Lists the location's directory with its saved settings, proving Elysium can reach
    /// it, and returns what sits directly inside.
    ///
    /// # Errors
    /// Returns [`StorageError`] when the provider refuses the access key or the request.
    pub async fn check(
        &self,
        location: &StorageLocation,
        access_key: &SecretString,
    ) -> Result<Listing, StorageError> {
        match location.provider() {
            StorageProvider::Bunny { zone, region } => {
                let entries = self
                    .bunny
                    .count_entries(region, &zone, &location.path_prefix, access_key)
                    .await?;
                // Bunny lists a whole directory in one response.
                Ok(Listing {
                    entries,
                    has_more: false,
                })
            }
            StorageProvider::S3 {
                service,
                bucket,
                region,
                access_key_id,
            } => {
                let target = S3Target {
                    service,
                    bucket: &bucket,
                    region: region.as_deref(),
                    access_key_id: &access_key_id,
                    secret_access_key: access_key,
                };
                self.s3.list_directory(&target, &location.path_prefix).await
            }
        }
    }
}
