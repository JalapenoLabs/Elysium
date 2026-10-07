// Copyright © 2026 Jalapeno Labs

//! A local fake of a satellite, for tests that drive the real SDK: the version, one thread,
//! its workspace files, and its artifact listing, which like a satellite's is every file under
//! `artifacts/` with its size and SHA-256.
//!
//! It answers the way a satellite does: protobuf for the version, the thread, and the
//! listing, the file's bytes with a `Content-Length` for a read, a `WorkspaceFileWritten` for
//! a write, and a contract error for a file that is not there.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use arsox_sdk::client::{Satellite as SatelliteClient, ThreadHandle};
use arsox_sdk::proto::artifact::v1::{Artifact, ListArtifactsResponse, WorkspaceFileWritten};
use arsox_sdk::proto::error::v1::{Error as ContractError, ErrorCode};
use arsox_sdk::proto::satellite::v1::GetVersionResponse;
use arsox_sdk::proto::thread::v1::{GetThreadResponse, Thread};
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::StreamExt as _;
use sha2::{Digest as _, Sha256};

pub const THREAD_ID: &str = "thread-1";

/// A workspace file as the fake holds it, with the headers it was written with.
#[derive(Debug, Clone)]
pub struct WorkspaceFile {
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

/// The fake workspace's files by path, such as `reports/today.txt`.
pub type Workspace = Arc<Mutex<BTreeMap<String, WorkspaceFile>>>;

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

async fn list_artifacts(State(workspace): State<Workspace>) -> Response {
    let artifacts = workspace
        .lock()
        .expect("lock")
        .iter()
        .filter_map(|(path, file)| {
            let relative = path.strip_prefix("artifacts/")?;
            Some(Artifact {
                name: relative.rsplit('/').next().unwrap_or(relative).to_owned(),
                path: relative.to_owned(),
                size_bytes: file.bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&file.bytes)),
                ..Artifact::default()
            })
        })
        .collect();
    let answer = ListArtifactsResponse {
        artifacts,
        page: None,
    };
    protobuf(StatusCode::OK, &answer)
}

/// Starts the fake satellite on a free local port and attaches to its one thread.
pub async fn fake_workspace() -> (ThreadHandle, Workspace) {
    let workspace = Workspace::default();
    let router = Router::new()
        .route("/v1/version", get(version))
        .route("/v1/threads/{id}", get(thread))
        .route("/v1/threads/{id}/artifacts", get(list_artifacts))
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
