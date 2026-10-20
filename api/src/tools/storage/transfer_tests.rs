// Copyright © 2026 Jalapeno Labs

//! Downloads and uploads end to end: the real SDK against the fleet's fake satellite, and
//! [`Storage`] against the storage module's fake Bunny zone.

use axum::http::HeaderMap;
use bytes::Bytes;

use super::*;
use crate::fleet::fake_satellite::{WorkspaceFile, fake_workspace};
use crate::storage::tests::{PASSWORD, bunny_location, fake_storage};

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
