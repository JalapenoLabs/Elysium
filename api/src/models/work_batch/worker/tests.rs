// Copyright © 2026 Jalapeno Labs

//! What a worker records, against Postgres: its report filed by its tools and taken once by the
//! engine holding its slot, a question that wakes its Project Manager and an answer that ends
//! the wait, and its session and log kept by the slot's holder alone.

use chrono::TimeDelta;
use serde_json::json;

use super::*;
use crate::database::schema::project_manager_wakes;
use crate::models::project::{self, NewProject};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::{NewWorkBatch, Priority, assign, find, finish, history, start};
use crate::test_support::{TEST_PERSON_ID, approved_batch, migrated_database};

/// A running batch of a new project named `name`, and the slot its worker holds.
async fn running(connection: &mut AsyncPgConnection, name: &str) -> (Uuid, Slot) {
    let project_id = project::create(
        connection,
        &NewProject {
            name: name.to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id;
    let batch = approved_batch(
        connection,
        NewWorkBatch {
            id: Uuid::now_v7(),
            project_id,
            title: "Annual pricing".to_owned(),
            goal: "Show annual pricing on the pricing page.".to_owned(),
            repositories: json!([]),
            priority: Priority::Asap,
            scheduled_for: None,
            turn_id: None,
            source_record_ids: json!([]),
            action_item_ids: json!([]),
            serves: None,
            created_by: PROJECT_MANAGER_USER_ID,
        },
    )
    .await;
    let slot = assign(connection, &[batch.id], 1, Utc::now())
        .await
        .expect("assigns")
        .remove(0);
    let started = start(connection, &slot)
        .await
        .expect("runs")
        .expect("started");
    (
        project_id,
        Slot {
            batch: started,
            token: slot.token,
        },
    )
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
async fn a_report_is_filed_by_the_workers_tools_and_taken_once_by_its_engine() {
    let (_url, mut connection) = migrated_database().await;
    let (project_id, slot) = running(&mut connection, "Launch").await;
    let filed = Report::Finished {
        succeeded: true,
        result: "Opened JalapenoLabs/Launch#12.".to_owned(),
    };

    assert!(
        !report(&mut connection, slot.batch.id, Uuid::now_v7(), &filed)
            .await
            .expect("runs"),
        "a batch of another project is never reported on"
    );
    assert!(
        report(&mut connection, slot.batch.id, project_id, &filed)
            .await
            .expect("runs")
    );
    let stale = Slot {
        batch: slot.batch.clone(),
        token: Uuid::now_v7(),
    };
    assert_eq!(
        take_report(&mut connection, &stale).await.expect("runs"),
        None,
        "only the engine holding the slot takes it"
    );
    assert_eq!(
        take_report(&mut connection, &slot).await.expect("runs"),
        Some(filed)
    );
    assert_eq!(
        take_report(&mut connection, &slot).await.expect("runs"),
        None,
        "taken once"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_question_wakes_the_project_manager_whose_answer_ends_the_wait() {
    let (_url, mut connection) = migrated_database().await;
    let (project_id, slot) = running(&mut connection, "Launch").await;
    let later = Utc::now() + TimeDelta::hours(4);

    let waiting = wait(
        &mut connection,
        &slot,
        Wait::Question,
        "Should annual plans get the launch discount too?",
        later,
    )
    .await
    .expect("runs")
    .expect("waiting");
    assert_eq!(waiting.waiting_on, Some(Wait::Question));
    assert_eq!(
        batch_wakes(&mut connection, project_id).await,
        1,
        "the question wakes its Project Manager"
    );

    let refused = answer(
        &mut connection,
        slot.batch.id,
        Uuid::now_v7(),
        "Yes.",
        PROJECT_MANAGER_USER_ID,
    )
    .await
    .expect_err("another project's batch");
    assert!(matches!(refused, AnswerError::Refused(_)));

    let answered = answer(
        &mut connection,
        slot.batch.id,
        project_id,
        "Yes, both.",
        PROJECT_MANAGER_USER_ID,
    )
    .await
    .expect("answered");
    assert!(
        answered.waiting_until.expect("still waiting") <= Utc::now(),
        "an answer ends the wait now"
    );
    assert_eq!(
        answers_since_waiting(&mut connection, slot.batch.id)
            .await
            .expect("reads"),
        ["Yes, both."]
    );

    let resumed = stop_waiting(&mut connection, &slot)
        .await
        .expect("runs")
        .expect("resumed");
    assert_eq!(resumed.waiting_on, None);
    let finished = finish(&mut connection, &slot, true, "Done.", CODING_AGENT_USER_ID)
        .await
        .expect("runs")
        .expect("finished");
    assert_eq!(finished.state, BatchState::Succeeded);
    assert_eq!(
        kinds(&mut connection, slot.batch.id).await,
        [
            "submitted",
            "approved",
            "assigned",
            "started",
            "waiting",
            "answered",
            "finished"
        ]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_wait_on_review_asks_nobody_and_finishing_clears_it() {
    let (_url, mut connection) = migrated_database().await;
    let (project_id, slot) = running(&mut connection, "Pricing").await;
    wait(
        &mut connection,
        &slot,
        Wait::Review,
        "Waiting on a review of #3.",
        Utc::now() + TimeDelta::hours(1),
    )
    .await
    .expect("runs")
    .expect("waiting");
    assert_eq!(batch_wakes(&mut connection, project_id).await, 0);
    let refused = answer(
        &mut connection,
        slot.batch.id,
        project_id,
        "Approved.",
        PROJECT_MANAGER_USER_ID,
    )
    .await
    .expect_err("no question to answer");
    assert!(matches!(refused, AnswerError::Refused(_)));

    // The database refuses a wait on a batch no worker has, so finishing must clear it.
    let finished = finish(
        &mut connection,
        &slot,
        false,
        "Abandoned.",
        CODING_AGENT_USER_ID,
    )
    .await
    .expect("runs")
    .expect("finished");
    assert_eq!(finished.waiting_on, None);
    assert_eq!(finished.waiting_until, None);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_workers_session_and_log_are_kept_by_the_slots_holder_alone() {
    let (_url, mut connection) = migrated_database().await;
    let (_project_id, slot) = running(&mut connection, "Launch").await;
    set_session(
        &mut connection,
        &slot,
        "4b1c4c84-36a4-4ad8-bd51-7a3c1d6e4f00",
    )
    .await
    .expect("kept");
    set_log(&mut connection, &slot, "Called Bash\n")
        .await
        .expect("kept");
    let stale = Slot {
        batch: slot.batch.clone(),
        token: Uuid::now_v7(),
    };
    set_log(&mut connection, &stale, "overwritten")
        .await
        .expect("runs");
    assert_eq!(
        log(&mut connection, slot.batch.id).await.expect("reads"),
        "Called Bash\n"
    );
    let found = find(&mut connection, slot.batch.id).await.expect("found");
    assert_eq!(
        found.worker_session_id.as_deref(),
        Some("4b1c4c84-36a4-4ad8-bd51-7a3c1d6e4f00")
    );
    assert_eq!(
        states(&mut connection, &[slot.batch.id])
            .await
            .expect("reads"),
        [(slot.batch.id, BatchState::Running)]
    );
}
