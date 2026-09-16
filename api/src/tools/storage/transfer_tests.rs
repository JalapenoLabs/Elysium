// Copyright © 2026 Jalapeno Labs

//! Downloads and uploads end to end: the real SDK against a local fake of a satellite's
//! workspace file routes, and [`Storage`] against the storage module's fake Bunny zone.
//!
//! The fake satellite answers the way a satellite does: protobuf for the version and the
//! thread, the file's bytes with a `Content-Length` for a read, a `WorkspaceFileWritten` for
//! a write, and a contract error for a file that is not there.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use arsox_sdk::client::Satellite as SatelliteClient;
use arsox_sdk::proto::artifact::v1::WorkspaceFileWritten;
use arsox_sdk::proto::error::v1::{Error as ContractError, ErrorCode};
use arsox_sdk::proto::satellite::v1::GetVersionResponse;
use arsox_sdk::proto::thread::v1::{GetThreadResponse, Thread};
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use futures_util::StreamExt as _;

use super::*;
use crate::storage::tests::{PASSWORD, bunny_location, fake_storage};

const THREAD_ID: &str = "thread-1";

/// A workspace file as the fake holds it, with the headers it was written with.
#[derive(Debug, Clone)]
struct WorkspaceFile {
    headers: HeaderMap,
    bytes: Vec<u8>,
}

/// The fake workspace's files by path, such as `reports/today.txt`.
type Workspace = Arc<Mutex<BTreeMap<String, WorkspaceFile>>>;

fn protobuf(status: StatusCode, message: &impl prost::Message) -> Response {
    let headers = [(header::CONTENT_TYPE, "application/protobuf")];
    (status, headers, message.encode_to_vec()).into_response()
}

async fn version() -> Response {
    let answer = GetVersionResponse {
        satellite_version: "test".to_owned(),
        proto_major: 1,
        proto_minor: 0,
    };
    protobuf(StatusCode::OK, &answer)
}

async fn thread(Path(thread_id): Path<String>) -> Response {
    let answer = GetThreadResponse {
        thread: Some(Thread {
            thread_id,
            ..Thread::default()
        }),
    };
    protobuf(StatusCode::OK, &answer)
}

async fn read_file(
    State(workspace): State<Workspace>,
    Path((_thread_id, path)): Path<(String, String)>,
) -> Response {
    let Some(file) = workspace.lock().expect("lock").get(&path).cloned() else {
        let missing = ContractError {
            code: ErrorCode::WorkspaceFileNotFound.into(),
            message: format!("nothing is at {path}"),
            ..ContractError::default()
        };
        return protobuf(StatusCode::NOT_FOUND, &missing);
    };
    let headers = [(header::CONTENT_TYPE, "text/csv")];
    (headers, file.bytes).into_response()
}

async fn write_file(
    State(workspace): State<Workspace>,
    Path((_thread_id, path)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let mut chunks = body.into_data_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = chunks.next().await {
        let Ok(chunk) = chunk else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        bytes.extend_from_slice(&chunk);
    }
    let written = WorkspaceFileWritten {
        path: path.clone(),
        size_bytes: bytes.len() as u64,
        ..WorkspaceFileWritten::default()
    };
    workspace
        .lock()
        .expect("lock")
        .insert(path, WorkspaceFile { headers, bytes });
    protobuf(StatusCode::OK, &written)
}

/// Starts the fake satellite on a free local port and attaches to its one thread.
async fn fake_workspace() -> (ThreadHandle, Workspace) {
    let workspace = Workspace::default();
    let router = Router::new()
        .route("/v1/version", get(version))
        .route("/v1/threads/{id}", get(thread))
        .route(
            "/v1/threads/{id}/files/{*path}",
            get(read_file).put(write_file),
        )
        .with_state(Arc::clone(&workspace));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, router).await });

    let client = SatelliteClient::connect(format!("http://{address}"), "satellite-secret")
        .await
        .expect("connect");
    let handle = client.threads().attach(THREAD_ID).await.expect("attach");
    (handle, workspace)
}

fn upload_arguments(workspace_path: &str, content_type: Option<&str>) -> UploadArguments {
    UploadArguments {
        workspace_path: workspace_path.to_owned(),
        location_id: Uuid::nil(),
        path: "reports/today.csv".to_owned(),
        content_type: content_type.map(ToOwned::to_owned),
    }
}

#[tokio::test]
async fn uploads_stream_the_workspace_file_into_the_location() {
    let (storage, stored) = fake_storage().await;
    let (handle, workspace) = fake_workspace().await;
    workspace.lock().expect("lock").insert(
        "out/today.csv".to_owned(),
        WorkspaceFile {
            headers: HeaderMap::new(),
            bytes: b"day,total\nmonday,3\n".to_vec(),
        },
    );

    let answer = copy_to_location(
        &storage,
        &handle,
        &bunny_location(),
        &SecretString::from(PASSWORD),
        upload_arguments("out/today.csv", None),
    )
    .await
    .expect("upload");
    assert_eq!(answer["entry"]["path"], "reports/today.csv");
    assert_eq!(answer["entry"]["sizeBytes"], 19);

    let file = stored
        .lock()
        .expect("lock")
        .get("artifacts/reports/today.csv")
        .cloned()
        .expect("stored under the location's directory");
    assert_eq!(file.bytes, b"day,total\nmonday,3\n");
    assert_eq!(file.headers.get("content-length").expect("length"), "19");
    assert_eq!(
        file.headers
            .get("override-content-type")
            .expect("the workspace's type"),
        "text/csv"
    );
}

#[tokio::test]
async fn downloads_stream_the_stored_file_into_the_workspace() {
    let (storage, _stored) = fake_storage().await;
    let (handle, workspace) = fake_workspace().await;
    let location = bunny_location();
    let password = SecretString::from(PASSWORD);
    let body =
        futures_util::stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"stored bytes"))]);
    storage
        .upload(&location, &password, "reports/today.csv", body, 12, None)
        .await
        .expect("seed the location");

    let arguments = DownloadArguments {
        location_id: Uuid::nil(),
        path: "reports/today.csv".to_owned(),
        workspace_path: "in/today.csv".to_owned(),
    };
    let answer = copy_to_workspace(&storage, &handle, &location, &password, &arguments)
        .await
        .expect("download");
    assert_eq!(
        answer,
        json!({ "workspacePath": "in/today.csv", "sizeBytes": 12 })
    );

    let file = workspace
        .lock()
        .expect("lock")
        .get("in/today.csv")
        .cloned()
        .expect("written to the workspace");
    assert_eq!(file.bytes, b"stored bytes");
    assert_eq!(file.headers.get("content-length").expect("length"), "12");
}

#[tokio::test]
async fn transfer_failures_name_the_side_that_refused() {
    let (storage, stored) = fake_storage().await;
    let (handle, _workspace) = fake_workspace().await;
    let location = bunny_location();
    let password = SecretString::from(PASSWORD);

    let missing_workspace_file = copy_to_location(
        &storage,
        &handle,
        &location,
        &password,
        upload_arguments("out/missing.csv", Some("text/csv")),
    )
    .await
    .expect_err("nothing to upload");
    assert!(
        matches!(&missing_workspace_file, ToolError::Workspace(message)
            if message.contains("nothing is at out/missing.csv")),
        "{missing_workspace_file}"
    );
    assert!(stored.lock().expect("lock").is_empty());

    let arguments = DownloadArguments {
        location_id: Uuid::nil(),
        path: "reports/missing.csv".to_owned(),
        workspace_path: "in/missing.csv".to_owned(),
    };
    let missing_stored_file =
        copy_to_workspace(&storage, &handle, &location, &password, &arguments)
            .await
            .expect_err("nothing to download");
    assert!(matches!(missing_stored_file, ToolError::NotFound(_)));
}
