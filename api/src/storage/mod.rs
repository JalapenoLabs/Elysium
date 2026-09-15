// Copyright © 2026 Jalapeno Labs

//! The providers behind storage locations, reached through one [`Storage`] handle.
//!
//! Handlers never match on a location's provider. They call [`Storage`], which picks
//! the provider's client here, so a new provider changes this module and the model, not
//! the routes.

pub mod bunny;

use secrecy::SecretString;

use crate::models::storage_location::{StorageLocation, StorageProvider};
use bunny::Bunny;

/// A provider refused a call or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The provider rejected the access key, or does not know the location there.
    #[error(
        "the storage provider refused the access key; check the key, and that the zone is in the chosen region"
    )]
    Unauthorized,
    #[error("{0}")]
    Refused(String),
}

/// Clients for every storage provider. Cheap to clone; clones share one HTTP client.
#[derive(Debug, Clone)]
pub struct Storage {
    bunny: Bunny,
}

impl Storage {
    pub const fn new(http: reqwest::Client) -> Self {
        Self {
            bunny: Bunny::new(http),
        }
    }

    /// Lists the location's directory with its saved settings, proving Elysium can reach
    /// it, and returns how many entries sit directly inside.
    ///
    /// # Errors
    /// Returns [`StorageError`] when the provider refuses the access key or the request.
    pub async fn check(
        &self,
        location: &StorageLocation,
        access_key: &SecretString,
    ) -> Result<usize, StorageError> {
        match location.provider() {
            StorageProvider::Bunny { zone, region } => {
                self.bunny
                    .count_entries(region, &zone, &location.path_prefix, access_key)
                    .await
            }
        }
    }
}
