// Copyright © 2026 Jalapeno Labs

//! Keeping announced files, end to end: the real SDK against the fleet's fake satellite,
//! [`Storage`] against the storage module's fake Bunny zone, and Postgres.

use axum::http::HeaderMap;

use super::*;
use crate::fleet::fake_satellite::{WorkspaceFile, fake_workspace};
use crate::storage::tests::fake_storage;
use crate::test_support::{cipher, migrated_database, studio_item_with_session};

/// The satellite the fixture's session names. Never reached: these tests hand the code the
/// fake's thread directly.
const UNREACHED_SATELLITE: &str = "http://arsox:8080";

fn announced(path: &str, bytes: &[u8]) -> Artifact {
    Artifact {
        path: path.to_owned(),
        size_bytes: bytes.len() as u64,
        content_type: Some("image/png".to_owned()),
        sha256: hex::encode(Sha256::digest(bytes)),
        ..Artifact::default()
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn bytes_already_kept_under_another_path_are_never_uploaded_over_or_deleted() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 2)
        .await
        .expect("pool");
    let cipher = cipher();
    let events = EventBus::new();
    let (storage, stored) = fake_storage().await;
    let (workspace, files) = fake_workspace().await;
    let studio = studio_item_with_session(&mut connection, &cipher, UNREACHED_SATELLITE).await;
    let (studio_item_id, session_id) = (studio.item.id, studio.session.id);
    let services = Services {
        database: &database,
        cipher: &cipher,
        storage: &storage,
        events: &events,
    };
    let source = Source {
        session_id,
        studio_item_id,
        workspace: &workspace,
    };

    // The agent renders hero.png, and Elysium keeps it.
    let render = b"the first render".to_vec();
    files.lock().expect("lock").insert(
        "artifacts/hero.png".to_owned(),
        WorkspaceFile {
            headers: HeaderMap::new(),
            bytes: render.clone(),
        },
    );
    let first = keep(services, source, &announced("hero.png", &render))
        .await
        .expect("the first file is kept");
    let Kept::New(first) = first else {
        panic!("the first file is new: {first:?}");
    };

    // It copies the render to front.png and announces the same bytes, then overwrites
    // front.png before Elysium reads it.
    files.lock().expect("lock").insert(
        "artifacts/front.png".to_owned(),
        WorkspaceFile {
            headers: HeaderMap::new(),
            bytes: b"something else entirely".to_vec(),
        },
    );
    let second = keep(services, source, &announced("front.png", &render))
        .await
        .expect("the copy is recorded");

    let Kept::New(second) = second else {
        panic!("the copy is recorded for its own path: {second:?}");
    };
    assert_eq!(second.artifact_path, "front.png");
    assert_eq!(
        second.storage_path, first.storage_path,
        "both name one stored object"
    );
    let kept = stored
        .lock()
        .expect("lock")
        .get(&first.storage_path)
        .map(|file| file.bytes.clone());
    assert_eq!(
        kept,
        Some(render),
        "the kept object still holds the first render's bytes"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn reconciling_keeps_what_was_never_announced_and_nothing_twice() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 2)
        .await
        .expect("pool");
    let cipher = cipher();
    let events = EventBus::new();
    let (storage, _stored) = fake_storage().await;
    let (workspace, files) = fake_workspace().await;
    let studio = studio_item_with_session(&mut connection, &cipher, UNREACHED_SATELLITE).await;
    let (studio_item_id, session_id) = (studio.item.id, studio.session.id);
    let services = Services {
        database: &database,
        cipher: &cipher,
        storage: &storage,
        events: &events,
    };
    let source = Source {
        session_id,
        studio_item_id,
        workspace: &workspace,
    };

    // hero.png was announced and kept; banana.glb's announcement was missed.
    let render = b"the hero render".to_vec();
    for (path, bytes) in [
        ("artifacts/hero.png", render.clone()),
        ("artifacts/banana.glb", b"glTF binary".to_vec()),
        ("scratch/notes.txt", b"not a deliverable".to_vec()),
    ] {
        files.lock().expect("lock").insert(
            path.to_owned(),
            WorkspaceFile {
                headers: HeaderMap::new(),
                bytes,
            },
        );
    }
    keep(services, source, &announced("hero.png", &render))
        .await
        .expect("hero.png is kept");

    reconcile(services, source).await;
    reconcile(services, source).await;

    let assets = studio_asset::list_for_items(&mut connection, &[studio_item_id])
        .await
        .expect("assets");
    let mut paths: Vec<&str> = assets
        .iter()
        .map(|asset| asset.artifact_path.as_str())
        .collect();
    paths.sort_unstable();
    assert_eq!(
        paths,
        ["banana.glb", "hero.png"],
        "each file once, and only artifacts"
    );
}
