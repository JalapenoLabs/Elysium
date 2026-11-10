// Copyright © 2026 Jalapeno Labs

//! Assigning items to their Project Managers against Postgres: what may be assigned, the wake it
//! writes, how it ends, and where its plan stands.

use chrono::SubsecRound;
use serde_json::Value;

use super::*;
use crate::models::action_item::{ActionItemPriority, NewActionItem, Owner};
use crate::models::action_item_attribution;
use crate::models::action_item_event::list_for_item;
use crate::models::project::{self, NewProject};
use crate::models::project_manager::Finished;
use crate::models::user::PROJECT_MANAGER_USER_ID;
use crate::models::work_batch::{self, NewWorkBatch, Priority};
use crate::test_support::{TEST_PERSON, TEST_PERSON_ID, migrated_database, person_rooted};

#[test]
fn a_plan_stands_where_its_assignments_wake_does() {
    assert_eq!(stage(Some(WakeTaken::Waiting)), PlanStage::Waiting);
    assert_eq!(
        stage(Some(WakeTaken::By(TurnStatus::Running))),
        PlanStage::Planning
    );
    assert_eq!(
        stage(Some(WakeTaken::By(TurnStatus::Succeeded))),
        PlanStage::Planned
    );
    assert_eq!(
        stage(Some(WakeTaken::By(TurnStatus::Failed))),
        PlanStage::Failed
    );
    assert_eq!(stage(None), PlanStage::Planned, "nothing waits for it");
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

async fn item_in(
    connection: &mut AsyncPgConnection,
    project_ids: Vec<Uuid>,
    state: ActionItemState,
) -> Uuid {
    action_item::create(
        connection,
        NewActionItem {
            title: "Annual pricing".to_owned(),
            notes: "Ada asked for annual pricing.".to_owned(),
            state,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids,
            initiative_ids: Vec::new(),
        },
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("created")
    .record
    .id
}

/// The assignment wakes waiting for `project_id`'s next turn, by the item each is for and who
/// wrote it.
async fn waiting_assignments(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> Vec<(Option<Uuid>, Uuid)> {
    project_manager_wakes::table
        .filter(project_manager_wakes::project_id.eq(project_id))
        .filter(project_manager_wakes::kind.eq(WakeKind::Assignment))
        .filter(project_manager_wakes::turn_id.is_null())
        .select((
            project_manager_wakes::action_item_id,
            project_manager_wakes::created_by,
        ))
        .load(connection)
        .await
        .expect("wakes")
}

async fn kinds(connection: &mut AsyncPgConnection, id: Uuid) -> Vec<String> {
    list_for_item(connection, id)
        .await
        .expect("history")
        .into_iter()
        .map(|entry| entry.kind)
        .collect()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn assigning_accepts_the_item_and_wakes_its_lead_projects_manager_once() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;
    let item = item_in(
        &mut connection,
        vec![launch, website],
        ActionItemState::Inbox,
    )
    .await;
    // Postgres keeps microseconds, so the moment compared with what it stores is one it can hold.
    let now = Utc::now().trunc_subsecs(6);

    let assigned = assign(&mut connection, item, Assigner::person(TEST_PERSON_ID), now)
        .await
        .expect("assigned");
    assert_eq!(assigned.record.state, ActionItemState::Open, "taken on");
    assert_eq!(assigned.record.manager_assigned_at, Some(now));
    assert_eq!(assigned.record.manager_assigned_by, Some(TEST_PERSON_ID));
    let recorded: Vec<(&str, &Value)> = assigned
        .history
        .iter()
        .map(|entry| (entry.kind.as_str(), &entry.data))
        .collect();
    assert_eq!(
        recorded,
        [
            ("state_changed", &json!({ "from": "inbox", "to": "open" })),
            ("manager_assigned", &json!({ "projectId": launch })),
        ]
    );
    assert_eq!(
        waiting_assignments(&mut connection, launch).await,
        [(Some(item), TEST_PERSON_ID)],
        "the lead's Project Manager, woken by the person"
    );
    assert!(
        waiting_assignments(&mut connection, website)
            .await
            .is_empty()
    );

    let again = assign(&mut connection, item, Assigner::person(TEST_PERSON_ID), now)
        .await
        .expect("assigning twice is no fault");
    assert!(again.history.is_empty(), "already assigned changes nothing");
    assert_eq!(waiting_assignments(&mut connection, launch).await.len(), 1);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_item_no_project_leads_or_already_finished_cannot_be_assigned() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let loose = item_in(&mut connection, Vec::new(), ActionItemState::Open).await;
    let refused = assign(
        &mut connection,
        loose,
        Assigner::person(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect_err("no lead");
    assert!(
        matches!(refused, WorkError::Conflict(message) if message.contains("no project leads")),
        "the refusal says why"
    );

    let finished = item_in(&mut connection, vec![launch], ActionItemState::Open).await;
    action_item::transition(
        &mut connection,
        finished,
        Transition::Resolve,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("resolved");
    let refused = assign(
        &mut connection,
        finished,
        Assigner::person(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect_err("resolved");
    assert!(matches!(refused, WorkError::Conflict(_)));
    assert!(
        waiting_assignments(&mut connection, launch)
            .await
            .is_empty()
    );
    assert_eq!(kinds(&mut connection, loose).await, ["created"]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn taking_an_item_back_drops_the_wake_no_turn_took() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let item = item_in(&mut connection, vec![launch], ActionItemState::Open).await;
    assign(
        &mut connection,
        item,
        Assigner::person(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("assigned");

    let unassigned = unassign(&mut connection, item, TEST_PERSON, Utc::now())
        .await
        .expect("unassigned");
    assert_eq!(unassigned.record.manager_assigned_at, None);
    assert_eq!(unassigned.record.manager_assigned_by, None);
    assert_eq!(unassigned.history.len(), 1);
    assert_eq!(unassigned.history[0].kind, "manager_unassigned");
    assert_eq!(unassigned.history[0].data, json!({ "projectId": launch }));
    assert!(
        waiting_assignments(&mut connection, launch)
            .await
            .is_empty()
    );
    assert_eq!(
        plan(&mut connection, &unassigned.record)
            .await
            .expect("reads"),
        None
    );

    let again = unassign(&mut connection, item, TEST_PERSON, Utc::now())
        .await
        .expect("unassigning twice is no fault");
    assert!(again.history.is_empty());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn moving_the_lead_ends_the_assignment_nobody_gave_the_new_lead() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;
    let item = item_in(
        &mut connection,
        vec![launch, website],
        ActionItemState::Open,
    )
    .await;
    assign(
        &mut connection,
        item,
        Assigner::person(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("assigned");

    let moved =
        action_item_attribution::set_lead(&mut connection, item, website, TEST_PERSON, Utc::now())
            .await
            .expect("the lead moves");
    assert_eq!(moved.record.lead_project_id, Some(website));
    assert_eq!(moved.record.manager_assigned_at, None);
    let ended = moved
        .history
        .iter()
        .find(|entry| entry.kind == "manager_unassigned")
        .expect("the assignment ended");
    assert_eq!(
        ended.data,
        json!({ "projectId": launch, "leadMoved": true })
    );
    assert!(
        waiting_assignments(&mut connection, launch)
            .await
            .is_empty()
    );
    assert!(
        waiting_assignments(&mut connection, website)
            .await
            .is_empty(),
        "nobody asked the new lead's Project Manager to plan it"
    );

    // An item taken out of its last project loses its lead, and its assignment with it.
    assign(
        &mut connection,
        item,
        Assigner::person(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("assigned to the new lead");
    action_item::remove_project(&mut connection, item, launch, TEST_PERSON, Utc::now())
        .await
        .expect("leaves");
    let left = action_item::remove_project(&mut connection, item, website, TEST_PERSON, Utc::now())
        .await
        .expect("leaves its last project");
    assert_eq!(left.record.lead_project_id, None);
    assert_eq!(left.record.manager_assigned_at, None);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_plan_follows_its_wake_through_a_turn_to_the_batches_naming_the_item() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let item = item_in(&mut connection, vec![launch], ActionItemState::Open).await;
    let assigned = assign(
        &mut connection,
        item,
        Assigner::person(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("assigned")
    .record;

    let waiting = plan(&mut connection, &assigned)
        .await
        .expect("reads")
        .expect("assigned");
    assert_eq!(waiting.project_id, launch);
    assert_eq!(waiting.assigned_by, TEST_PERSON_ID);
    assert_eq!(waiting.stage, PlanStage::Waiting);
    assert_eq!(waiting.turn_id, None);
    assert!(waiting.batches.is_empty());

    let (turn, taken) = project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("the assignment woke it");
    assert_eq!(taken.len(), 1);
    let planning = plan(&mut connection, &assigned)
        .await
        .expect("reads")
        .expect("assigned");
    assert_eq!(planning.stage, PlanStage::Planning);
    assert_eq!(planning.turn_id, Some(turn.id));

    let batch = work_batch::submit(
        &mut connection,
        NewWorkBatch {
            id: Uuid::now_v7(),
            project_id: launch,
            title: "Annual pricing".to_owned(),
            goal: "Show annual pricing.".to_owned(),
            repositories: json!([]),
            priority: Priority::Normal,
            scheduled_for: None,
            turn_id: Some(turn.id),
            source_record_ids: json!([]),
            action_item_ids: json!([item]),
            source_message_ids: json!([]),
            lineage: person_rooted(),
            created_by: PROJECT_MANAGER_USER_ID,
        },
    )
    .await
    .expect("submitted")
    .batch;
    let done = Finished::Succeeded {
        summary: None,
        reply: None,
        timeline_record_id: None,
    };
    project_manager::finish_turn(&mut connection, turn.id, done, "")
        .await
        .expect("finished");

    let planned = plan(&mut connection, &assigned)
        .await
        .expect("reads")
        .expect("assigned");
    assert_eq!(planned.stage, PlanStage::Planned);
    assert_eq!(
        planned
            .batches
            .iter()
            .map(|listed| (listed.id, listed.state))
            .collect::<Vec<_>>(),
        [(batch.id, BatchState::Proposed)],
        "the batch waits for a person, and no worker started"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_turn_plans_only_what_is_still_assigned_to_its_project() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let kept = item_in(&mut connection, vec![launch], ActionItemState::Open).await;
    let deleted = item_in(&mut connection, vec![launch], ActionItemState::Open).await;
    for item in [kept, deleted] {
        assign(
            &mut connection,
            item,
            Assigner::person(TEST_PERSON_ID),
            Utc::now(),
        )
        .await
        .expect("assigned");
    }
    action_item::soft_delete(&mut connection, deleted, TEST_PERSON, Utc::now())
        .await
        .expect("deleted");

    let assigned: Vec<Uuid> = assigned_among(&mut connection, launch, &[kept, deleted])
        .await
        .expect("reads")
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert_eq!(assigned, [kept]);
    let elsewhere = project_named(&mut connection, "Website").await;
    assert!(
        assigned_among(&mut connection, elsewhere, &[kept])
            .await
            .expect("reads")
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_database_holds_an_assignment_whole() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let item = item_in(&mut connection, vec![launch], ActionItemState::Open).await;
    let refused = diesel::update(action_items::table.find(item))
        .set(action_items::manager_assigned_at.eq(Utc::now()))
        .execute(&mut connection)
        .await;
    assert!(refused.is_err(), "an assignment names who assigned it");
}
