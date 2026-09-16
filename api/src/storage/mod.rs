// Copyright © 2026 Jalapeno Labs

//! The providers behind storage locations, reached through one [`Storage`] handle.
//!
//! Handlers never match on a location's provider. They call [`Storage`], which picks
//! the provider's client here, so a new provider changes this module and the model, not
//! the routes.
//!
//! Every file operation names its target by a path relative to the location's directory
//! (`path_prefix`), checked by [`path::RelativePath`] before any request is made. Entries
//! come back named the same way, so a path from a listing can be passed straight to
//! [`Storage::download`] or [`Storage::delete`].
//!
//! File bodies stream in both directions and are never held in memory whole; see
//! [`transfer`] for how stalled transfers are ended.

// Tests exercise every operation; outside them, only `check` has a caller until the
// storage gateway for coding agents lands. `expect` fails the build once that is no longer
// true, so this attribute cannot outlive its reason.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "file operations are called only by tests until the storage gateway lands"
    )
)]

pub mod bunny;
pub mod path;
pub mod s3;
pub mod transfer;

#[cfg(test)]
mod tests;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::Stream;
use reqwest::header::CONTENT_TYPE;
use secrecy::SecretString;
use serde::Serialize;

use crate::models::storage_location::{StorageLocation, StorageProvider};
use bunny::{Bunny, BunnyZone};
use path::RelativePath;
use s3::{S3, S3Target};
use transfer::ByteStream;

/// The largest file one upload may carry: 5 GiB, the most S3 accepts in a single
/// `PutObject`. Bunny documents no ceiling, but one bound for every provider keeps a file
/// that fits one location valid on any other. Larger files need multipart uploads.
pub const MAX_UPLOAD_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// A provider refused a call, could not be reached, or was asked for something invalid.
#[derive(Debug, Clone, thiserror::Error)]
pub enum StorageError {
    /// The provider rejected the password or key, or does not know the location there. The
    /// message says what to check, in the provider's terms.
    #[error("{0}")]
    Unauthorized(&'static str),
    /// Nothing is stored at the path.
    #[error("no file is stored at that path")]
    NotFound,
    /// The caller's request cannot be carried out as asked, such as a path that is not
    /// plain or an upload over [`MAX_UPLOAD_BYTES`]. No provider was called, or the upload
    /// was abandoned.
    #[error("{0}")]
    Invalid(String),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Directory,
}

/// A file or directory inside a location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Relative to the location's directory, without leading or trailing slashes.
    pub path: String,
    pub kind: EntryKind,
    /// A file's size; `None` for a directory.
    pub size_bytes: Option<u64>,
    /// When the provider last saw the entry change, if it says. S3 keeps no times for
    /// directories, which are only key prefixes.
    pub modified_at: Option<DateTime<Utc>>,
}

/// One page of a directory's entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub entries: Vec<Entry>,
    /// Passed back to [`Storage::list`] to read the next page; `None` on the last page.
    pub next_cursor: Option<String>,
}

/// A file being read from a provider.
pub struct Download {
    /// The file's size, when the provider sends it.
    pub content_length: Option<u64>,
    /// The file's media type, when the provider sends it.
    pub content_type: Option<String>,
    /// The file's bytes as they arrive. It ends with an error if the provider fails or
    /// stalls part way.
    pub body: ByteStream,
}

impl Download {
    /// Streams a provider's successful answer to a download.
    fn from_response(response: reqwest::Response, provider: &'static str) -> Self {
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        Self {
            content_length: response.content_length(),
            content_type,
            body: transfer::body_of(response, provider),
        }
    }
}

/// Written by hand because the body stream has no `Debug` of its own.
impl std::fmt::Debug for Download {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Download")
            .field("content_length", &self.content_length)
            .field("content_type", &self.content_type)
            .finish_non_exhaustive()
    }
}

/// A location's provider with its settings and access key, ready to call.
enum Target<'a> {
    Bunny(BunnyZone<'a>),
    S3(S3Target<'a>),
}

impl<'a> Target<'a> {
    fn new(provider: &'a StorageProvider, access_key: &'a SecretString) -> Self {
        match provider {
            StorageProvider::Bunny { zone, region } => Self::Bunny(BunnyZone {
                region: *region,
                name: zone,
                password: access_key,
            }),
            StorageProvider::S3 {
                service,
                bucket,
                region,
                access_key_id,
            } => Self::S3(S3Target {
                service: *service,
                bucket,
                region: region.as_deref(),
                access_key_id,
                secret_access_key: access_key,
            }),
        }
    }
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
        let page = self.list(location, access_key, "", None).await?;
        Ok(Listing {
            entries: page.entries.len(),
            has_more: page.next_cursor.is_some(),
        })
    }

    /// Lists one page of what sits directly inside `directory`, where `""` is the
    /// location's own directory. Pass a page's `next_cursor` back as `cursor` for the next.
    ///
    /// A directory that does not exist yet is empty. Bunny lists a whole directory in one
    /// page; S3 pages hold up to 1,000 entries.
    ///
    /// # Errors
    /// Returns [`StorageError::Invalid`] for a path that is not plain, or a cursor for a
    /// Bunny location, which never hands one out. Otherwise [`StorageError::Unauthorized`]
    /// or [`StorageError::Refused`] as the provider answers.
    pub async fn list(
        &self,
        location: &StorageLocation,
        access_key: &SecretString,
        directory: &str,
        cursor: Option<&str>,
    ) -> Result<Page, StorageError> {
        let directory = RelativePath::parse(directory)?;
        let key = directory.within(&location.path_prefix)?;
        let provider = location.provider();
        match Target::new(&provider, access_key) {
            Target::Bunny(zone) => {
                if cursor.is_some() {
                    return Err(StorageError::Invalid(
                        "Bunny Storage lists a whole directory at once, so it takes no cursor"
                            .to_owned(),
                    ));
                }
                self.bunny.list(&zone, &key, &directory).await
            }
            Target::S3(bucket) => self.s3.list(&bucket, &key, &directory, cursor).await,
        }
    }

    /// Describes the file or directory at `path`, or `None` when nothing is there.
    ///
    /// # Errors
    /// Returns [`StorageError::Invalid`] for a path that is not plain or is empty, and
    /// otherwise [`StorageError::Unauthorized`] or [`StorageError::Refused`] as the
    /// provider answers.
    pub async fn stat(
        &self,
        location: &StorageLocation,
        access_key: &SecretString,
        path: &str,
    ) -> Result<Option<Entry>, StorageError> {
        let path = RelativePath::parse_named(path)?;
        let key = path.within(&location.path_prefix)?;
        let provider = location.provider();
        match Target::new(&provider, access_key) {
            Target::Bunny(zone) => self.bunny.stat(&zone, &key, &path).await,
            Target::S3(bucket) => self.s3.stat(&bucket, &key, &path).await,
        }
    }

    /// Opens the file at `path` for reading, streamed as the provider sends it.
    ///
    /// # Errors
    /// Returns [`StorageError::NotFound`] when no file is there, [`StorageError::Invalid`]
    /// for a path that is not plain or is empty, and otherwise
    /// [`StorageError::Unauthorized`] or [`StorageError::Refused`] as the provider answers.
    pub async fn download(
        &self,
        location: &StorageLocation,
        access_key: &SecretString,
        path: &str,
    ) -> Result<Download, StorageError> {
        let path = RelativePath::parse_named(path)?;
        let key = path.within(&location.path_prefix)?;
        let provider = location.provider();
        match Target::new(&provider, access_key) {
            Target::Bunny(zone) => self.bunny.download(&zone, &key).await,
            Target::S3(bucket) => self.s3.download(&bucket, &key).await,
        }
    }

    /// Writes `body`, exactly `content_length` bytes, to the file at `path`, replacing any
    /// file already there. Directories along the path need not exist.
    ///
    /// Returns the written file as an entry built from what was sent: the providers answer
    /// an upload without the file's details, so `modified_at` is `None`.
    ///
    /// # Errors
    /// Returns [`StorageError::Invalid`] for a path that is not plain or is empty, a
    /// `content_length` over [`MAX_UPLOAD_BYTES`], or a body longer or shorter than
    /// `content_length`. Otherwise [`StorageError::Unauthorized`] or
    /// [`StorageError::Refused`] as the provider answers, or when the body fails or stalls.
    pub async fn upload<Chunks, ChunkError>(
        &self,
        location: &StorageLocation,
        access_key: &SecretString,
        path: &str,
        body: Chunks,
        content_length: u64,
        content_type: Option<&str>,
    ) -> Result<Entry, StorageError>
    where
        Chunks: Stream<Item = Result<Bytes, ChunkError>> + Send + 'static,
        ChunkError: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let path = RelativePath::parse_named(path)?;
        if content_length > MAX_UPLOAD_BYTES {
            return Err(StorageError::Invalid(format!(
                "a file must be {MAX_UPLOAD_BYTES} bytes (5 GiB) or smaller to upload"
            )));
        }
        let key = path.within(&location.path_prefix)?;
        let provider = location.provider();
        match Target::new(&provider, access_key) {
            Target::Bunny(zone) => {
                self.bunny
                    .upload(&zone, &key, body, content_length, content_type)
                    .await?;
            }
            Target::S3(bucket) => {
                self.s3
                    .upload(&bucket, &key, body, content_length, content_type)
                    .await?;
            }
        }
        Ok(Entry {
            path: path.as_str().to_owned(),
            kind: EntryKind::File,
            size_bytes: Some(content_length),
            modified_at: None,
        })
    }

    /// Deletes the file at `path`. Deleting a file that is not there succeeds, so a retried
    /// delete is harmless.
    ///
    /// Only files are deleted. The path is described first, because Bunny deletes a
    /// directory with everything inside it, and on S3 a directory is only a prefix that no
    /// single delete removes.
    ///
    /// # Errors
    /// Returns [`StorageError::Invalid`] for a path that is not plain, is empty, or names a
    /// directory, and otherwise [`StorageError::Unauthorized`] or [`StorageError::Refused`]
    /// as the provider answers.
    pub async fn delete(
        &self,
        location: &StorageLocation,
        access_key: &SecretString,
        path: &str,
    ) -> Result<(), StorageError> {
        let path = RelativePath::parse_named(path)?;
        let key = path.within(&location.path_prefix)?;
        let provider = location.provider();
        let target = Target::new(&provider, access_key);

        let described = match &target {
            Target::Bunny(zone) => self.bunny.stat(zone, &key, &path).await?,
            Target::S3(bucket) => self.s3.stat(bucket, &key, &path).await?,
        };
        let Some(entry) = described else {
            return Ok(());
        };
        if entry.kind == EntryKind::Directory {
            return Err(StorageError::Invalid(format!(
                "\"{}\" is a directory; only files are deleted",
                entry.path
            )));
        }

        match &target {
            Target::Bunny(zone) => self.bunny.delete(zone, &key).await,
            Target::S3(bucket) => self.s3.delete(bucket, &key).await,
        }
    }
}
