// Copyright © 2026 Jalapeno Labs

//! Work batches against Postgres: slots handed out under leases, fenced, taken over after a
//! restart, and given back; and what people, Elysia, and Project Managers change while a batch
//! waits, each in its history. A person's decision on a proposed batch is `approval/tests.rs`.

use secrecy::ExposeSecret;

use super::*;
use crate::database::schema::project_manager_wakes;
use crate::models::project::{self, NewProject};
use crate::models::user::ELYSIA_USER_ID;
use crate::test_support::{TEST_PERSON_ID, approved_batch, migrated_database};

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

/// A batch a person approved, waiting for a slot.
async fn submitted(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    priority: Priority,
    scheduled_for: Option<DateTime<Utc>>,
) -> WorkBatch {
    approved_batch(
        connection,
        NewWorkBatch {
            id: Uuid::now_v7(),
            project_id,
            title: "Annual pricing".to_owned(),
            goal: "Show annual pricing on the pricing page.".to_owned(),
            repositories: json!([]),
            priority,
            scheduled_for,
            turn_id: None,
            source_record_ids: json!([]),
            action_item_ids: json!([]),
            created_by: PROJECT_MANAGER_USER_ID,
        },
    )
    .await
}

async fn kinds(connection: &mut AsyncPgConnection, id: Uuid) -> Vec<String> {
    history(connection, id)
        .await
        .expect("history")
        .into_iter()
        .map(|entry| entry.kind)
        .collect()
}

async fn batch_wakes(connection: &mut AsyncPgConnection, project_id: Uuid) -> i64 {
    project_manager_wakes::table
        .filter(project_manager_wakes::project_id.eq(project_id))
        .filter(project_manager_wakes::work_batch_id.is_not_null())
        .count()
        .get_result(connection)
        .await
        .expect("counted")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_slot_is_handed_out_within_the_size_fenced_by_its_token_and_given_back() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let first = submitted(&mut connection, launch, Priority::Asap, None).await;
    let second = submitted(&mut connection, launch, Priority::Normal, None).await;
    let now = Utc::now();

    let slots = assign(&mut connection, &[first.id, second.id], 1, now)
        .await
        .expect("assigns");
    assert_eq!(slots.len(), 1, "one slot in a pool of one");
    let slot = &slots[0];
    assert_eq!(slot.batch.id, first.id);
    assert_eq!(slot.batch.state, BatchState::Assigned);
    assert!(
        assign(&mut connection, &[second.id], 1, now)
            .await
            .expect("assigns")
            .is_empty(),
        "a full pool hands out nothing"
    );

    // A stale token records nothing.
    let stale = Slot {
        batch: slot.batch.clone(),
        token: Uuid::now_v7(),
    };
    assert!(
        start(&mut connection, &stale)
            .await
            .expect("runs")
            .is_none()
    );
    let running = start(&mut connection, slot)
        .await
        .expect("runs")
        .expect("started");
    assert_eq!(running.state, BatchState::Running);
    assert!(
        finish(&mut connection, &stale, true, "Done.", SYSTEM_USER_ID)
            .await
            .expect("runs")
            .is_none()
    );

    let finished = finish(&mut connection, slot, true, "Done.", SYSTEM_USER_ID)
        .await
        .expect("runs")
        .expect("finished");
    assert_eq!(finished.state, BatchState::Succeeded);
    assert!(in_slots(&mut connection).await.expect("reads").is_empty());
    assert_eq!(batch_wakes(&mut connection, launch).await, 1);
    assert_eq!(
        kinds(&mut connection, first.id).await,
        ["submitted", "approved", "assigned", "started", "finished"]
    );

    let next = assign(&mut connection, &[second.id], 1, now)
        .await
        .expect("assigns");
    assert_eq!(
        next.len(),
        1,
        "the slot it gave back goes to the next batch"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_next_engine_takes_over_slots_whose_leases_lapsed_and_the_old_token_is_fenced() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let crashed = submitted(&mut connection, launch, Priority::Normal, None).await;
    let stopped = submitted(&mut connection, launch, Priority::Normal, None).await;
    let now = Utc::now();
    let slots = assign(&mut connection, &[crashed.id, stopped.id], 2, now)
        .await
        .expect("assigns");
    let crashed_slot = slots
        .iter()
        .find(|slot| slot.batch.id == crashed.id)
        .expect("one");
    let stopped_slot = slots
        .iter()
        .find(|slot| slot.batch.id == stopped.id)
        .expect("one");
    start(&mut connection, crashed_slot)
        .await
        .expect("runs")
        .expect("started");

    // The engine holding `stopped` stops cleanly and lets it go; the one holding `crashed` died.
    release(&mut connection, &[(stopped.id, stopped_slot.token)], now)
        .await
        .expect("released");
    let taken = adopt(&mut connection, now).await.expect("adopts");
    assert_eq!(
        taken.iter().map(|slot| slot.batch.id).collect::<Vec<_>>(),
        [stopped.id],
        "a released slot is taken at once; a held lease is left alone"
    );

    let after_the_lease = now + LEASE + TimeDelta::seconds(1);
    let adopted = adopt(&mut connection, after_the_lease)
        .await
        .expect("adopts");
    let crashed_again = adopted
        .iter()
        .find(|slot| slot.batch.id == crashed.id)
        .expect("taken over once its lease lapsed");
    assert_eq!(
        crashed_again.batch.state,
        BatchState::Running,
        "a running batch is found again, never started over"
    );
    assert_ne!(crashed_again.token, crashed_slot.token);
    assert!(
        renew(
            &mut connection,
            &[(crashed.id, crashed_slot.token)],
            after_the_lease
        )
        .await
        .expect("renews")
        .is_empty(),
        "the engine that lost the slot learns so on its next renewal"
    );
    assert!(
        finish(
            &mut connection,
            crashed_slot,
            false,
            "Lost.",
            SYSTEM_USER_ID
        )
        .await
        .expect("runs")
        .is_none()
    );
    assert_eq!(
        renew(
            &mut connection,
            &[(crashed.id, crashed_again.token)],
            after_the_lease
        )
        .await
        .expect("renews"),
        [crashed.id]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn two_passes_at_once_never_hand_out_more_slots_than_the_pool_holds() {
    let (url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let mut ids = Vec::new();
    for _index in 0..6 {
        ids.push(
            submitted(&mut connection, launch, Priority::Normal, None)
                .await
                .id,
        );
    }
    let mut other = AsyncPgConnection::establish(url.expose_secret())
        .await
        .expect("a second connection");
    let now = Utc::now();
    let reversed: Vec<Uuid> = ids.iter().rev().copied().collect();

    let (one, two) = tokio::join!(
        assign(&mut connection, &ids, 3, now),
        assign(&mut other, &reversed, 3, now),
    );
    assert_eq!(one.expect("assigns").len() + two.expect("assigns").len(), 3);
    assert_eq!(in_slots(&mut connection).await.expect("reads").len(), 3);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_waiting_batch_is_paused_resumed_run_next_and_reprioritized_with_its_history() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let batch = submitted(&mut connection, launch, Priority::Normal, None).await;
    let now = Utc::now();
    let run_next = Changes {
        run_next: Some(true),
        ..Changes::default()
    };
    let pause = Changes {
        paused: Some(true),
        ..Changes::default()
    };

    let (pinned, changed) = change(&mut connection, batch.id, run_next, TEST_PERSON_ID, now)
        .await
        .expect("runs next");
    assert!(changed && pinned.pinned_at.is_some());
    let (_same, changed) = change(&mut connection, batch.id, run_next, TEST_PERSON_ID, now)
        .await
        .expect("idempotent");
    assert!(!changed, "asking twice changes nothing and records nothing");

    let (paused, _changed) = change(&mut connection, batch.id, pause, ELYSIA_USER_ID, now)
        .await
        .expect("pauses");
    assert_eq!(paused.state, BatchState::Paused);
    assert!(
        paused.pinned_at.is_none(),
        "a paused batch gives up its place at the front"
    );
    let refused = change(&mut connection, batch.id, run_next, TEST_PERSON_ID, now)
        .await
        .expect_err("a paused batch does not run next");
    assert!(matches!(refused, ChangeError::Refused(_)));

    let tomorrow = now + TimeDelta::days(1);
    let (rescheduled, _changed) = change(
        &mut connection,
        batch.id,
        Changes {
            priority: Some((Priority::Scheduled, Some(tomorrow))),
            paused: Some(false),
            ..Changes::default()
        },
        TEST_PERSON_ID,
        now,
    )
    .await
    .expect("reschedules and resumes");
    assert_eq!(rescheduled.state, BatchState::Queued);
    assert_eq!(rescheduled.priority, Priority::Scheduled);
    let not_yet = change(&mut connection, batch.id, run_next, TEST_PERSON_ID, now)
        .await
        .expect_err("a batch scheduled for later is not due");
    assert!(matches!(not_yet, ChangeError::Refused(reason) if reason.contains("asap")));
    let in_the_past = change(
        &mut connection,
        batch.id,
        Changes {
            priority: Some((Priority::Scheduled, Some(now - TimeDelta::hours(1)))),
            ..Changes::default()
        },
        TEST_PERSON_ID,
        now,
    )
    .await
    .expect_err("a time in the past");
    assert!(matches!(in_the_past, ChangeError::Invalid(_)));

    let entries = history(&mut connection, batch.id).await.expect("history");
    let kinds: Vec<&str> = entries.iter().map(|entry| entry.kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["submitted", "approved", "pinned", "paused", "reprioritized", "resumed"]
    );
    assert_eq!(entries[3].created_by, ELYSIA_USER_ID);
    assert_eq!(entries[3].data, json!({ "from": "queued" }));
    assert_eq!(
        entries[4].data["before"],
        json!({ "priority": "normal", "scheduledFor": null })
    );
    assert_eq!(entries[4].data["after"]["priority"], "scheduled");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn pausing_a_batch_in_a_slot_gives_the_slot_back_and_its_holder_lets_it_go() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let batch = submitted(&mut connection, launch, Priority::Asap, None).await;
    let now = Utc::now();
    let slots = assign(&mut connection, &[batch.id], 4, now)
        .await
        .expect("assigns");

    let (paused, _changed) = change(
        &mut connection,
        batch.id,
        Changes {
            paused: Some(true),
            ..Changes::default()
        },
        TEST_PERSON_ID,
        now,
    )
    .await
    .expect("pauses");
    assert_eq!(paused.state, BatchState::Paused);
    assert!(paused.assigned_at.is_none() && paused.lease_expires_at.is_none());
    assert!(
        renew(&mut connection, &[(batch.id, slots[0].token)], now)
            .await
            .expect("renews")
            .is_empty()
    );
    assert!(
        start(&mut connection, &slots[0])
            .await
            .expect("runs")
            .is_none(),
        "no worker starts a batch whose slot was given back"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_shrunken_pool_takes_back_a_slot_no_worker_started_on() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let batch = submitted(&mut connection, launch, Priority::Normal, None).await;
    let slots = assign(&mut connection, &[batch.id], 1, Utc::now())
        .await
        .expect("assigns");

    let queued = unassign(&mut connection, &slots[0])
        .await
        .expect("runs")
        .expect("taken back");
    assert_eq!(queued.state, BatchState::Queued);
    assert!(queued.assigned_at.is_none());
    assert_eq!(
        kinds(&mut connection, batch.id).await,
        ["submitted", "approved", "assigned", "unassigned"]
    );
    assert!(
        unassign(&mut connection, &slots[0])
            .await
            .expect("runs")
            .is_none(),
        "a slot taken back once is not taken back twice"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn cancelling_tells_the_project_manager_unless_it_withdrew_the_batch_itself() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let elsewhere = project_named(&mut connection, "Elsewhere").await;
    let by_a_person = submitted(&mut connection, launch, Priority::Normal, None).await;
    let by_itself = submitted(&mut connection, launch, Priority::Normal, None).await;

    let cancelled = cancel(
        &mut connection,
        by_a_person.id,
        None,
        "Launch moved a week.",
        TEST_PERSON_ID,
    )
    .await
    .expect("cancels")
    .batch;
    assert_eq!(cancelled.state, BatchState::Cancelled);
    assert_eq!(cancelled.result.as_deref(), Some("Launch moved a week."));
    assert_eq!(batch_wakes(&mut connection, launch).await, 1);

    let other_project = cancel(
        &mut connection,
        by_itself.id,
        Some(elsewhere),
        "Not mine.",
        PROJECT_MANAGER_USER_ID,
    )
    .await
    .expect_err("another project's batch");
    assert!(matches!(other_project, ChangeError::NotFound));
    cancel(
        &mut connection,
        by_itself.id,
        Some(launch),
        "Superseded.",
        PROJECT_MANAGER_USER_ID,
    )
    .await
    .expect("its own");
    assert_eq!(batch_wakes(&mut connection, launch).await, 1);

    let again = cancel(
        &mut connection,
        by_itself.id,
        None,
        "Twice.",
        TEST_PERSON_ID,
    )
    .await
    .expect_err("already cancelled");
    assert!(matches!(again, ChangeError::Refused(_)));
    let changed = change(
        &mut connection,
        by_itself.id,
        Changes {
            paused: Some(true),
            ..Changes::default()
        },
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect_err("a cancelled batch is not paused");
    assert!(matches!(changed, ChangeError::Refused(_)));
}
