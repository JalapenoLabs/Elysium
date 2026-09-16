// Copyright © 2026 Jalapeno Labs

//! Buckets reached over the S3 API: Amazon S3, and Google Cloud Storage through its
//! S3-compatible XML API.
//!
//! Requests are presigned with `rusty-s3` (AWS Signature Version 4) and sent through
//! Elysium's shared HTTP client. Endpoints are fixed per service in [`bucket`]; only the
//! bucket and, on AWS, the validated region come from a location's settings.
//!
//! Google Cloud Storage takes HMAC keys (Cloud Storage settings, Interoperability) in
//! place of AWS access keys, and accepts `auto` as the signing region.
//!
//! Presigned URLs carry their signature in the query string, so no error message here
//! ever includes a request's URL.

use std::time::Duration;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::Stream;
use instant_xml::FromXml;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, LAST_MODIFIED};
use reqwest::{RequestBuilder, Response, StatusCode};
use rusty_s3::actions::{ListObjectsV2, ListObjectsV2Response};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use secrecy::{ExposeSecret, SecretString};
use tracing::{Level, event};
use url::Url;

use super::path::RelativePath;
use super::transfer::{self, send_failure};
use super::{Download, Entry, EntryKind, Page, StorageError};
use crate::models::storage_location::S3Service;

/// How long a presigned request stays valid. It is sent at once, so a minute only covers
/// clock drift between Elysium and the service. The service checks it when a request
/// starts, so an upload that takes longer is unaffected.
const PRESIGNED_LIFETIME: Duration = Duration::from_secs(60);

/// How long a call that moves no file may take, matching Bunny's.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The most keys one listing returns. S3's own ceiling; a directory with more pages
/// through them with continuation tokens.
const LISTING_MAX_KEYS: usize = 1000;

/// Error codes that mean the key id or secret is wrong, rather than the key lacking
/// permission. `InvalidSecurity` is Google Cloud's code for a malformed signature.
const UNAUTHORIZED_CODES: [&str; 3] = [
    "InvalidAccessKeyId",
    "SignatureDoesNotMatch",
    "InvalidSecurity",
];

const KEY_REFUSED: &str =
    "the bucket's service refused the access key; check the access key id and secret";

/// A `HEAD` answer carries no error document, so a `403` cannot say whether the key is
/// wrong or only lacks permission. S3 also answers `403` for a missing object when the
/// key may not list the bucket.
const HEAD_REFUSED: &str = "the bucket's service refused to describe the object; check the access key id and \
     secret, and that the key may read and list the bucket";

/// A service's name as the service itself writes it, for error messages.
const fn service_name(service: S3Service) -> &'static str {
    match service {
        S3Service::Aws => "Amazon S3",
        S3Service::GoogleCloud => "Google Cloud Storage",
    }
}

/// Where a bucket is reached and how requests to it are signed.
///
/// AWS buckets use virtual-hosted URLs, which AWS recommends, except names with dots:
/// those break TLS on `<bucket>.s3.<region>.amazonaws.com`, so they use path-style URLs.
/// Google Cloud uses path-style URLs for the same reason, and `auto` as its region.
fn bucket(service: S3Service, name: &str, region: Option<&str>) -> Result<Bucket, StorageError> {
    let (endpoint, url_style, signing_region) = match service {
        S3Service::Aws => {
            let region = region.ok_or_else(|| {
                StorageError::Refused("an Amazon S3 bucket needs its region".to_owned())
            })?;
            let url_style = if name.contains('.') {
                UrlStyle::Path
            } else {
                UrlStyle::VirtualHost
            };
            (
                format!("https://s3.{region}.amazonaws.com"),
                url_style,
                region,
            )
        }
        S3Service::GoogleCloud => (
            "https://storage.googleapis.com".to_owned(),
            UrlStyle::Path,
            "auto",
        ),
    };

    let endpoint = Url::parse(&endpoint)
        .map_err(|error| StorageError::Refused(format!("invalid bucket endpoint: {error}")))?;
    Bucket::new(
        endpoint,
        url_style,
        name.to_owned(),
        signing_region.to_owned(),
    )
    .map_err(|error| StorageError::Refused(format!("invalid bucket: {error}")))
}

/// The error document S3 and Google Cloud answer with, such as
/// `<Error><Code>NoSuchBucket</Code><Message>...</Message></Error>`.
#[derive(Debug, FromXml)]
#[xml(rename = "Error")]
struct ErrorBody {
    #[xml(rename = "Code")]
    code: String,
    #[xml(rename = "Message")]
    message: String,
}

/// Explains an error response from its status and body.
///
/// Only `NoSuchKey` counts as [`StorageError::NotFound`]; a missing bucket is a
/// misconfigured location, reported with the service's own message.
fn refusal(name: &str, status: StatusCode, body: &str) -> StorageError {
    let Ok(error) = instant_xml::from_str::<ErrorBody>(body) else {
        return StorageError::Refused(format!("{name}: {status}"));
    };
    if UNAUTHORIZED_CODES.contains(&error.code.as_str()) {
        return StorageError::Unauthorized(KEY_REFUSED);
    }
    if error.code == "NoSuchKey" {
        return StorageError::NotFound;
    }
    StorageError::Refused(format!("{name}: {} ({})", error.message, error.code))
}

/// Passes a successful response through, and reads a refused one's error document.
async fn accepted(name: &str, response: Response) -> Result<Response, StorageError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response
        .text()
        .await
        .map_err(|error| StorageError::Refused(format!("{name}: {}", error.without_url())))?;
    Err(refusal(name, status, &body))
}

/// The S3 prefix for a directory's contents: its key with a trailing slash, or empty for
/// the bucket's root.
fn directory_prefix(key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    format!("{key}/")
}

/// The files and subdirectories of one listing page, named relative to the location
/// through `directory`.
///
/// A key equal to `prefix` is the directory's own marker object, which some tools create
/// for empty directories, and is not an entry inside it.
fn entries_of(
    listing: ListObjectsV2Response,
    prefix: &str,
    directory: &RelativePath,
) -> Vec<Entry> {
    let mut entries = Vec::with_capacity(listing.common_prefixes.len() + listing.contents.len());

    for common_prefix in listing.common_prefixes {
        let name = common_prefix
            .prefix
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix('/'))
            .filter(|name| !name.is_empty());
        let Some(name) = name else {
            event!(
                name: "storage.s3.listing.skipped",
                Level::DEBUG,
                storage.key = common_prefix.prefix,
                "skipped a subdirectory {{storage.key}} that no plain path can name",
            );
            continue;
        };
        entries.push(Entry {
            path: directory.child(name),
            kind: EntryKind::Directory,
            size_bytes: None,
            modified_at: None,
        });
    }

    for object in listing.contents {
        let Some(name) = object.key.strip_prefix(prefix) else {
            event!(
                name: "storage.s3.listing.skipped",
                Level::DEBUG,
                storage.key = object.key,
                "skipped a key {{storage.key}} outside the listed directory",
            );
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let modified_at = DateTime::parse_from_rfc3339(&object.last_modified)
            .map(|time| time.with_timezone(&Utc))
            .inspect_err(|error| {
                event!(
                    name: "storage.s3.timestamp.unreadable",
                    Level::WARN,
                    timestamp = object.last_modified,
                    error.message = %error,
                    "unreadable LastModified {{timestamp}}; the entry has no modification time",
                );
            })
            .ok();
        entries.push(Entry {
            path: directory.child(name),
            kind: EntryKind::File,
            size_bytes: Some(object.size),
            modified_at,
        });
    }

    entries
}

/// A file's details from a `HEAD` answer's headers.
fn file_entry(path: &RelativePath, response: &Response) -> Entry {
    let modified_at = response
        .headers()
        .get(LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .and_then(|text| DateTime::parse_from_rfc2822(text).ok())
        .map(|time| time.with_timezone(&Utc));
    // `Response::content_length` reads the body's length, which a HEAD answer has none of.
    let size_bytes = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|text| text.parse().ok());
    Entry {
        path: path.as_str().to_owned(),
        kind: EntryKind::File,
        size_bytes,
        modified_at,
    }
}

#[derive(Debug, Clone)]
pub struct S3 {
    http: reqwest::Client,
}

/// A bucket and the key that opens it.
#[derive(Debug)]
pub struct S3Target<'a> {
    pub service: S3Service,
    pub bucket: &'a str,
    pub region: Option<&'a str>,
    pub access_key_id: &'a str,
    pub secret_access_key: &'a SecretString,
}

impl S3Target<'_> {
    /// The bucket and credentials that presign this target's requests.
    fn signing(&self) -> Result<(Bucket, Credentials), StorageError> {
        let bucket = bucket(self.service, self.bucket, self.region)?;
        let credentials =
            Credentials::new(self.access_key_id, self.secret_access_key.expose_secret());
        Ok((bucket, credentials))
    }
}

impl S3 {
    pub const fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Sends a request that moves no file, bounded by [`REQUEST_TIMEOUT`].
    async fn send(name: &'static str, request: RequestBuilder) -> Result<Response, StorageError> {
        let response = request
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| send_failure(name, error))?;
        accepted(name, response).await
    }

    /// Reads one listing page of keys under `prefix`, grouped by directory.
    async fn list_objects(
        &self,
        target: &S3Target<'_>,
        prefix: &str,
        cursor: Option<&str>,
        max_keys: usize,
    ) -> Result<ListObjectsV2Response, StorageError> {
        let name = service_name(target.service);
        let (bucket, credentials) = target.signing()?;
        let mut action = bucket.list_objects_v2(Some(&credentials));
        action.with_prefix(prefix);
        action.with_delimiter("/");
        action.with_max_keys(max_keys);
        if let Some(cursor) = cursor {
            action.with_continuation_token(cursor);
        }

        let response = Self::send(name, self.http.get(action.sign(PRESIGNED_LIFETIME))).await?;
        let body = response
            .text()
            .await
            .map_err(|error| StorageError::Refused(format!("{name}: {}", error.without_url())))?;
        ListObjectsV2::parse_response(&body).map_err(|error| {
            StorageError::Refused(format!("{name} sent an unreadable listing: {error}"))
        })
    }

    /// Lists one page of what sits directly inside the directory at `key`, named relative
    /// to the location through `directory`. `cursor` continues from an earlier page.
    ///
    /// A directory that does not exist is empty: S3 directories are only key prefixes.
    ///
    /// # Errors
    /// Returns [`StorageError::Unauthorized`] when the service rejects the key id or
    /// secret, and [`StorageError::Refused`] when it cannot be reached or refuses the
    /// listing, such as for a missing bucket, a key without permission, or a stale cursor.
    pub async fn list(
        &self,
        target: &S3Target<'_>,
        key: &str,
        directory: &RelativePath,
        cursor: Option<&str>,
    ) -> Result<Page, StorageError> {
        let prefix = directory_prefix(key);
        let listing = self
            .list_objects(target, &prefix, cursor, LISTING_MAX_KEYS)
            .await?;
        let next_cursor = listing.next_continuation_token.clone();
        Ok(Page {
            entries: entries_of(listing, &prefix, directory),
            next_cursor,
        })
    }

    /// Describes the file or directory at `key`, or `None` when nothing is there.
    ///
    /// A key with no object is a directory when any key sits beneath it.
    ///
    /// # Errors
    /// As [`S3::list`], with a `403` reported as [`StorageError::Unauthorized`] since a
    /// `HEAD` answer does not say why it was refused.
    pub async fn stat(
        &self,
        target: &S3Target<'_>,
        key: &str,
        path: &RelativePath,
    ) -> Result<Option<Entry>, StorageError> {
        let name = service_name(target.service);
        let (bucket, credentials) = target.signing()?;
        let url = bucket
            .head_object(Some(&credentials), key)
            .sign(PRESIGNED_LIFETIME);
        let response = self
            .http
            .head(url)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| send_failure(name, error))?;

        match response.status() {
            status if status.is_success() => return Ok(Some(file_entry(path, &response))),
            StatusCode::NOT_FOUND => {}
            StatusCode::FORBIDDEN => return Err(StorageError::Unauthorized(HEAD_REFUSED)),
            status => return Err(StorageError::Refused(format!("{name}: {status}"))),
        }

        let beneath = self
            .list_objects(target, &directory_prefix(key), None, 1)
            .await?;
        if beneath.contents.is_empty() && beneath.common_prefixes.is_empty() {
            return Ok(None);
        }
        Ok(Some(Entry {
            path: path.as_str().to_owned(),
            kind: EntryKind::Directory,
            size_bytes: None,
            modified_at: None,
        }))
    }

    /// Streams the object at `key`.
    ///
    /// # Errors
    /// Returns [`StorageError::NotFound`] when no object is there, and otherwise as
    /// [`S3::list`].
    pub async fn download(
        &self,
        target: &S3Target<'_>,
        key: &str,
    ) -> Result<Download, StorageError> {
        let name = service_name(target.service);
        let (bucket, credentials) = target.signing()?;
        let url = bucket
            .get_object(Some(&credentials), key)
            .sign(PRESIGNED_LIFETIME);
        let response = transfer::send_for_body(self.http.get(url), name).await?;
        let response = accepted(name, response).await?;
        Ok(Download::from_response(response, name))
    }

    /// Writes `chunks`, exactly `content_length` bytes, to the object at `key` in one
    /// `PutObject`, replacing any object already there.
    ///
    /// # Errors
    /// As [`transfer::send_upload`], and otherwise as [`S3::list`].
    pub async fn upload<Chunks, ChunkError>(
        &self,
        target: &S3Target<'_>,
        key: &str,
        chunks: Chunks,
        content_length: u64,
        content_type: Option<&str>,
    ) -> Result<(), StorageError>
    where
        Chunks: Stream<Item = Result<Bytes, ChunkError>> + Send + 'static,
        ChunkError: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let name = service_name(target.service);
        let (bucket, credentials) = target.signing()?;
        let url = bucket
            .put_object(Some(&credentials), key)
            .sign(PRESIGNED_LIFETIME);
        // Presigned URLs sign only the host, so the type travels as a plain header.
        let mut request = self.http.put(url);
        if let Some(content_type) = content_type {
            request = request.header(CONTENT_TYPE, content_type);
        }
        let response = transfer::send_upload(request, chunks, content_length, name).await?;
        accepted(name, response).await?;
        Ok(())
    }

    /// Deletes the object at `key`. Nothing there already counts as deleted: S3 answers
    /// `204` either way, and Google Cloud's `NoSuchKey` is treated the same.
    ///
    /// # Errors
    /// As [`S3::list`].
    pub async fn delete(&self, target: &S3Target<'_>, key: &str) -> Result<(), StorageError> {
        let name = service_name(target.service);
        let (bucket, credentials) = target.signing()?;
        let url = bucket
            .delete_object(Some(&credentials), key)
            .sign(PRESIGNED_LIFETIME);
        match Self::send(name, self.http.delete(url)).await {
            Ok(_) | Err(StorageError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> SecretString {
        SecretString::from("secret")
    }

    #[test]
    fn aws_buckets_are_virtual_hosted_unless_their_names_have_dots() {
        let plain = bucket(S3Service::Aws, "elysium-files", Some("eu-west-2")).expect("bucket");
        assert_eq!(
            plain.base_url().as_str(),
            "https://elysium-files.s3.eu-west-2.amazonaws.com/"
        );

        let dotted =
            bucket(S3Service::Aws, "files.example.com", Some("us-east-1")).expect("bucket");
        assert_eq!(
            dotted.base_url().as_str(),
            "https://s3.us-east-1.amazonaws.com/files.example.com/"
        );

        bucket(S3Service::Aws, "elysium-files", None)
            .expect_err("an AWS bucket without a region is refused");
    }

    #[test]
    fn google_cloud_buckets_use_its_fixed_endpoint_and_auto_region() {
        let gcs = bucket(S3Service::GoogleCloud, "elysium_files", None).expect("bucket");
        assert_eq!(
            gcs.base_url().as_str(),
            "https://storage.googleapis.com/elysium_files/"
        );
        assert_eq!(gcs.region(), "auto");
    }

    #[test]
    fn object_requests_are_presigned_on_each_services_url() {
        let secret = secret();
        let targets = [
            (
                S3Target {
                    service: S3Service::Aws,
                    bucket: "elysium-files",
                    region: Some("eu-west-2"),
                    access_key_id: "AKIAEXAMPLE",
                    secret_access_key: &secret,
                },
                "https://elysium-files.s3.eu-west-2.amazonaws.com/builds/app%20v2.tar.gz",
            ),
            (
                S3Target {
                    service: S3Service::Aws,
                    bucket: "files.example.com",
                    region: Some("us-east-1"),
                    access_key_id: "AKIAEXAMPLE",
                    secret_access_key: &secret,
                },
                "https://s3.us-east-1.amazonaws.com/files.example.com/builds/app%20v2.tar.gz",
            ),
            (
                S3Target {
                    service: S3Service::GoogleCloud,
                    bucket: "elysium_files",
                    region: None,
                    access_key_id: "GOOG1EXAMPLE",
                    secret_access_key: &secret,
                },
                "https://storage.googleapis.com/elysium_files/builds/app%20v2.tar.gz",
            ),
        ];

        let key = "builds/app v2.tar.gz";
        for (target, object_url) in targets {
            let (bucket, credentials) = target.signing().expect("signing");
            let urls = [
                bucket
                    .get_object(Some(&credentials), key)
                    .sign(PRESIGNED_LIFETIME),
                bucket
                    .put_object(Some(&credentials), key)
                    .sign(PRESIGNED_LIFETIME),
                bucket
                    .head_object(Some(&credentials), key)
                    .sign(PRESIGNED_LIFETIME),
                bucket
                    .delete_object(Some(&credentials), key)
                    .sign(PRESIGNED_LIFETIME),
            ];
            for url in urls {
                let unsigned = url.as_str().split('?').next().expect("a URL");
                assert_eq!(unsigned, object_url);
                assert!(
                    url.query()
                        .is_some_and(|query| query.contains("X-Amz-Signature="))
                );
            }
        }
    }

    #[test]
    fn listings_are_scoped_to_the_directory() {
        assert_eq!(directory_prefix(""), "");
        assert_eq!(directory_prefix("uploads/2026"), "uploads/2026/");
    }

    #[test]
    fn listing_pages_become_entries_named_inside_the_location() {
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
            <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
                <Name>elysium-files</Name>
                <Prefix>artifacts/builds/</Prefix>
                <KeyCount>4</KeyCount>
                <MaxKeys>1000</MaxKeys>
                <Delimiter>/</Delimiter>
                <EncodingType>url</EncodingType>
                <IsTruncated>true</IsTruncated>
                <NextContinuationToken>1ueGcxLPRx1Tr</NextContinuationToken>
                <Contents>
                    <Key>artifacts/builds/</Key>
                    <LastModified>2026-03-17T12:00:00.000Z</LastModified>
                    <ETag>"d41d8cd98f00b204e9800998ecf8427e"</ETag>
                    <Size>0</Size>
                    <StorageClass>STANDARD</StorageClass>
                </Contents>
                <Contents>
                    <Key>artifacts/builds/app%20v2.tar.gz</Key>
                    <LastModified>2026-03-18T08:30:00.000Z</LastModified>
                    <ETag>"9b2cf535f27731c974343645a3985328"</ETag>
                    <Size>1048576</Size>
                    <StorageClass>STANDARD</StorageClass>
                </Contents>
                <CommonPrefixes>
                    <Prefix>artifacts/builds/nightly/</Prefix>
                </CommonPrefixes>
            </ListBucketResult>"#;
        let listing = ListObjectsV2::parse_response(body).expect("parses");
        let next_cursor = listing.next_continuation_token.clone();
        let directory = RelativePath::parse("builds").expect("path");
        let entries = entries_of(listing, "artifacts/builds/", &directory);

        assert_eq!(next_cursor.as_deref(), Some("1ueGcxLPRx1Tr"));
        assert_eq!(entries.len(), 2, "the directory's own marker is not listed");
        assert_eq!(entries[0].path, "builds/nightly");
        assert_eq!(entries[0].kind, EntryKind::Directory);
        assert_eq!(entries[1].path, "builds/app v2.tar.gz");
        assert_eq!(entries[1].kind, EntryKind::File);
        assert_eq!(entries[1].size_bytes, Some(1_048_576));
        assert_eq!(
            entries[1].modified_at.map(|time| time.to_rfc3339()),
            Some("2026-03-18T08:30:00+00:00".to_owned())
        );
    }

    #[test]
    fn error_documents_parse_with_extra_fields() {
        let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
            <Error><Code>NoSuchBucket</Code><Message>The specified bucket does not exist</Message>\
            <BucketName>missing</BucketName><RequestId>ABC</RequestId></Error>";
        let error = instant_xml::from_str::<ErrorBody>(body).expect("parses");
        assert_eq!(error.code, "NoSuchBucket");
        assert_eq!(error.message, "The specified bucket does not exist");
    }

    #[test]
    fn error_codes_map_to_storage_errors() {
        let document = |code: &str| {
            format!("<Error><Code>{code}</Code><Message>Service message</Message></Error>")
        };
        let forbidden = StatusCode::FORBIDDEN;

        for code in UNAUTHORIZED_CODES {
            let error = refusal("Amazon S3", forbidden, &document(code));
            assert!(matches!(error, StorageError::Unauthorized(_)), "{code}");
        }
        assert!(matches!(
            refusal("Amazon S3", StatusCode::NOT_FOUND, &document("NoSuchKey")),
            StorageError::NotFound
        ));
        let missing_bucket = refusal(
            "Amazon S3",
            StatusCode::NOT_FOUND,
            &document("NoSuchBucket"),
        );
        assert_eq!(
            missing_bucket.to_string(),
            "Amazon S3: Service message (NoSuchBucket)"
        );
        let unreadable = refusal("Google Cloud Storage", StatusCode::BAD_GATEWAY, "<html>");
        assert_eq!(
            unreadable.to_string(),
            "Google Cloud Storage: 502 Bad Gateway"
        );
    }

    #[test]
    fn head_answers_describe_files_from_their_headers() {
        let answer = axum::http::Response::builder()
            .header(CONTENT_LENGTH, "1048576")
            .header(LAST_MODIFIED, "Wed, 18 Mar 2026 08:30:00 GMT")
            .body(Vec::new())
            .expect("response");
        let response = Response::from(answer);
        let path = RelativePath::parse("builds/app.tar.gz").expect("path");
        let entry = file_entry(&path, &response);

        assert_eq!(entry.path, "builds/app.tar.gz");
        assert_eq!(entry.size_bytes, Some(1_048_576));
        assert_eq!(
            entry.modified_at.map(|time| time.to_rfc3339()),
            Some("2026-03-18T08:30:00+00:00".to_owned())
        );
    }
}
