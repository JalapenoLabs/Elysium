// Copyright © 2026 Jalapeno Labs

//! What makes two reports one finding, and how reports land: the fingerprint's spellings, a
//! second report counted on the first, two at once landing on one row, a dismissed finding
//! staying dismissed however often it is seen again, and a decision taken once.
//!
//! The database tests are `#[ignore]`d so `cargo test` stays hermetic;
//! `api/scripts/verify-migrations.sh` runs them.

use diesel_async::AsyncConnection as _;
use secrecy::ExposeSecret as _;
use serde_json::json;

use super::*;
use crate::models::project::{self, NewProject};
use crate::models::work_batch::{NewWorkBatch, Priority};
use crate::test_support::{TEST_PERSON_ID, approved_batch, migrated_database, person_rooted};

fn reported(title: &str, repository_url: Option<&str>, location: &str) -> NewFinding {
    NewFinding {
        title: title.to_owned(),
        detail: "The retry loop never gives up when the provider answers 429.".to_owned(),
        repository_url: repository_url.map(str::to_owned),
        location: location.to_owned(),
        cost: Cost::Small,
    }
}

#[test]
fn a_repository_spelled_any_way_git_accepts_is_one_repository() {
    let spellings = [
        "https://github.com/JalapenoLabs/Elysium",
        "https://github.com/jalapenolabs/elysium.git",
        "https://github.com/JalapenoLabs/Elysium/",
        "git@github.com:JalapenoLabs/Elysium.git",
        "ssh://github.com/JalapenoLabs/Elysium",
    ];
    let first = fingerprint(&reported(
        "Retry forever",
        Some(spellings[0]),
        "src/retry.rs",
    ));
    for spelling in spellings {
        let again = fingerprint(&reported("Retry forever", Some(spelling), "src/retry.rs"));
        assert_eq!(again, first, "{spelling}");
    }
}

#[test]
fn lines_case_and_punctuation_never_make_a_second_finding() {
    let first = fingerprint(&reported("Retry forever on 429", None, "api/src/retry.rs"));
    for (title, location) in [
        ("retry forever, on 429!", "api/src/retry.rs"),
        ("  Retry   FOREVER on 429 ", "./api/src/retry.rs"),
        ("Retry forever on 429", "api/src/retry.rs:12"),
        ("Retry forever on 429", "api/src/retry.rs:12:5"),
        ("Retry forever on 429", "API/src/retry.rs#L12"),
        ("Retry forever on 429", "api/src/retry.rs#L12-L20"),
    ] {
        assert_eq!(
            fingerprint(&reported(title, None, location)),
            first,
            "{title} at {location}"
        );
    }
}

#[test]
fn another_place_title_or_repository_is_another_finding() {
    let first = fingerprint(&reported(
        "Retry forever",
        Some("https://github.com/JalapenoLabs/Elysium"),
        "src/retry.rs",
    ));
    let others = [
        reported(
            "Retry forever",
            Some("https://github.com/JalapenoLabs/Elysium"),
            "src/backoff.rs",
        ),
        reported(
            "Retry too soon",
            Some("https://github.com/JalapenoLabs/Elysium"),
            "src/retry.rs",
        ),
        reported(
            "Retry forever",
            Some("https://github.com/JalapenoLabs/Seraphim"),
            "src/retry.rs",
        ),
        reported("Retry forever", None, "src/retry.rs"),
    ];
    for other in others {
        assert_ne!(fingerprint(&other), first, "{other:?}");
    }
}

#[test]
fn a_fingerprint_is_what_its_column_admits() {
    let fingerprint = fingerprint(&reported("Retry forever", None, "src/retry.rs"));
    assert_eq!(fingerprint.len(), 64);
    assert!(
        fingerprint
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    );
}

#[test]
fn a_place_keeps_what_is_not_a_line_number() {
    assert_eq!(normalized_location("Settings page"), "settings page");
    assert_eq!(
        normalized_location("docs/api.md#Errors"),
        "docs/api.md#errors"
    );
    assert_eq!(
        normalized_location("https://example.com/pricing"),
        "https://example.com/pricing"
    );
}

async fn project_named(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    project::create(
        connection,
        &NewProject {
            name: name.to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id
}

/// A worker's batch on `project_id`, which a finding it reports names.
async fn workers_batch(connection: &mut AsyncPgConnection, project_id: Uuid) -> Uuid {
    approved_batch(
        connection,
        NewWorkBatch {
            id: Uuid::now_v7(),
            project_id,
            title: "Annual pricing".to_owned(),
            goal: "Show annual pricing on the pricing page.".to_owned(),
            repositories: json!([]),
            priority: Priority::Normal,
            scheduled_for: None,
            turn_id: None,
            source_record_ids: json!([]),
            action_item_ids: json!([]),
            source_message_ids: json!([]),
            lineage: person_rooted(),
            created_by: crate::models::user::PROJECT_MANAGER_USER_ID,
        },
    )
    .await
    .id
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_same_finding_reported_again_is_counted_on_the_first() {
    let (_url, mut connection) = migrated_database().await;
    let project_id = project_named(&mut connection, "Launch").await;
    let batch_id = workers_batch(&mut connection, project_id).await;
    let worker = Reporter::Worker { batch_id };

    let first = report(
        &mut connection,
        project_id,
        worker,
        reported("Retry forever", None, "src/retry.rs:12"),
        Utc::now(),
    )
    .await
    .expect("reported");
    assert!(first.is_new);
    assert_eq!(first.finding.state, FindingState::Open);
    assert_eq!(first.finding.created_by, CODING_AGENT_USER_ID);
    assert_eq!(first.finding.work_batch_id, Some(batch_id));

    let again = report(
        &mut connection,
        project_id,
        worker,
        reported("retry forever.", None, "src/retry.rs:40"),
        Utc::now(),
    )
    .await
    .expect("reported");
    assert!(!again.is_new);
    assert_eq!(again.finding.id, first.finding.id);
    assert_eq!(again.finding.reports, 2);
    assert_eq!(
        again.finding.location, "src/retry.rs:12",
        "what the first report said stays"
    );

    // Another project holds its own findings, so the same words there are new.
    let elsewhere = project_named(&mut connection, "Elsewhere").await;
    let other_batch = workers_batch(&mut connection, elsewhere).await;
    let theirs = report(
        &mut connection,
        elsewhere,
        Reporter::Worker {
            batch_id: other_batch,
        },
        reported("Retry forever", None, "src/retry.rs"),
        Utc::now(),
    )
    .await
    .expect("reported");
    assert!(theirs.is_new);

    assert_eq!(
        count_open(&mut connection, project_id)
            .await
            .expect("counted"),
        1
    );
    let mut counts = open_counts(&mut connection).await.expect("counted");
    counts.sort();
    let mut expected = vec![(project_id, 1), (elsewhere, 1)];
    expected.sort();
    assert_eq!(counts, expected);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn two_reports_at_once_land_on_one_finding() {
    let (url, mut connection) = migrated_database().await;
    let project_id = project_named(&mut connection, "Launch").await;
    let batch_id = workers_batch(&mut connection, project_id).await;
    let mut second_connection = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("a second connection");

    let worker = Reporter::Worker { batch_id };
    let (first, second) = tokio::join!(
        report(
            &mut connection,
            project_id,
            worker,
            reported("Retry forever", None, "src/retry.rs"),
            Utc::now(),
        ),
        report(
            &mut second_connection,
            project_id,
            worker,
            reported("Retry forever", None, "src/retry.rs"),
            Utc::now(),
        ),
    );
    let (first, second) = (first.expect("reported"), second.expect("reported"));
    assert_eq!(first.finding.id, second.finding.id);
    assert_eq!(
        u8::from(first.is_new) + u8::from(second.is_new),
        1,
        "exactly one of them made it"
    );
    let listed = list_open(&mut connection, project_id)
        .await
        .expect("listed");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].reports, 2);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_dismissed_finding_stays_dismissed_and_is_decided_once() {
    let (_url, mut connection) = migrated_database().await;
    let project_id = project_named(&mut connection, "Launch").await;
    let batch_id = workers_batch(&mut connection, project_id).await;
    let worker = Reporter::Worker { batch_id };
    let first = report(
        &mut connection,
        project_id,
        worker,
        reported("Retry forever", None, "src/retry.rs"),
        Utc::now(),
    )
    .await
    .expect("reported");
    let id = first.finding.id;

    lock_open(&mut connection, id).await.expect("open");
    let dismissed = record_dismissed(
        &mut connection,
        id,
        TEST_PERSON_ID,
        Utc::now(),
        Some("The provider never answers 429."),
    )
    .await
    .expect("dismissed");
    assert_eq!(dismissed.state, FindingState::Dismissed);
    assert_eq!(dismissed.decided_by, Some(TEST_PERSON_ID));

    let again = report(
        &mut connection,
        project_id,
        worker,
        reported("Retry forever", None, "./src/retry.rs#L3"),
        Utc::now(),
    )
    .await
    .expect("reported");
    assert!(!again.is_new);
    assert_eq!(again.finding.state, FindingState::Dismissed);
    assert_eq!(
        again.finding.dismissal_reason.as_deref(),
        Some("The provider never answers 429.")
    );
    assert!(
        list_open(&mut connection, project_id)
            .await
            .expect("listed")
            .is_empty(),
        "a dismissed finding is never raised again"
    );

    let refused = lock_open(&mut connection, id)
        .await
        .expect_err("decided once");
    assert!(matches!(
        refused,
        DecideError::Decided(FindingState::Dismissed)
    ));
    let unknown = lock_open(&mut connection, Uuid::now_v7())
        .await
        .expect_err("no such finding");
    assert!(matches!(unknown, DecideError::NotFound));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_project_manager_reports_by_its_turn() {
    use crate::models::project_manager;

    let (_url, mut connection) = migrated_database().await;
    let project_id = project_named(&mut connection, "Launch").await;
    project_manager::wake_with_message(&mut connection, project_id, "Plan it.", TEST_PERSON_ID)
        .await
        .expect("woken");
    let (turn, _wakes) = project_manager::start_turn(&mut connection, project_id, 40)
        .await
        .expect("runs")
        .expect("a turn");

    let reported = report(
        &mut connection,
        project_id,
        Reporter::ProjectManager { turn_id: turn.id },
        reported("No pricing page in the docs", None, "Docs, Pricing"),
        Utc::now(),
    )
    .await
    .expect("reported");
    assert_eq!(reported.finding.project_manager_turn_id, Some(turn.id));
    assert_eq!(reported.finding.work_batch_id, None);
    assert_eq!(reported.finding.created_by, PROJECT_MANAGER_USER_ID);
}
