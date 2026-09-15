// Copyright © 2026 Jalapeno Labs

//! Bunny Storage's HTTP API.
//!
//! Every call signs in with the storage zone's password in the `AccessKey` header; the
//! account-wide API key is neither needed nor accepted. A zone answers only on its own
//! region's endpoint, and Bunny refuses a zone asked for on the wrong one with the same
//! `401` it gives a wrong password.

use std::time::Duration;

use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use url::Url;

use super::StorageError;
use crate::models::storage_location::BunnyStorageRegion;

/// How long one call may take. Listing a directory answers in well under a second; a
/// minute covers a slow link without leaving a request hanging.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Each region's storage endpoint, from Bunny's documentation. Frankfurt is the bare host.
const fn endpoint(region: BunnyStorageRegion) -> &'static str {
    match region {
        BunnyStorageRegion::Frankfurt => "https://storage.bunnycdn.com",
        BunnyStorageRegion::London => "https://uk.storage.bunnycdn.com",
        BunnyStorageRegion::NewYork => "https://ny.storage.bunnycdn.com",
        BunnyStorageRegion::LosAngeles => "https://la.storage.bunnycdn.com",
        BunnyStorageRegion::Singapore => "https://sg.storage.bunnycdn.com",
        BunnyStorageRegion::Stockholm => "https://se.storage.bunnycdn.com",
        BunnyStorageRegion::SaoPaulo => "https://br.storage.bunnycdn.com",
        BunnyStorageRegion::Johannesburg => "https://jh.storage.bunnycdn.com",
        BunnyStorageRegion::Sydney => "https://syd.storage.bunnycdn.com",
    }
}

/// The URL of a directory in a zone. Each path segment is percent-encoded, and the
/// trailing slash is what makes Bunny list the directory instead of fetching a file.
fn directory_url(region: BunnyStorageRegion, zone: &str, directory: &str) -> Url {
    let mut url = Url::parse(endpoint(region)).expect("every region endpoint is a valid URL");
    url.path_segments_mut()
        .expect("an https URL has path segments")
        .clear()
        .push(zone)
        .extend(directory.split('/').filter(|segment| !segment.is_empty()))
        .push("");
    url
}

/// Bunny's error body, such as `{"HttpCode":401,"Message":"Unauthorized"}`.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ErrorBody {
    message: String,
}

#[derive(Debug, Clone)]
pub struct Bunny {
    http: reqwest::Client,
}

impl Bunny {
    pub const fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Counts the entries, files and directories, directly inside `directory` of `zone`.
    ///
    /// A directory that does not exist yet counts as empty, whether Bunny answers `404` or
    /// an empty listing: Bunny creates directories as files are written into them.
    ///
    /// # Errors
    /// Returns [`StorageError::Unauthorized`] when Bunny refuses the password, or the zone
    /// is not in `region`, and [`StorageError::Refused`] when it cannot be reached or
    /// answers with anything else.
    pub async fn count_entries(
        &self,
        region: BunnyStorageRegion,
        zone: &str,
        directory: &str,
        access_key: &SecretString,
    ) -> Result<usize, StorageError> {
        let response = self
            .http
            .get(directory_url(region, zone, directory))
            .header("AccessKey", access_key.expose_secret())
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| StorageError::Refused(format!("Bunny Storage: {error}")))?;

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(StorageError::Unauthorized(
                "Bunny Storage refused the password; check it, and that the zone is in the chosen region",
            ));
        }
        // An unknown zone or wrong region is a 401, so a 404 past sign-in can only mean the
        // directory has not been written to yet.
        if status == StatusCode::NOT_FOUND {
            return Ok(0);
        }
        if !status.is_success() {
            let message = response
                .json::<ErrorBody>()
                .await
                .map_or_else(|_unreadable| status.to_string(), |body| body.message);
            return Err(StorageError::Refused(format!("Bunny Storage: {message}")));
        }

        // Only the count is used, so each entry's fields are left unread.
        let entries: Vec<serde::de::IgnoredAny> = response.json().await.map_err(|error| {
            StorageError::Refused(format!("Bunny Storage sent an unreadable listing: {error}"))
        })?;
        Ok(entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_urls_use_the_regions_endpoint_and_end_in_a_slash() {
        assert_eq!(
            directory_url(BunnyStorageRegion::Frankfurt, "files", "").as_str(),
            "https://storage.bunnycdn.com/files/"
        );
        assert_eq!(
            directory_url(BunnyStorageRegion::Sydney, "files", "uploads/2026").as_str(),
            "https://syd.storage.bunnycdn.com/files/uploads/2026/"
        );
    }

    #[test]
    fn directory_segments_are_percent_encoded() {
        assert_eq!(
            directory_url(BunnyStorageRegion::London, "files", "team docs/a?b#c").as_str(),
            "https://uk.storage.bunnycdn.com/files/team%20docs/a%3Fb%23c/"
        );
    }
}
