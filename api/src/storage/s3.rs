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

use std::time::Duration;

use instant_xml::FromXml;
use rusty_s3::actions::ListObjectsV2;
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::{Listing, StorageError};
use crate::models::storage_location::S3Service;

/// How long a presigned request stays valid. It is sent at once, so a minute only covers
/// clock drift between Elysium and the service.
const PRESIGNED_LIFETIME: Duration = Duration::from_secs(60);

/// How long one call may take, matching Bunny's.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The most keys one listing returns. S3's own ceiling; a directory with more reports
/// that there are more rather than paging through them.
const LISTING_MAX_KEYS: usize = 1000;

/// Error codes that mean the key id or secret is wrong, rather than the key lacking
/// permission. `InvalidSecurity` is Google Cloud's code for a malformed signature.
const UNAUTHORIZED_CODES: [&str; 3] = [
    "InvalidAccessKeyId",
    "SignatureDoesNotMatch",
    "InvalidSecurity",
];

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

/// The S3 prefix for a location's directory: its path with a trailing slash, or empty for
/// the bucket's root.
fn directory_prefix(directory: &str) -> String {
    if directory.is_empty() {
        return String::new();
    }
    format!("{directory}/")
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

impl S3 {
    pub const fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Lists the objects and subdirectories directly inside `directory` of the bucket.
    ///
    /// # Errors
    /// Returns [`StorageError::Unauthorized`] when the service rejects the key id or
    /// secret, and [`StorageError::Refused`] when it cannot be reached or refuses the
    /// listing, such as for a missing bucket or a key without permission to list it.
    pub async fn list_directory(
        &self,
        target: &S3Target<'_>,
        directory: &str,
    ) -> Result<Listing, StorageError> {
        let name = service_name(target.service);
        let bucket = bucket(target.service, target.bucket, target.region)?;
        let credentials = Credentials::new(
            target.access_key_id,
            target.secret_access_key.expose_secret(),
        );

        let prefix = directory_prefix(directory);
        let mut action = bucket.list_objects_v2(Some(&credentials));
        action.with_prefix(prefix.as_str());
        action.with_delimiter("/");
        action.with_max_keys(LISTING_MAX_KEYS);
        let url = action.sign(PRESIGNED_LIFETIME);

        // The presigned URL carries the signature, so errors drop it before they are shown.
        let response = self
            .http
            .get(url)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| StorageError::Refused(format!("{name}: {}", error.without_url())))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| StorageError::Refused(format!("{name}: {}", error.without_url())))?;

        if !status.is_success() {
            let Ok(error) = instant_xml::from_str::<ErrorBody>(&body) else {
                return Err(StorageError::Refused(format!("{name}: {status}")));
            };
            if UNAUTHORIZED_CODES.contains(&error.code.as_str()) {
                return Err(StorageError::Unauthorized(
                    "the bucket's service refused the access key; check the access key id and secret",
                ));
            }
            return Err(StorageError::Refused(format!(
                "{name}: {} ({})",
                error.message, error.code
            )));
        }

        let listing = ListObjectsV2::parse_response(&body).map_err(|error| {
            StorageError::Refused(format!("{name} sent an unreadable listing: {error}"))
        })?;
        Ok(Listing {
            entries: listing.contents.len() + listing.common_prefixes.len(),
            has_more: listing.next_continuation_token.is_some(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn listings_are_scoped_to_the_directory() {
        assert_eq!(directory_prefix(""), "");
        assert_eq!(directory_prefix("uploads/2026"), "uploads/2026/");
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
}
