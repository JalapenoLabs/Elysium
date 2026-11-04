// Copyright © 2026 Jalapeno Labs

//! Backups end to end against a migrated database and the storage module's fake Bunny zone, with
//! fake dumps. The real `pg_dump` in the Postgres container is exercised against a scratch stack;
//! see `docs/infrastructure.md`, Backups.

use chrono::TimeDelta;
use diesel::ExpressionMethods;
use diesel::QueryDsl;

use super::*;
use crate::database::schema::action_items;
use crate::models::project::ProjectScope;
use crate::models::storage_location::{BunnyStorageRegion, NewStorageLocation, StorageProvider};
use crate::storage::tests::{Files, PASSWORD, StoredFile, fake_storage};
use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};

/// Backups against a fresh database, whose dumps fail for `failing`, and the fake zone's files.
struct Fixture {
    backups: Backups,
    files: Files,
    connection: AsyncPgConnection,
    location: StorageLocation,
    activity: Activities,
}

async fn fixture(failing: Option<&str>) -> Fixture {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("Postgres accepts connections");
    let cipher = Arc::new(cipher());
    let (storage, files) = fake_storage().await;
    let location = storage_location::create(
        &mut connection,
        &cipher,
        &NewStorageLocation {
            created_by: TEST_PERSON_ID,
            name: "Offsite".to_owned(),
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
    .expect("a location");
    let activity = Activities::new();
    let backups = Backups::new(BackupsDeps {
        database,
        cipher,
        storage,
        events: EventBus::in_process(),
        activity: activity.clone(),
        dumper: Dumper::Fake {
            failing: failing.map(str::to_owned),
        },
        staging_directory: std::env::temp_dir().join(format!("elysium-backups-{}", Uuid::now_v7())),
        shutdown: CancellationToken::new(),
    });
    Fixture {
        backups,
        files,
        connection,
        location,
        activity,
    }
}

async fn choose(connection: &mut AsyncPgConnection, location: Option<Uuid>) {
    let changes = user::WorkspaceSettingsChanges {
        backup_storage_location_id: Some(location),
        ..user::WorkspaceSettingsChanges::default()
    };
    user::update_workspace_settings(connection, changes, TEST_PERSON_ID)
        .await
        .expect("the destination is chosen");
}

fn stored_paths(files: &Files) -> Vec<String> {
    files.lock().expect("lock").keys().cloned().collect()
}

fn store(files: &Files, path: &str) {
    files.lock().expect("lock").insert(
        path.to_owned(),
        StoredFile {
            headers: axum::http::HeaderMap::new(),
            bytes: b"kept".to_vec(),
        },
    );
}

/// A scheduled attempt that failed at `started_at`, as an earlier run left it.
async fn failed_on_schedule(
    connection: &mut AsyncPgConnection,
    location: Uuid,
    started_at: DateTime<Utc>,
) {
    let backup = database_backup::begin(
        connection,
        DatabaseBackupTrigger::Schedule,
        location,
        started_at,
        SYSTEM_USER_ID,
    )
    .await
    .expect("an attempt begins");
    database_backup::finish(
        connection,
        backup.id,
        BackupOutcome::Failed("Bunny refused the password".to_owned()),
    )
    .await
    .expect("an attempt fails");
}

async fn raised_items(connection: &mut AsyncPgConnection) -> Vec<String> {
    action_items::table
        .filter(action_items::created_by.eq(SYSTEM_USER_ID))
        .select(action_items::title)
        .load(connection)
        .await
        .expect("items")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_backup_stores_every_database_and_records_its_size() {
    let mut fixture = fixture(None).await;
    choose(&mut fixture.connection, Some(fixture.location.id)).await;

    let backup = fixture
        .backups
        .back_up(DatabaseBackupTrigger::Admin, TEST_PERSON_ID)
        .await
        .expect("a backup begins");

    assert_eq!(backup.status, DatabaseBackupStatus::Succeeded, "{backup:?}");
    assert_eq!(backup.created_by, TEST_PERSON_ID);
    let databases = databases(&mut fixture.connection).await.expect("names");
    assert_eq!(databases[1..], ["kratos", "hydra"]);
    let mut expected: Vec<String> = databases
        .iter()
        .map(|database| files::dump_path(backup.started_at, database))
        .collect();
    expected.sort();
    assert_eq!(stored_paths(&fixture.files), expected);

    let bytes: usize = databases
        .iter()
        .map(|database| format!("fake dump of {database}").len())
        .sum();
    assert_eq!(
        backup.size_bytes,
        Some(i64::try_from(bytes).expect("small"))
    );
    assert!(
        fixture.activity.is_idle(),
        "the backup's activity ended with it"
    );
    assert!(
        !fixture.backups.inner.deps.staging_directory.exists(),
        "every staged dump is removed once the backup ends"
    );
}

/// A backup is whole or absent: the location never holds part of one.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_failed_dump_removes_what_was_uploaded_and_records_why() {
    let mut fixture = fixture(Some("hydra")).await;
    choose(&mut fixture.connection, Some(fixture.location.id)).await;

    let backup = fixture
        .backups
        .back_up(DatabaseBackupTrigger::Admin, TEST_PERSON_ID)
        .await
        .expect("a backup begins");

    assert_eq!(backup.status, DatabaseBackupStatus::Failed);
    let error = backup.error.expect("a failure says why");
    assert!(error.contains("could not dump hydra"), "{error}");
    assert!(error.contains("does not exist"), "{error}");
    assert!(
        stored_paths(&fixture.files).is_empty(),
        "nothing of it is kept"
    );
    assert!(
        !fixture.backups.inner.deps.staging_directory.exists(),
        "a failed dump is not left staged"
    );
    assert!(
        raised_items(&mut fixture.connection).await.is_empty(),
        "whoever asked saw it fail"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn backups_past_retention_are_removed_after_one_succeeds() {
    let mut fixture = fixture(None).await;
    choose(&mut fixture.connection, Some(fixture.location.id)).await;
    let old = files::dump_path(
        Utc::now() - TimeDelta::days(files::RETENTION_DAYS + 1),
        "elysium",
    );
    let recent = files::dump_path(Utc::now() - TimeDelta::days(2), "elysium");
    let unrelated = "database-backups/notes.txt";
    let elsewhere = "studio/20200101T000000Z-elysium.dump";
    for path in [old.as_str(), recent.as_str(), unrelated, elsewhere] {
        store(&fixture.files, path);
    }

    let backup = fixture
        .backups
        .back_up(DatabaseBackupTrigger::Admin, TEST_PERSON_ID)
        .await
        .expect("a backup begins");

    assert_eq!(backup.status, DatabaseBackupStatus::Succeeded);
    let stored = stored_paths(&fixture.files);
    assert!(!stored.contains(&old), "{stored:?}");
    for kept in [recent.as_str(), unrelated, elsewhere] {
        assert!(stored.iter().any(|path| path == kept), "{kept} is kept");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_backup_cannot_begin_without_a_destination() {
    let fixture = fixture(None).await;
    let refused = fixture
        .backups
        .back_up(DatabaseBackupTrigger::Admin, TEST_PERSON_ID)
        .await;
    assert!(
        matches!(refused, Err(BeginError::NoDestination)),
        "{refused:?}"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_backup_cannot_begin_while_another_runs() {
    let mut fixture = fixture(None).await;
    choose(&mut fixture.connection, Some(fixture.location.id)).await;
    database_backup::begin(
        &mut fixture.connection,
        DatabaseBackupTrigger::Deployment,
        fixture.location.id,
        Utc::now().trunc_subsecs(0),
        SYSTEM_USER_ID,
    )
    .await
    .expect("one is running");

    let refused = fixture
        .backups
        .back_up(DatabaseBackupTrigger::Admin, TEST_PERSON_ID)
        .await;
    assert!(
        matches!(refused, Err(BeginError::AlreadyRunning)),
        "{refused:?}"
    );
}

/// Earlier attempts are retried within the night; only its last failure is reported, once.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_nights_last_scheduled_failure_raises_one_inbox_item() {
    let mut fixture = fixture(Some("kratos")).await;
    choose(&mut fixture.connection, Some(fixture.location.id)).await;

    let first = fixture
        .backups
        .back_up(DatabaseBackupTrigger::Schedule, SYSTEM_USER_ID)
        .await
        .expect("a backup begins");
    assert_eq!(first.status, DatabaseBackupStatus::Failed);
    assert!(raised_items(&mut fixture.connection).await.is_empty());

    let slot = schedule::latest_slot(Utc::now());
    failed_on_schedule(&mut fixture.connection, fixture.location.id, slot).await;
    let last = fixture
        .backups
        .back_up(DatabaseBackupTrigger::Schedule, SYSTEM_USER_ID)
        .await
        .expect("a backup begins");
    assert_eq!(last.status, DatabaseBackupStatus::Failed);
    assert_eq!(
        raised_items(&mut fixture.connection).await,
        ["Last night's database backup failed"]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_backup_cut_short_fails_at_the_next_start_and_leaves_nothing() {
    let mut fixture = fixture(None).await;
    choose(&mut fixture.connection, Some(fixture.location.id)).await;
    let started_at = Utc::now().trunc_subsecs(0);
    let running = database_backup::begin(
        &mut fixture.connection,
        DatabaseBackupTrigger::Schedule,
        fixture.location.id,
        started_at,
        SYSTEM_USER_ID,
    )
    .await
    .expect("one is running");
    let partial = files::dump_path(started_at, "kratos");
    store(&fixture.files, &partial);

    fixture
        .backups
        .settle_after_restart()
        .await
        .expect("settled");

    let settled = database_backup::find(&mut fixture.connection, running.id)
        .await
        .expect("read")
        .expect("kept");
    assert_eq!(settled.status, DatabaseBackupStatus::Failed);
    assert_eq!(settled.error.as_deref(), Some(INTERRUPTED));
    assert!(stored_paths(&fixture.files).is_empty());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_status_names_the_destination_and_when_the_schedule_runs_next() {
    let mut fixture = fixture(None).await;

    let off = fixture.backups.status().await.expect("a status");
    assert!(off.destination.is_none());
    assert_eq!(
        off.next_attempt_at, None,
        "nothing is scheduled without a location"
    );

    choose(&mut fixture.connection, Some(fixture.location.id)).await;
    fixture
        .backups
        .back_up(DatabaseBackupTrigger::Admin, TEST_PERSON_ID)
        .await
        .expect("a backup begins");
    let on = fixture.backups.status().await.expect("a status");
    let destination = on.destination.expect("a destination");
    assert_eq!(destination.name, "Offsite");
    assert_eq!(
        on.next_attempt_at,
        Some(schedule::next_slot(Utc::now())),
        "tonight's backup is done"
    );
    assert_eq!(on.history.len(), 1);
}

/// A location backups go to cannot be deleted, or backups would stop without anyone saying.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_destination_cannot_be_deleted_while_it_is_chosen() {
    let mut fixture = fixture(None).await;
    choose(&mut fixture.connection, Some(fixture.location.id)).await;

    let refused = storage_location::delete(&mut fixture.connection, fixture.location.id).await;
    let Err(DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, info)) = refused
    else {
        panic!("deleting the destination is refused: {refused:?}");
    };
    assert_eq!(
        info.constraint_name(),
        Some(storage_location::BACKUP_DESTINATION_CONSTRAINT)
    );

    choose(&mut fixture.connection, None).await;
    storage_location::delete(&mut fixture.connection, fixture.location.id)
        .await
        .expect("a location no longer chosen can be deleted");
}
