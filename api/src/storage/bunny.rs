// Copyright © 2026 Jalapeno Labs

//! Bunny Storage's HTTP API.
//!
//! Every call signs in with the storage zone's password in the `AccessKey` header; the
//! account-wide API key is neither needed nor accepted. A zone answers only on its own
//! region's endpoint, and Bunny refuses a zone asked for on the wrong one with the same
//! `401` it gives a wrong password.
//!
//! The operations, as Bunny's documentation and its official TypeScript SDK use them:
//!
//! | Operation | Request                                                               |
//! |-----------|-----------------------------------------------------------------------|
//! | List      | `GET /<zone>/<directory>/`, with the trailing slash; a JSON array      |
//! | Stat      | `DESCRIBE /<zone>/<path>`; one JSON object shaped like a listing entry |
//! | Download  | `GET /<zone>/<path>`                                                   |
//! | Upload    | `PUT /<zone>/<path>`; answers `201`                                    |
//! | Delete    | `DELETE /<zone>/<path>`                                                |

use std::time::Duration;

use bytes::Bytes;
use chrono::{DateTime, NaiveDateTime, Utc};
use futures_util::Stream;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tracing::{Level, event};
use url::Url;

use super::path::RelativePath;
use super::transfer::{self, send_failure};
use super::{Download, Entry, EntryKind, Page, StorageError};
use crate::models::storage_location::BunnyStorageRegion;

/// How long a call that moves no file may take. Listing a directory answers in well under
/// a second; a minute covers a slow link without leaving a request hanging.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The provider's name, for error messages.
const PROVIDER: &str = "Bunny Storage";

/// Bunny's one answer to both a wrong password and a zone asked for in the wrong region.
const PASSWORD_REFUSED: &str =
    "Bunny Storage refused the password; check it, and that the zone is in the chosen region";

/// Bunny's own name for a stat request, which answers with one entry's details.
const DESCRIBE: &[u8] = b"DESCRIBE";

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

/// The URL of `key` in a zone, a file's or with `as_directory` a directory's. Each path
/// segment is percent-encoded, and a directory's trailing slash is what makes Bunny list
/// it instead of fetching a file.
fn object_url(endpoint: &str, zone: &str, key: &str, as_directory: bool) -> Url {
    let mut url = Url::parse(endpoint).expect("every region endpoint is a valid URL");
    {
        let mut segments = url
            .path_segments_mut()
            .expect("an https URL has path segments");
        segments
            .clear()
            .push(zone)
            .extend(key.split('/').filter(|segment| !segment.is_empty()));
        if as_directory {
            segments.push("");
        }
    }
    url
}

/// Bunny's error body, such as `{"HttpCode":401,"Message":"Unauthorized"}`.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ErrorBody {
    message: String,
}

/// One file or directory as Bunny describes it, in a listing or a `DESCRIBE` answer. Bunny
/// sends more fields (`Guid`, `Path`, `Checksum`, `ReplicatedZones`); only these are used.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StorageObject {
    object_name: String,
    /// Bytes for a file; Bunny sends `0` for a directory.
    length: u64,
    last_changed: String,
    is_directory: bool,
}

impl StorageObject {
    /// This object as an [`Entry`] at `path`, relative to the location.
    fn into_entry(self, path: String) -> Entry {
        let modified_at = parse_timestamp(&self.last_changed);
        if self.is_directory {
            return Entry {
                path,
                kind: EntryKind::Directory,
                size_bytes: None,
                modified_at,
            };
        }
        Entry {
            path,
            kind: EntryKind::File,
            size_bytes: Some(self.length),
            modified_at,
        }
    }
}

/// Reads one of Bunny's timestamps, which are UTC.
///
/// Bunny writes them without an offset, such as `2026-03-17T12:43:15.843`; one that
/// carries an offset is accepted too, in case Bunny starts adding one.
fn parse_timestamp(text: &str) -> Option<DateTime<Utc>> {
    if let Ok(with_offset) = DateTime::parse_from_rfc3339(text) {
        return Some(with_offset.with_timezone(&Utc));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(naive.and_utc());
    }
    event!(
        name: "storage.bunny.timestamp.unreadable",
        Level::WARN,
        timestamp = text,
        "Bunny Storage sent an unreadable timestamp {{timestamp}}; the entry has no modification time",
    );
    None
}

/// Turns a response Bunny refused into a [`StorageError`], passing successes through.
///
/// A `404` becomes [`StorageError::NotFound`]: an unknown zone or wrong region is a `401`,
/// so past sign-in a `404` can only mean nothing is stored at the path.
async fn accepted(response: Response) -> Result<Response, StorageError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if status == StatusCode::UNAUTHORIZED {
        return Err(StorageError::Unauthorized(PASSWORD_REFUSED));
    }
    if status == StatusCode::NOT_FOUND {
        return Err(StorageError::NotFound);
    }
    let message = response
        .json::<ErrorBody>()
        .await
        .map_or_else(|_unreadable| status.to_string(), |body| body.message);
    Err(StorageError::Refused(format!("{PROVIDER}: {message}")))
}

/// A storage zone and the password that opens it.
#[derive(Debug)]
pub struct BunnyZone<'a> {
    pub region: BunnyStorageRegion,
    pub name: &'a str,
    pub password: &'a SecretString,
}

#[derive(Debug, Clone)]
pub struct Bunny {
    http: reqwest::Client,
    /// Stands in for every region's endpoint, so tests can point the client at a local
    /// fake of Bunny's API. Production code has no way to set it.
    #[cfg(test)]
    test_endpoint: Option<String>,
}

impl Bunny {
    pub const fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            #[cfg(test)]
            test_endpoint: None,
        }
    }

    /// A client that sends every zone's requests to `endpoint`, such as a local fake.
    #[cfg(test)]
    pub const fn with_test_endpoint(http: reqwest::Client, endpoint: String) -> Self {
        Self {
            http,
            test_endpoint: Some(endpoint),
        }
    }

    /// A request for `key` in `zone`, signed in with the zone's password.
    fn request(
        &self,
        method: Method,
        zone: &BunnyZone<'_>,
        key: &str,
        as_directory: bool,
    ) -> RequestBuilder {
        #[cfg(not(test))]
        let endpoint = endpoint(zone.region);
        #[cfg(test)]
        let endpoint = self
            .test_endpoint
            .as_deref()
            .unwrap_or_else(|| endpoint(zone.region));
        self.http
            .request(method, object_url(endpoint, zone.name, key, as_directory))
            .header("AccessKey", zone.password.expose_secret())
    }

    /// Sends a request that moves no file, bounded by [`REQUEST_TIMEOUT`].
    async fn send(request: RequestBuilder) -> Result<Response, StorageError> {
        let response = request
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| send_failure(PROVIDER, error))?;
        accepted(response).await
    }

    /// Lists everything directly inside the directory at `key`, named relative to the
    /// location through `directory`.
    ///
    /// Bunny lists a whole directory in one response, so the page is always the last. A
    /// directory that does not exist yet is empty, whether Bunny answers `404` or `[]`:
    /// Bunny creates directories as files are written into them.
    ///
    /// # Errors
    /// Returns [`StorageError::Unauthorized`] when Bunny refuses the password, or the zone
    /// is not in its region, and [`StorageError::Refused`] when it cannot be reached or
    /// answers with anything else.
    pub async fn list(
        &self,
        zone: &BunnyZone<'_>,
        key: &str,
        directory: &RelativePath,
    ) -> Result<Page, StorageError> {
        let request = self
            .request(Method::GET, zone, key, true)
            .header(ACCEPT, "application/json");
        let response = match Self::send(request).await {
            Ok(response) => response,
            Err(StorageError::NotFound) => {
                return Ok(Page {
                    entries: Vec::new(),
                    next_cursor: None,
                });
            }
            Err(error) => return Err(error),
        };

        let objects: Vec<StorageObject> = response.json().await.map_err(|error| {
            StorageError::Refused(format!("{PROVIDER} sent an unreadable listing: {error}"))
        })?;
        let entries = objects
            .into_iter()
            .map(|object| {
                let path = directory.child(&object.object_name);
                object.into_entry(path)
            })
            .collect();
        Ok(Page {
            entries,
            next_cursor: None,
        })
    }

    /// Describes the file or directory at `key`, or `None` when nothing is there.
    ///
    /// # Errors
    /// As [`Bunny::list`].
    pub async fn stat(
        &self,
        zone: &BunnyZone<'_>,
        key: &str,
        path: &RelativePath,
    ) -> Result<Option<Entry>, StorageError> {
        let describe = Method::from_bytes(DESCRIBE).expect("DESCRIBE is a valid method name");
        let request = self
            .request(describe, zone, key, false)
            .header(ACCEPT, "application/json");
        let response = match Self::send(request).await {
            Ok(response) => response,
            Err(StorageError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };

        let object: StorageObject = response.json().await.map_err(|error| {
            StorageError::Refused(format!(
                "{PROVIDER} sent an unreadable description: {error}"
            ))
        })?;
        Ok(Some(object.into_entry(path.as_str().to_owned())))
    }

    /// Streams the file at `key`.
    ///
    /// # Errors
    /// Returns [`StorageError::NotFound`] when no file is there, and otherwise as
    /// [`Bunny::list`].
    pub async fn download(
        &self,
        zone: &BunnyZone<'_>,
        key: &str,
    ) -> Result<Download, StorageError> {
        let request = self.request(Method::GET, zone, key, false);
        let response = accepted(transfer::send_for_body(request, PROVIDER).await?).await?;
        Ok(Download::from_response(response, PROVIDER))
    }

    /// Writes `chunks`, exactly `content_length` bytes, to the file at `key`, replacing
    /// any file already there.
    ///
    /// The body is labeled `application/octet-stream` and the file's own type goes in
    /// `Override-Content-Type`, as Bunny's SDK does: Bunny answers `401` to a body it does
    /// not take for raw binary.
    ///
    /// # Errors
    /// As [`transfer::send_upload`], and otherwise as [`Bunny::list`].
    pub async fn upload<Chunks, ChunkError>(
        &self,
        zone: &BunnyZone<'_>,
        key: &str,
        chunks: Chunks,
        content_length: u64,
        content_type: Option<&str>,
    ) -> Result<(), StorageError>
    where
        Chunks: Stream<Item = Result<Bytes, ChunkError>> + Send + 'static,
        ChunkError: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let mut request = self
            .request(Method::PUT, zone, key, false)
            .header(CONTENT_TYPE, "application/octet-stream");
        if let Some(content_type) = content_type {
            request = request.header("Override-Content-Type", content_type);
        }
        let response = transfer::send_upload(request, chunks, content_length, PROVIDER).await?;
        accepted(response).await?;
        Ok(())
    }

    /// Deletes the file at `key`. Nothing there already counts as deleted.
    ///
    /// Bunny deletes a directory with everything inside it, so callers check that `key` is
    /// a file first.
    ///
    /// # Errors
    /// As [`Bunny::list`].
    pub async fn delete(&self, zone: &BunnyZone<'_>, key: &str) -> Result<(), StorageError> {
        match Self::send(self.request(Method::DELETE, zone, key, false)).await {
            Ok(_) | Err(StorageError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_use_the_regions_endpoint_and_directories_end_in_a_slash() {
        let frankfurt = endpoint(BunnyStorageRegion::Frankfurt);
        let sydney = endpoint(BunnyStorageRegion::Sydney);

        assert_eq!(
            object_url(frankfurt, "files", "", true).as_str(),
            "https://storage.bunnycdn.com/files/"
        );
        assert_eq!(
            object_url(sydney, "files", "uploads/2026", true).as_str(),
            "https://syd.storage.bunnycdn.com/files/uploads/2026/"
        );
        assert_eq!(
            object_url(sydney, "files", "uploads/2026/app.tar.gz", false).as_str(),
            "https://syd.storage.bunnycdn.com/files/uploads/2026/app.tar.gz"
        );
    }

    #[test]
    fn every_region_has_its_own_host() {
        let regions = [
            (BunnyStorageRegion::Frankfurt, "storage.bunnycdn.com"),
            (BunnyStorageRegion::London, "uk.storage.bunnycdn.com"),
            (BunnyStorageRegion::NewYork, "ny.storage.bunnycdn.com"),
            (BunnyStorageRegion::LosAngeles, "la.storage.bunnycdn.com"),
            (BunnyStorageRegion::Singapore, "sg.storage.bunnycdn.com"),
            (BunnyStorageRegion::Stockholm, "se.storage.bunnycdn.com"),
            (BunnyStorageRegion::SaoPaulo, "br.storage.bunnycdn.com"),
            (BunnyStorageRegion::Johannesburg, "jh.storage.bunnycdn.com"),
            (BunnyStorageRegion::Sydney, "syd.storage.bunnycdn.com"),
        ];
        for (region, host) in regions {
            let url = object_url(endpoint(region), "files", "a.txt", false);
            assert_eq!(url.host_str(), Some(host));
        }
    }

    #[test]
    fn segments_are_percent_encoded() {
        let london = endpoint(BunnyStorageRegion::London);
        assert_eq!(
            object_url(london, "files", "team docs/a?b#c", true).as_str(),
            "https://uk.storage.bunnycdn.com/files/team%20docs/a%3Fb%23c/"
        );
        assert_eq!(
            object_url(london, "files", "team docs/100%.txt", false).as_str(),
            "https://uk.storage.bunnycdn.com/files/team%20docs/100%25.txt"
        );
    }

    #[test]
    fn listings_parse_into_entries_named_inside_the_location() {
        // Shaped like Bunny's answer, with the fields Elysium does not read left in.
        let body = r#"[
            {
                "Guid": "5d2b0b8c-4c9e-4f0e-9e39-3a1c9b1d2f10",
                "StorageZoneName": "files",
                "Path": "/files/builds/",
                "ObjectName": "nightly",
                "Length": 0,
                "LastChanged": "2026-03-17T12:43:15.843",
                "ServerId": 0,
                "ArrayNumber": 0,
                "IsDirectory": true,
                "UserId": "user",
                "ContentType": "",
                "DateCreated": "2026-03-17T12:43:15.843",
                "StorageZoneId": 1,
                "Checksum": null,
                "ReplicatedZones": null
            },
            {
                "Guid": "8f1a3c1e-2b7d-4d6a-8a0e-6c5e4b3a2d10",
                "StorageZoneName": "files",
                "Path": "/files/builds/",
                "ObjectName": "app.tar.gz",
                "Length": 1048576,
                "LastChanged": "2026-03-18T08:00:00",
                "ServerId": 12,
                "ArrayNumber": 1,
                "IsDirectory": false,
                "UserId": "user",
                "ContentType": "application/gzip",
                "DateCreated": "2026-03-18T08:00:00",
                "StorageZoneId": 1,
                "Checksum": "D7A8FBB307D7809469CA9ABCB0082E4F8D5651E46D3CDB762D02D0BF37C9E592",
                "ReplicatedZones": "UK,NY"
            }
        ]"#;
        let directory = RelativePath::parse("builds").expect("path");
        let entries: Vec<Entry> = serde_json::from_str::<Vec<StorageObject>>(body)
            .expect("parses")
            .into_iter()
            .map(|object| {
                let path = directory.child(&object.object_name);
                object.into_entry(path)
            })
            .collect();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "builds/nightly");
        assert_eq!(entries[0].kind, EntryKind::Directory);
        assert_eq!(entries[0].size_bytes, None);
        assert_eq!(
            entries[0].modified_at.map(|time| time.to_rfc3339()),
            Some("2026-03-17T12:43:15.843+00:00".to_owned())
        );
        assert_eq!(entries[1].path, "builds/app.tar.gz");
        assert_eq!(entries[1].kind, EntryKind::File);
        assert_eq!(entries[1].size_bytes, Some(1_048_576));
    }

    #[test]
    fn timestamps_are_utc_with_or_without_an_offset() {
        let expected = "2026-03-17T12:43:15+00:00";
        for text in [
            "2026-03-17T12:43:15",
            "2026-03-17T12:43:15Z",
            "2026-03-17T14:43:15+02:00",
        ] {
            let parsed = parse_timestamp(text).expect(text);
            assert_eq!(parsed.to_rfc3339(), expected, "{text}");
        }
        assert_eq!(parse_timestamp("yesterday"), None);
    }

    #[test]
    fn error_bodies_parse() {
        let body = r#"{"HttpCode":400,"Message":"Unable to process request"}"#;
        let error: ErrorBody = serde_json::from_str(body).expect("parses");
        assert_eq!(error.message, "Unable to process request");
    }
}
