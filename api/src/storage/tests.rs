// Copyright © 2026 Jalapeno Labs

//! [`Storage`]'s file operations end to end, against a local fake of Bunny Storage's HTTP
//! API, and against the real S3 services with credentials that cannot work.
//!
//! The fake answers the way Bunny does: `AccessKey` sign-in, JSON listings for a path with
//! a trailing slash, `DESCRIBE` for one entry, `201` for an upload, and a `DELETE` that
//! removes a directory with everything inside it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream};
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::errors::ApiError;
use crate::models::storage_location::{BunnyStorageRegion, S3Service, StorageLocationKind};

const ZONE: &str = "files";
const PASSWORD: &str = "zone-password";

/// Every header an upload arrived with, and the bytes it carried.
#[derive(Debug, Clone)]
struct StoredFile {
    headers: HeaderMap,
    bytes: Vec<u8>,
}

/// The fake zone's files by key, such as `artifacts/builds/app.tar.gz`.
type Files = Arc<Mutex<BTreeMap<String, StoredFile>>>;

fn bunny_json(status: StatusCode, message: &str) -> Response {
    let body = json!({ "HttpCode": status.as_u16(), "Message": message });
    (status, axum::Json(body)).into_response()
}

fn described(name: &str, length: usize, is_directory: bool) -> serde_json::Value {
    json!({
        "Guid": Uuid::now_v7(),
        "StorageZoneName": ZONE,
        "Path": format!("/{ZONE}/"),
        "ObjectName": name,
        "Length": length,
        "LastChanged": "2026-03-17T12:43:15.843",
        "IsDirectory": is_directory,
        "DateCreated": "2026-03-17T12:43:15.843",
        "Checksum": null,
        "ReplicatedZones": null,
    })
}

/// Reads an upload's body chunk by chunk, as it streams in.
async fn receive(body: Body) -> Option<Vec<u8>> {
    let mut chunks = body.into_data_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = chunks.next().await {
        bytes.extend_from_slice(&chunk.ok()?);
    }
    Some(bytes)
}

async fn fake_bunny(
    State(files): State<Files>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    if headers
        .get("AccessKey")
        .and_then(|value| value.to_str().ok())
        != Some(PASSWORD)
    {
        return bunny_json(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let Some(path) = uri.path().strip_prefix(&format!("/{ZONE}/")) else {
        return bunny_json(StatusCode::UNAUTHORIZED, "Unauthorized");
    };
    let is_directory_request = path.is_empty() || path.ends_with('/');
    let key = path.trim_end_matches('/').to_owned();
    let children_prefix = if key.is_empty() {
        String::new()
    } else {
        format!("{key}/")
    };

    if method == Method::PUT {
        let Some(bytes) = receive(body).await else {
            // A body that fails part way leaves nothing behind, as with Bunny.
            return bunny_json(StatusCode::BAD_REQUEST, "Upload failed");
        };
        files
            .lock()
            .expect("lock")
            .insert(key, StoredFile { headers, bytes });
        return bunny_json(StatusCode::CREATED, "File uploaded.");
    }

    let mut files = files.lock().expect("lock");
    let has_children = files
        .keys()
        .any(|stored| stored.starts_with(&children_prefix));

    if method == Method::DELETE {
        let existed = files.remove(&key).is_some();
        if !existed && !has_children {
            return bunny_json(StatusCode::NOT_FOUND, "Object Not Found");
        }
        files.retain(|stored, _file| !stored.starts_with(&children_prefix));
        return bunny_json(StatusCode::OK, "File deleted successfuly.");
    }

    if method.as_str() == "DESCRIBE" {
        let name = key.rsplit('/').next().unwrap_or_default();
        if let Some(file) = files.get(&key) {
            return axum::Json(described(name, file.bytes.len(), false)).into_response();
        }
        if has_children {
            return axum::Json(described(name, 0, true)).into_response();
        }
        return bunny_json(StatusCode::NOT_FOUND, "Object Not Found");
    }

    if is_directory_request {
        let mut listing = BTreeMap::new();
        for (stored, file) in files.iter() {
            let Some(rest) = stored.strip_prefix(&children_prefix) else {
                continue;
            };
            match rest.split_once('/') {
                Some((directory, _inside)) => {
                    listing.insert(directory.to_owned(), described(directory, 0, true));
                }
                None => {
                    listing.insert(rest.to_owned(), described(rest, file.bytes.len(), false));
                }
            }
        }
        let entries: Vec<_> = listing.into_values().collect();
        return axum::Json(entries).into_response();
    }

    match files.get(&key) {
        Some(file) => (
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            file.bytes.clone(),
        )
            .into_response(),
        None => bunny_json(StatusCode::NOT_FOUND, "Object Not Found"),
    }
}

/// Starts the fake on a free local port, and a [`Storage`] whose Bunny client uses it.
async fn fake_storage() -> (Storage, Files) {
    let files = Files::default();
    let router = Router::new()
        .fallback(fake_bunny)
        .with_state(Arc::clone(&files));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, router).await });

    let http = reqwest::Client::new();
    let storage = Storage {
        bunny: Bunny::with_test_endpoint(http.clone(), format!("http://{address}")),
        s3: S3::new(http),
    };
    (storage, files)
}

fn location(
    kind: StorageLocationKind,
    path_prefix: &str,
    s3_service: Option<S3Service>,
) -> StorageLocation {
    let is_s3 = kind == StorageLocationKind::S3;
    let s3_bucket = is_s3.then(|| "elysium-probe-bucket-that-does-not-exist".to_owned());
    let s3_region = (s3_service == Some(S3Service::Aws)).then(|| "us-east-1".to_owned());
    StorageLocation {
        id: Uuid::now_v7(),
        name: "artifacts".to_owned(),
        kind,
        bunny_zone: (!is_s3).then(|| ZONE.to_owned()),
        bunny_region: (!is_s3).then_some(BunnyStorageRegion::Frankfurt),
        path_prefix: path_prefix.to_owned(),
        storage_limit_bytes: None,
        access_key_encrypted: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        all_projects: true,
        s3_service,
        s3_bucket,
        s3_region,
        s3_access_key_id: is_s3.then(|| "AKIAIOSFODNN7EXAMPLE".to_owned()),
    }
}

fn bunny_location() -> StorageLocation {
    location(StorageLocationKind::Bunny, "artifacts", None)
}

fn chunks_of(parts: &[&'static [u8]]) -> impl Stream<Item = Result<Bytes, std::io::Error>> + use<> {
    let chunks: Vec<Result<Bytes, std::io::Error>> = parts
        .iter()
        .map(|part| Ok(Bytes::from_static(part)))
        .collect();
    stream::iter(chunks)
}

async fn read_all(download: Download) -> Vec<u8> {
    let mut body = download.body;
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk.expect("chunk"));
    }
    bytes
}

/// Compiles only for a `Send` value.
const fn assert_send<Value: Send>(_value: &Value) {}

/// Axum handlers must return `Send` futures, so every operation's future must be one.
#[test]
fn every_operation_can_run_in_a_request_handler() {
    let storage = Storage::new(reqwest::Client::new());
    let location = bunny_location();
    let password = SecretString::from(PASSWORD);

    // The futures are only checked, never polled, so no request is made.
    assert_send(&storage.list(&location, &password, "", None));
    assert_send(&storage.stat(&location, &password, "a.txt"));
    assert_send(&storage.download(&location, &password, "a.txt"));
    assert_send(&storage.upload(&location, &password, "a.txt", chunks_of(&[]), 0, None));
    assert_send(&storage.delete(&location, &password, "a.txt"));
}

#[tokio::test]
async fn uploads_stream_with_a_declared_length_and_download_back() {
    let (storage, files) = fake_storage().await;
    let location = bunny_location();
    let password = SecretString::from(PASSWORD);

    let entry = storage
        .upload(
            &location,
            &password,
            "builds/app.tar.gz",
            chunks_of(&[b"first ", b"second ", b"third"]),
            18,
            Some("application/gzip"),
        )
        .await
        .expect("upload");
    assert_eq!(entry.path, "builds/app.tar.gz");
    assert_eq!(entry.kind, EntryKind::File);
    assert_eq!(entry.size_bytes, Some(18));

    let stored = files
        .lock()
        .expect("lock")
        .get("artifacts/builds/app.tar.gz")
        .cloned()
        .expect("stored under the location's directory");
    assert_eq!(stored.bytes, b"first second third");
    assert_eq!(stored.headers.get("content-length").expect("length"), "18");
    assert!(
        stored.headers.get("transfer-encoding").is_none(),
        "a declared length is sent unchunked"
    );
    assert_eq!(
        stored.headers.get("content-type").expect("type"),
        "application/octet-stream"
    );
    assert_eq!(
        stored
            .headers
            .get("override-content-type")
            .expect("override"),
        "application/gzip"
    );

    let download = storage
        .download(&location, &password, "builds/app.tar.gz")
        .await
        .expect("download");
    assert_eq!(download.content_length, Some(18));
    assert_eq!(read_all(download).await, b"first second third");
}

#[tokio::test]
async fn listings_and_descriptions_name_entries_inside_the_location() {
    let (storage, _files) = fake_storage().await;
    let location = bunny_location();
    let password = SecretString::from(PASSWORD);
    for path in ["notes.md", "builds/app.tar.gz", "builds/nightly/app.tar.gz"] {
        storage
            .upload(&location, &password, path, chunks_of(&[b"12345"]), 5, None)
            .await
            .expect(path);
    }

    let root = storage
        .list(&location, &password, "", None)
        .await
        .expect("list");
    let root_paths: Vec<_> = root
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.kind))
        .collect();
    assert_eq!(
        root_paths,
        [
            ("builds", EntryKind::Directory),
            ("notes.md", EntryKind::File)
        ]
    );
    assert_eq!(root.next_cursor, None);

    let builds = storage
        .list(&location, &password, "builds", None)
        .await
        .expect("list");
    assert_eq!(builds.entries[0].path, "builds/app.tar.gz");
    assert_eq!(builds.entries[0].size_bytes, Some(5));
    assert_eq!(builds.entries[1].path, "builds/nightly");

    let empty = storage
        .list(&location, &password, "not-written-yet", None)
        .await
        .expect("a directory not written to yet");
    assert!(empty.entries.is_empty());

    let file = storage
        .stat(&location, &password, "builds/app.tar.gz")
        .await
        .expect("stat")
        .expect("a file");
    assert_eq!(file.kind, EntryKind::File);
    assert_eq!(file.size_bytes, Some(5));
    let directory = storage
        .stat(&location, &password, "builds/nightly")
        .await
        .expect("stat")
        .expect("a directory");
    assert_eq!(directory.kind, EntryKind::Directory);
    let missing = storage
        .stat(&location, &password, "builds/missing.tar.gz")
        .await
        .expect("stat");
    assert_eq!(missing, None);

    let check = storage.check(&location, &password).await.expect("check");
    assert_eq!(check.entries, 2);
}

#[tokio::test]
async fn deletes_remove_files_only_and_tolerate_missing_ones() {
    let (storage, files) = fake_storage().await;
    let location = bunny_location();
    let password = SecretString::from(PASSWORD);
    for path in ["builds/app.tar.gz", "builds/nightly/app.tar.gz"] {
        storage
            .upload(&location, &password, path, chunks_of(&[b"12345"]), 5, None)
            .await
            .expect(path);
    }

    storage
        .delete(&location, &password, "builds/app.tar.gz")
        .await
        .expect("delete");
    storage
        .delete(&location, &password, "builds/app.tar.gz")
        .await
        .expect("deleting it again succeeds");

    let refused = storage
        .delete(&location, &password, "builds/nightly")
        .await
        .expect_err("a directory is not deleted");
    assert!(matches!(refused, StorageError::Invalid(_)));
    assert_eq!(
        files.lock().expect("lock").keys().collect::<Vec<_>>(),
        ["artifacts/builds/nightly/app.tar.gz"],
        "the directory's contents survive"
    );

    let missing = storage
        .download(&location, &password, "builds/app.tar.gz")
        .await
        .expect_err("the file is gone");
    assert!(matches!(missing, StorageError::NotFound));
    assert!(matches!(ApiError::from(missing), ApiError::NotFound));
}

#[tokio::test]
async fn uploads_that_break_their_declared_length_are_refused() {
    let (storage, files) = fake_storage().await;
    let location = bunny_location();
    let password = SecretString::from(PASSWORD);

    let short = storage
        .upload(
            &location,
            &password,
            "short.bin",
            chunks_of(&[b"1234"]),
            10,
            None,
        )
        .await
        .expect_err("fewer bytes than declared");
    assert!(
        matches!(&short, StorageError::Invalid(message) if message.contains("4 of its 10")),
        "{short}"
    );

    let long = storage
        .upload(
            &location,
            &password,
            "long.bin",
            chunks_of(&[b"1234", b"5678"]),
            6,
            None,
        )
        .await
        .expect_err("more bytes than declared");
    assert!(matches!(long, StorageError::Invalid(_)), "{long}");
    assert!(matches!(ApiError::from(long), ApiError::BadRequest(_)));

    assert!(
        files.lock().expect("lock").is_empty(),
        "nothing partial is stored"
    );
}

#[tokio::test]
async fn requests_that_cannot_be_served_are_refused_before_any_call() {
    // No fake is running at this endpoint, so reaching the network would fail differently.
    let http = reqwest::Client::new();
    let storage = Storage {
        bunny: Bunny::with_test_endpoint(http.clone(), "http://127.0.0.1:9".to_owned()),
        s3: S3::new(http),
    };
    let location = bunny_location();
    let password = SecretString::from(PASSWORD);

    let oversized = storage
        .upload(
            &location,
            &password,
            "huge.bin",
            chunks_of(&[]),
            MAX_UPLOAD_BYTES + 1,
            None,
        )
        .await
        .expect_err("over the single-request ceiling");
    assert!(matches!(oversized, StorageError::Invalid(_)));

    let cursor = storage
        .list(&location, &password, "", Some("next"))
        .await
        .expect_err("Bunny hands out no cursors");
    assert!(matches!(cursor, StorageError::Invalid(_)));

    for path in ["", "../escape", "/absolute", "trailing/"] {
        let error = storage
            .download(&location, &password, path)
            .await
            .expect_err(path);
        assert!(matches!(error, StorageError::Invalid(_)), "{path}");
    }
}

#[tokio::test]
async fn a_wrong_password_is_unauthorized_for_every_operation() {
    let (storage, _files) = fake_storage().await;
    let location = bunny_location();
    let wrong = SecretString::from("not-the-password");

    let listed = storage.list(&location, &wrong, "", None).await;
    let described = storage.stat(&location, &wrong, "a.txt").await;
    let downloaded = storage.download(&location, &wrong, "a.txt").await;
    let uploaded = storage
        .upload(&location, &wrong, "a.txt", chunks_of(&[b"1"]), 1, None)
        .await;
    let deleted = storage.delete(&location, &wrong, "a.txt").await;

    assert!(matches!(listed, Err(StorageError::Unauthorized(_))));
    assert!(matches!(described, Err(StorageError::Unauthorized(_))));
    assert!(matches!(downloaded, Err(StorageError::Unauthorized(_))));
    assert!(matches!(uploaded, Err(StorageError::Unauthorized(_))));
    assert!(matches!(deleted, Err(StorageError::Unauthorized(_))));
}

/// Sends every operation to Amazon S3 and Google Cloud Storage with a key that does not
/// exist, proving each is signed and sent the way the service expects and that the refusal
/// maps to [`StorageError::Unauthorized`]. Nothing can be written: the key opens nothing.
#[tokio::test]
#[ignore = "reaches Amazon S3 and Google Cloud Storage over the network"]
async fn s3_services_refuse_a_fake_key_for_every_operation() {
    let storage = Storage::new(reqwest::Client::new());
    let secret = SecretString::from("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");

    for service in [S3Service::Aws, S3Service::GoogleCloud] {
        let location = location(StorageLocationKind::S3, "elysium-probe", Some(service));
        let listed = storage.list(&location, &secret, "", None).await;
        let described = storage.stat(&location, &secret, "probe.txt").await;
        let downloaded = storage.download(&location, &secret, "probe.txt").await;
        let uploaded = storage
            .upload(
                &location,
                &secret,
                "probe.txt",
                chunks_of(&[b"probe"]),
                5,
                Some("text/plain"),
            )
            .await;
        let deleted = storage.delete(&location, &secret, "probe.txt").await;
        // `Storage::delete` stops at its `HEAD` when the key is refused, so the `DELETE`
        // request itself is sent through the provider client.
        let provider = location.provider();
        let Target::S3(bucket) = Target::new(&provider, &secret) else {
            panic!("an S3 location");
        };
        let deleted_directly = storage.s3.delete(&bucket, "elysium-probe/probe.txt").await;

        let outcomes = [
            ("list", listed.map(|_page| ())),
            ("stat", described.map(|_entry| ())),
            ("download", downloaded.map(|_download| ())),
            ("upload", uploaded.map(|_entry| ())),
            ("delete", deleted),
            ("DELETE request", deleted_directly),
        ];
        for (operation, outcome) in outcomes {
            eprintln!("{service:?} {operation}: {outcome:?}");
            assert!(
                matches!(outcome, Err(StorageError::Unauthorized(_))),
                "{service:?} {operation}: {outcome:?}"
            );
        }
    }
}
