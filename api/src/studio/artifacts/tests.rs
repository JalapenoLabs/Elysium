// Copyright © 2026 Jalapeno Labs

//! Keeping announced files, end to end: the real SDK against the fleet's fake satellite,
//! [`Storage`] against the storage module's fake Bunny zone, and Postgres.

use axum::http::HeaderMap;
use secrecy::SecretString;
use sha2::Sha256;

use super::*;
use crate::fleet::fake_satellite::{WorkspaceFile, fake_workspace};
use crate::models::coding_session::{self, NewCodingSession};
use crate::models::project::ProjectScope;
use crate::models::satellite::{self, NewSatellite};
use crate::models::storage_location::{BunnyStorageRegion, NewStorageLocation, StorageProvider};
use crate::models::studio_item::NewStudioItem;
use crate::storage::tests::{PASSWORD, fake_storage};
use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};

/// An item whose files go to the fake zone, and a Studio session for it.
async fn item_and_session(
    connection: &mut diesel_async::AsyncPgConnection,
    cipher: &Cipher,
) -> (Uuid, i64) {
    let location = storage_location::create(
        connection,
        cipher,
        &NewStorageLocation {
            created_by: TEST_PERSON_ID,
            name: "Studio files".to_owned(),
            provider: StorageProvider::Bunny {
                zone: "files".to_owned(),
                region: BunnyStorageRegion::Frankfurt,
            },
            path_prefix: String::new(),
            storage_limit_bytes: None,
            access_key: SecretString::from(PASSWORD),
            projects: ProjectScope::All,
        },
    )
    .await
    .expect("location");
    let item = studio_item::create(
        connection,
        &NewStudioItem {
            created_by: TEST_PERSON_ID,
            id: Uuid::now_v7(),
            title: "Banana".to_owned(),
            prompt: "Model a banana".to_owned(),
            project_id: None,
            storage_location_id: location.id,
        },
    )
    .await
    .expect("item");
    let satellite_id = satellite::create(
        connection,
        cipher,
        &NewSatellite {
            created_by: TEST_PERSON_ID,
            name: "orbit".to_owned(),
            description: String::new(),
            url: "http://arsox:8080".to_owned(),
            secret: SecretString::from("bearer"),
            is_active: true,
        },
    )
    .await
    .expect("satellite")
    .id;
    let session_id = coding_session::reserve_id(connection).await.expect("reserve");
    let session = coding_session::create(
        connection,
        &NewCodingSession {
            created_by: TEST_PERSON_ID,
            id: session_id,
            project_id: None,
            satellite_id,
            thread_id: "thread-1".to_owned(),
            title: "Banana".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: Some(item.id),
        },
    )
    .await
    .expect("session");
    (item.id, session.id)
}

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
    let (studio_item_id, session_id) = item_and_session(&mut connection, &cipher).await;
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
    assert_eq!(second.storage_path, first.storage_path, "both name one stored object");
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
