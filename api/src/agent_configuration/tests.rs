// Copyright © 2026 Jalapeno Labs

//! The hold on workers and Project Managers, and its one inbox item.

use chrono::TimeDelta;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use serde_json::json;

use super::*;
use crate::database::schema::workspace_settings;
use crate::models::user::SYSTEM_USER_ID;
use crate::runner::configuration::{Checkout, Source};
use crate::test_support::migrated_database;

const REPOSITORY: &str = "https://github.com/navarrotech/agents";

fn settings(repository: Option<&str>, github_credential_id: Option<Uuid>) -> WorkspaceSettings {
    WorkspaceSettings {
        signup_open: true,
        require_mfa: false,
        updated_by: SYSTEM_USER_ID,
        updated_at: Utc::now(),
        agent_configuration_repository: repository.map(str::to_owned),
        agent_configuration_github_credential_id: github_credential_id,
        backup_storage_location_id: None,
        worker_pool_size: None,
        page_attachment_storage_location_id: None,
        meeting_attachment_max_bytes: 1_000_000_000,
    }
}

fn status(repository: Option<&str>, github_credential_id: Option<Uuid>) -> Status {
    Status {
        repository: repository.map(str::to_owned),
        github_credential_id,
        commit_sha: None,
        commit_subject: None,
        fetched_at: None,
        skipped: json!([]),
        error: None,
        failed_at: None,
        inbox_item_id: None,
        updated_at: Utc::now(),
    }
}

fn fetched(mut status: Status) -> Status {
    status.commit_sha = Some("611c46a583443c3a8dd4caad4a89dffa964add43".to_owned());
    status.commit_subject = Some("Memory".to_owned());
    status.fetched_at = Some(Utc::now());
    status
}

fn source(github_credential_id: Option<Uuid>) -> Source {
    Source {
        repository: REPOSITORY.to_owned(),
        github_credential_id,
    }
}

fn checkout() -> Checkout {
    Checkout {
        repository: REPOSITORY.to_owned(),
        commit: "611c46a583443c3a8dd4caad4a89dffa964add43".to_owned(),
        subject: "Memory: Elysium production".to_owned(),
        fetched_at: Utc::now(),
        skipped: Vec::new(),
    }
}

#[test]
fn agents_start_only_on_a_checkout_of_the_repository_and_token_the_settings_name() {
    let token = Some(Uuid::now_v7());
    assert_eq!(
        hold(&settings(None, None), &status(None, None)),
        Some(Hold::Unset)
    );
    assert_eq!(
        hold(&settings(Some(REPOSITORY), token), &status(None, None)),
        Some(Hold::Fetching),
        "nothing fetched yet"
    );
    assert_eq!(
        hold(
            &settings(Some(REPOSITORY), token),
            &fetched(status(Some(REPOSITORY), None))
        ),
        Some(Hold::Fetching),
        "a checkout read with another token says nothing of this one"
    );
    assert_eq!(
        hold(
            &settings(Some(REPOSITORY), token),
            &fetched(status(Some("https://github.com/someone/else"), token))
        ),
        Some(Hold::Fetching),
        "nor one of another repository"
    );
    assert_eq!(
        hold(
            &settings(Some(REPOSITORY), token),
            &fetched(status(Some(REPOSITORY), token))
        ),
        None
    );

    let failed_at = Utc::now() - TimeDelta::minutes(1);
    let mut failed = fetched(status(Some(REPOSITORY), token));
    failed.error = Some("Repository not found".to_owned());
    failed.failed_at = Some(failed_at);
    assert_eq!(
        hold(&settings(Some(REPOSITORY), token), &failed),
        Some(Hold::Failed {
            error: "Repository not found".to_owned(),
            failed_at,
        }),
        "a failed fetch holds work back even while an older checkout is there"
    );
}

#[test]
fn a_hold_says_its_reason_as_clients_read_it() {
    let failed_at = "2026-11-06T18:00:00Z".parse().expect("a time");
    assert_eq!(
        serde_json::to_value(Hold::Failed {
            error: "denied".to_owned(),
            failed_at,
        })
        .expect("serializes"),
        json!({ "reason": "failed", "error": "denied", "failedAt": "2026-11-06T18:00:00Z" })
    );
    assert_eq!(
        serde_json::to_value(Hold::Unset).expect("serializes"),
        json!({ "reason": "unset" })
    );
}

async fn set_repository(connection: &mut AsyncPgConnection, repository: Option<&str>) {
    diesel::update(workspace_settings::table)
        .set(workspace_settings::agent_configuration_repository.eq(repository))
        .execute(connection)
        .await
        .expect("settings updated");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn held_work_raises_one_inbox_item_until_a_fetch_resolves_it() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    set_repository(&mut connection, Some(REPOSITORY)).await;
    let failed = status_record::record_failed(
        &mut connection,
        &source(None),
        "Repository not found",
        Utc::now(),
    )
    .await
    .expect("recorded");
    let hold = current_hold(&mut connection)
        .await
        .expect("read")
        .expect("held");
    assert!(matches!(hold, Hold::Failed { .. }), "{hold:?}");
    assert_eq!(failed.commit_sha, None);

    raise_hold(&mut connection, &events, &hold)
        .await
        .expect("raised");
    raise_hold(&mut connection, &events, &hold)
        .await
        .expect("raised again");
    let raised = status_record::find(&mut connection)
        .await
        .expect("status")
        .inbox_item_id
        .expect("an item");
    let item = action_item::find(&mut connection, raised)
        .await
        .expect("the item");
    assert_eq!(item.state, ActionItemState::Inbox);
    assert_eq!(
        item.title,
        "The configuration repository could not be fetched"
    );
    assert!(item.notes.contains("Repository not found"));
    let items: i64 = crate::database::schema::action_items::table
        .count()
        .get_result(&mut connection)
        .await
        .expect("counted");
    assert_eq!(
        items, 1,
        "every look that holds work back shares the one item"
    );

    status_record::record_fetched(&mut connection, &source(None), &checkout())
        .await
        .expect("recorded");
    assert_eq!(current_hold(&mut connection).await.expect("read"), None);
    resolve_hold(&mut connection, &events)
        .await
        .expect("resolved");
    let item = action_item::find(&mut connection, raised)
        .await
        .expect("the item");
    assert_eq!(item.state, ActionItemState::Resolved);
    assert_eq!(
        status_record::find(&mut connection)
            .await
            .expect("status")
            .inbox_item_id,
        None
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_item_the_person_dismissed_is_raised_again_and_fetching_raises_none() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    raise_hold(&mut connection, &events, &Hold::Fetching)
        .await
        .expect("nothing raised");
    assert_eq!(
        status_record::find(&mut connection)
            .await
            .expect("status")
            .inbox_item_id,
        None,
        "a fetch under way lifts in moments"
    );

    raise_hold(&mut connection, &events, &Hold::Unset)
        .await
        .expect("raised");
    let first = status_record::find(&mut connection)
        .await
        .expect("status")
        .inbox_item_id
        .expect("an item");
    action_item::transition(
        &mut connection,
        first,
        Transition::Dismiss,
        Actor::System,
        Utc::now(),
    )
    .await
    .expect("dismissed");
    raise_hold(&mut connection, &events, &Hold::Unset)
        .await
        .expect("raised again");
    let second = status_record::find(&mut connection)
        .await
        .expect("status")
        .inbox_item_id
        .expect("an item");
    assert_ne!(first, second, "work still waits, so it is said again");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_failure_keeps_the_last_checkout_of_the_same_repository_only() {
    let (_url, mut connection) = migrated_database().await;
    let other = Source {
        repository: "https://github.com/someone/else".to_owned(),
        github_credential_id: None,
    };
    set_repository(&mut connection, Some(REPOSITORY)).await;
    status_record::record_fetched(&mut connection, &source(None), &checkout())
        .await
        .expect("recorded");
    let failed =
        status_record::record_failed(&mut connection, &source(None), "timed out", Utc::now())
            .await
            .expect("recorded");
    assert_eq!(
        failed.commit_sha.as_deref(),
        Some(checkout().commit.as_str())
    );
    assert_eq!(failed.error.as_deref(), Some("timed out"));

    set_repository(&mut connection, Some(&other.repository)).await;
    let forgotten = status_record::record_failed(&mut connection, &other, "not found", Utc::now())
        .await
        .expect("recorded");
    assert_eq!(
        forgotten.commit_sha, None,
        "a checkout of another repository is forgotten"
    );
    assert_eq!(
        forgotten.repository.as_deref(),
        Some(other.repository.as_str())
    );

    let refetched = status_record::record_fetched(&mut connection, &other, &checkout())
        .await
        .expect("recorded");
    assert_eq!(refetched.error, None);
    assert_eq!(refetched.failed_at, None);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_slow_attempt_at_a_replaced_repository_records_nothing() {
    let (_url, mut connection) = migrated_database().await;
    let replacement = Source {
        repository: "https://github.com/someone/else".to_owned(),
        github_credential_id: None,
    };
    // A worker began on the old repository; meanwhile an admin replaced it and it was fetched.
    set_repository(&mut connection, Some(&replacement.repository)).await;
    let fetched = status_record::record_fetched(&mut connection, &replacement, &checkout())
        .await
        .expect("recorded");

    let stale =
        status_record::record_failed(&mut connection, &source(None), "timed out", Utc::now())
            .await
            .expect("read");
    assert_eq!(
        stale, fetched,
        "the old repository's failure changes nothing"
    );
    let stale = status_record::record_fetched(&mut connection, &source(None), &checkout())
        .await
        .expect("read");
    assert_eq!(stale, fetched, "nor does its success");
    assert_eq!(current_hold(&mut connection).await.expect("read"), None);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_one_item_follows_the_hold_when_it_changes_while_open() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    raise_hold(&mut connection, &events, &Hold::Unset)
        .await
        .expect("raised");
    let raised = status_record::find(&mut connection)
        .await
        .expect("status")
        .inbox_item_id
        .expect("an item");

    let failed = Hold::Failed {
        error: "remote: Repository not found.".to_owned(),
        failed_at: Utc::now(),
    };
    raise_hold(&mut connection, &events, &failed)
        .await
        .expect("raised again");
    let status = status_record::find(&mut connection).await.expect("status");
    assert_eq!(status.inbox_item_id, Some(raised), "still one item");
    let item = action_item::find(&mut connection, raised)
        .await
        .expect("the item");
    assert_eq!(
        item.title,
        "The configuration repository could not be fetched"
    );
    assert!(item.notes.contains("Repository not found"));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_attempt_recorded_while_an_admin_replaces_the_repository_waits_and_records_nothing() {
    use diesel_async::{AsyncConnection, SimpleAsyncConnection};
    use secrecy::ExposeSecret;

    let (url, mut connection) = migrated_database().await;
    set_repository(&mut connection, Some(REPOSITORY)).await;
    let before = status_record::find(&mut connection).await.expect("status");

    // An admin's change to the settings, under way and not yet committed.
    let mut admin = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("a second connection");
    admin
        .batch_execute(
            "BEGIN; UPDATE workspace_settings \
             SET agent_configuration_repository = 'https://github.com/someone/else'",
        )
        .await
        .expect("the change begins");

    let recording = tokio::spawn(async move {
        status_record::record_failed(&mut connection, &source(None), "timed out", Utc::now())
            .await
            .expect("read")
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(!recording.is_finished(), "it waits for the admin's change");
    admin.batch_execute("COMMIT").await.expect("committed");

    let after = recording.await.expect("recorded");
    assert_eq!(
        after, before,
        "an attempt at the replaced repository records nothing"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_repository_saved_with_a_query_is_refused_without_repeating_it() {
    use std::sync::Arc;

    use crate::runner::configuration::ConfigurationSource;

    let (url, mut connection) = migrated_database().await;
    // Saved before the settings refused a query, as no route accepts it now.
    set_repository(
        &mut connection,
        Some("https://github.com/acme/agents.git?token=secret-value"),
    )
    .await;
    let source = ConfigurationSource::Database {
        database: crate::connections::connect_postgres(&url, 2)
            .await
            .expect("Postgres accepts connections"),
        cipher: Arc::new(crate::test_support::cipher()),
    };
    // Named in full: diesel's `RunQueryDsl::load` is in scope too.
    let loaded = ConfigurationSource::load(&source)
        .await
        .expect("the settings read")
        .expect("a repository");
    let refusal = loaded.repository.expect_err("refused");
    assert!(refusal.contains("query or a fragment"), "{refusal}");
    assert!(!refusal.contains("secret-value"), "{refusal}");
}
