// Copyright © 2026 Jalapeno Labs

//! The dial's rules, and the dial end to end: who may turn it, what each turn records, and what a
//! submitted batch does at each level.
//!
//! The database tests are `#[ignore]`d so `cargo test` stays hermetic;
//! `api/scripts/verify-migrations.sh` runs them.

use chrono::Utc;
use diesel::{OptionalExtension, QueryDsl};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::json;
use uuid::Uuid;

use super::{Autonomy, latest_change, set};
use crate::database::schema::work_batch_approval_items;
use crate::models::project::{self, NewProject, Project};
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::lineage::Origin;
use crate::models::work_batch::lineage::{Lineage, Root, decide};
use crate::models::work_batch::{self, BatchState, NewWorkBatch, Priority};
use crate::test_support::{TEST_PERSON_ID, migrated_database, person_rooted, test_member};

const LEVELS: [Autonomy; 3] = [
    Autonomy::ApproveEveryBatch,
    Autonomy::ApproveFoundWork,
    Autonomy::FullAutonomy,
];

#[test]
fn found_work_never_queues_itself_at_any_level() {
    for level in LEVELS {
        assert!(!level.queues(Origin::Found), "{level:?}");
    }
}

#[test]
fn work_a_person_asked_for_runs_once_the_project_trusts_it() {
    assert!(!Autonomy::ApproveEveryBatch.queues(Origin::HumanRooted));
    assert!(Autonomy::ApproveFoundWork.queues(Origin::HumanRooted));
    assert!(Autonomy::FullAutonomy.queues(Origin::HumanRooted));
}

#[test]
fn only_full_autonomy_writes_to_trackers_at_once() {
    let writing: Vec<Autonomy> = LEVELS
        .into_iter()
        .filter(|level| level.writes_directly())
        .collect();
    assert_eq!(writing, [Autonomy::FullAutonomy]);
}

#[test]
fn a_project_starts_at_approve_every_batch() {
    assert_eq!(Autonomy::default(), Autonomy::ApproveEveryBatch);
}

#[test]
fn levels_travel_as_their_database_names() {
    assert_eq!(
        serde_json::to_value(LEVELS).expect("serializes"),
        json!(["approve_every_batch", "approve_found_work", "full_autonomy"])
    );
    let parsed: Autonomy = serde_json::from_value(json!("approve_found_work")).expect("parses");
    assert_eq!(parsed, Autonomy::ApproveFoundWork);
    serde_json::from_value::<Autonomy>(json!("autonomous")).expect_err("no such level");
}

async fn new_project(connection: &mut AsyncPgConnection) -> Project {
    project::create(
        connection,
        &NewProject {
            name: "Launch".to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
}

fn batch_for(project_id: Uuid, lineage: Lineage) -> NewWorkBatch {
    NewWorkBatch {
        id: Uuid::now_v7(),
        project_id,
        title: "Annual pricing".to_owned(),
        goal: "Show annual pricing on the pricing page.".to_owned(),
        serves: None,
        repositories: json!([]),
        priority: Priority::Normal,
        scheduled_for: None,
        turn_id: None,
        source_record_ids: json!([]),
        action_item_ids: json!([]),
        source_message_ids: json!([]),
        lineage,
        created_by: PROJECT_MANAGER_USER_ID,
    }
}

fn found() -> Lineage {
    decide(&[Root::Agent], &[]).expect("an agent's notice is a root")
}

async fn has_approval_item(connection: &mut AsyncPgConnection, project_id: Uuid) -> bool {
    work_batch_approval_items::table
        .find(project_id)
        .select(work_batch_approval_items::project_id)
        .first::<Uuid>(connection)
        .await
        .optional()
        .expect("read")
        .is_some()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn turning_the_dial_records_who_and_when_and_choosing_the_same_level_records_nothing() {
    let (_url, mut connection) = migrated_database().await;
    let created = new_project(&mut connection).await;
    assert_eq!(created.autonomy, Autonomy::ApproveEveryBatch);
    assert_eq!(
        latest_change(&mut connection, created.id)
            .await
            .expect("read"),
        None,
        "a new project is at the default nobody set"
    );

    let person = test_member(&mut connection).await;
    let before = Utc::now();
    let turned = set(&mut connection, created.id, Autonomy::FullAutonomy, &person)
        .await
        .expect("set");
    assert_eq!(turned.autonomy, Autonomy::FullAutonomy);
    let change = latest_change(&mut connection, created.id)
        .await
        .expect("read")
        .expect("recorded");
    assert_eq!(change.previous_autonomy, Autonomy::ApproveEveryBatch);
    assert_eq!(change.autonomy, Autonomy::FullAutonomy);
    assert_eq!(change.created_by, TEST_PERSON_ID);
    assert!(change.created_at >= before - chrono::TimeDelta::seconds(5));

    let again = set(&mut connection, created.id, Autonomy::FullAutonomy, &person)
        .await
        .expect("set");
    assert_eq!(again.autonomy, Autonomy::FullAutonomy);
    assert_eq!(
        latest_change(&mut connection, created.id)
            .await
            .expect("read"),
        Some(change.clone()),
        "the same level changes nothing"
    );

    set(
        &mut connection,
        created.id,
        Autonomy::ApproveFoundWork,
        &person,
    )
    .await
    .expect("set");
    let newest = latest_change(&mut connection, created.id)
        .await
        .expect("read")
        .expect("recorded");
    assert_eq!(newest.previous_autonomy, Autonomy::FullAutonomy);
    assert_eq!(newest.autonomy, Autonomy::ApproveFoundWork);
    assert_ne!(newest.id, change.id);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn at_the_default_every_batch_waits_for_a_person() {
    let (_url, mut connection) = migrated_database().await;
    let project_id = new_project(&mut connection).await.id;

    let written = work_batch::submit(&mut connection, batch_for(project_id, person_rooted()))
        .await
        .expect("submitted");
    assert_eq!(written.batch.state, BatchState::Proposed);
    assert!(written.approval_item.is_some(), "the person is asked");
    let history = work_batch::history(&mut connection, written.batch.id)
        .await
        .expect("read");
    assert_eq!(history[0].kind, "submitted");
    assert_eq!(history[0].data, json!({}));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn approving_found_work_only_queues_what_a_person_asked_for_and_says_why() {
    let (_url, mut connection) = migrated_database().await;
    let project_id = new_project(&mut connection).await.id;
    let person = test_member(&mut connection).await;
    set(
        &mut connection,
        project_id,
        Autonomy::ApproveFoundWork,
        &person,
    )
    .await
    .expect("set");

    let asked = work_batch::submit(&mut connection, batch_for(project_id, person_rooted()))
        .await
        .expect("submitted");
    assert_eq!(asked.batch.state, BatchState::Queued);
    assert!(asked.approval_item.is_none());
    assert!(
        !has_approval_item(&mut connection, project_id).await,
        "nothing waits, so nobody is asked"
    );
    let history = work_batch::history(&mut connection, asked.batch.id)
        .await
        .expect("read");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, "submitted");
    assert_eq!(history[0].data, json!({ "autonomy": "approve_found_work" }));

    // The pool hands it a slot like any approved batch.
    let slots = work_batch::assign(&mut connection, &[asked.batch.id], 1, Utc::now())
        .await
        .expect("assigned");
    assert_eq!(slots.len(), 1);

    let noticed = work_batch::submit(&mut connection, batch_for(project_id, found()))
        .await
        .expect("submitted");
    assert_eq!(noticed.batch.state, BatchState::Proposed);
    assert!(
        noticed.approval_item.is_some(),
        "found work waits for the person"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn full_autonomy_still_proposes_found_work_and_the_pool_hands_it_nothing() {
    let (_url, mut connection) = migrated_database().await;
    let project_id = new_project(&mut connection).await.id;
    let person = test_member(&mut connection).await;
    set(&mut connection, project_id, Autonomy::FullAutonomy, &person)
        .await
        .expect("set");

    let noticed = work_batch::submit(&mut connection, batch_for(project_id, found()))
        .await
        .expect("submitted");
    assert_eq!(noticed.batch.state, BatchState::Proposed);
    assert!(noticed.approval_item.is_some());
    let slots = work_batch::assign(&mut connection, &[noticed.batch.id], 4, Utc::now())
        .await
        .expect("looked");
    assert!(slots.is_empty(), "a proposed batch is never given a slot");

    let asked = work_batch::submit(&mut connection, batch_for(project_id, person_rooted()))
        .await
        .expect("submitted");
    assert_eq!(asked.batch.state, BatchState::Queued);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn turning_the_dial_moves_no_batch_already_waiting() {
    let (_url, mut connection) = migrated_database().await;
    let project_id = new_project(&mut connection).await.id;
    let proposed = work_batch::submit(&mut connection, batch_for(project_id, person_rooted()))
        .await
        .expect("submitted");

    let person = test_member(&mut connection).await;
    set(&mut connection, project_id, Autonomy::FullAutonomy, &person)
        .await
        .expect("set");
    let still = work_batch::find(&mut connection, proposed.batch.id)
        .await
        .expect("read");
    assert_eq!(
        still.state,
        BatchState::Proposed,
        "what was submitted under the old level is still the person's to decide"
    );
}
