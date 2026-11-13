// Copyright © 2026 Jalapeno Labs

//! Assigning items to their Project Managers against Postgres: what may be assigned, the wake it
//! writes, how it ends, and where its plan stands.

use chrono::SubsecRound;
use serde_json::Value;

use super::*;
use crate::models::action_item::{ActionItemPriority, NewActionItem, Owner};
use crate::models::action_item_attribution;
use crate::models::action_item_event::list_for_item;
use crate::models::meeting::Reach;
use crate::models::project::{self, NewProject};
use crate::models::project_manager::Finished;
use crate::models::user::{PROJECT_MANAGER_USER_ID, SYSTEM_USER_ID};
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

/// Makes Project Manager turn `turn_id` have read a private meeting of the system user's, which
/// nobody shared with the test person.
async fn read_a_private_meeting(connection: &mut AsyncPgConnection, turn_id: Uuid) {
    let (meeting, _capture) = crate::models::meeting::tests::stored(connection).await;
    diesel::sql_query("UPDATE meetings SET created_by = $1, visibility = 'private' WHERE id = $2")
        .bind::<diesel::sql_types::Uuid, _>(SYSTEM_USER_ID)
        .bind::<diesel::sql_types::Uuid, _>(meeting.id)
        .execute(connection)
        .await
        .expect("made private");
    meeting_reads::keep(
        connection,
        meeting_reads::Turn::ProjectManager(turn_id),
        &[meeting.id],
    )
    .await
    .expect("kept");
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
        plan(&mut connection, &unassigned.record, Reader::ANY_MEMBER)
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

/// Taking an item back drops only the wake no turn took, so reassigning it while a turn holds the
/// first leaves that one taken and a second waiting. A turn that fails and gives its wakes back
/// must drop its copy rather than make the item wait twice, which the index refuses.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn reassigning_during_a_turn_that_fails_leaves_the_item_waiting_once() {
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
    let (turn, taken) = project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("the assignment woke it");
    assert_eq!(taken.len(), 1);

    unassign(&mut connection, item, TEST_PERSON, Utc::now())
        .await
        .expect("taken back");
    assign(
        &mut connection,
        item,
        Assigner::person(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("assigned again");

    let failed = Finished::Failed {
        error: "the runner stopped",
        retry: true,
    };
    project_manager::finish_turn(&mut connection, turn.id, failed, "")
        .await
        .expect("gives its wakes back without waking the item twice");
    assert_eq!(
        waiting_assignments(&mut connection, launch).await,
        [(Some(item), TEST_PERSON_ID)]
    );
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

    let waiting = plan(&mut connection, &assigned, Reader::ANY_MEMBER)
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
    let planning = plan(&mut connection, &assigned, Reader::ANY_MEMBER)
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
            serves: None,
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

    let planned = plan(&mut connection, &assigned, Reader::ANY_MEMBER)
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

    // Once the turn that proposed it read a meeting nobody shared with the person, the batch is
    // left out of the plan they read, and an admin, who reads every meeting, still reads it.
    read_a_private_meeting(&mut connection, turn.id).await;
    let member = Reader {
        id: TEST_PERSON_ID,
        reach: Reach::Public,
    };
    let held = plan(&mut connection, &assigned, member)
        .await
        .expect("reads")
        .expect("assigned");
    assert!(held.batches.is_empty(), "{:?}", held.batches);
    let admin = Reader {
        id: TEST_PERSON_ID,
        reach: Reach::Every,
    };
    let read = plan(&mut connection, &assigned, admin)
        .await
        .expect("reads")
        .expect("assigned");
    assert_eq!(read.batches.len(), 1);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn deleting_an_item_takes_it_from_its_project_manager() {
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

    let deleted = action_item::soft_delete(&mut connection, item, TEST_PERSON, Utc::now())
        .await
        .expect("deleted");
    assert_eq!(deleted.record.manager_assigned_at, None);
    assert_eq!(
        deleted
            .history
            .iter()
            .map(|entry| entry.kind.as_str())
            .collect::<Vec<_>>(),
        ["deleted", "manager_unassigned"]
    );
    assert!(
        waiting_assignments(&mut connection, launch)
            .await
            .is_empty()
    );

    let restored = action_item::restore(&mut connection, item, TEST_PERSON, Utc::now())
        .await
        .expect("restored");
    assert_eq!(
        plan(&mut connection, &restored.record, Reader::ANY_MEMBER)
            .await
            .expect("reads"),
        None,
        "nothing planned it, and it no longer says it is"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn finishing_an_item_ends_its_assignment_so_reopening_it_reads_unassigned() {
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

    // The watcher reads its issue closed before the Project Manager's turn runs.
    let watcher = Actor::Watcher(crate::models::action_item_link::LinkProvider::Jira);
    let resolved = action_item::transition(
        &mut connection,
        item,
        Transition::Resolve,
        watcher,
        Utc::now(),
    )
    .await
    .expect("resolved");
    assert_eq!(resolved.record.manager_assigned_at, None);
    let ended = resolved.history.last().expect("recorded");
    assert_eq!(ended.kind, "manager_unassigned");
    assert_eq!(ended.data, json!({ "projectId": launch, "finished": true }));
    assert!(
        waiting_assignments(&mut connection, launch)
            .await
            .is_empty()
    );

    let reopened = action_item::transition(
        &mut connection,
        item,
        Transition::Reopen,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("reopened");
    assert_eq!(
        plan(&mut connection, &reopened.record, Reader::ANY_MEMBER)
            .await
            .expect("reads"),
        None,
        "nothing planned it, and it no longer says it is"
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
    diesel::update(action_items::table.find(deleted))
        .set(action_items::deleted_at.eq(Utc::now()))
        .execute(&mut connection)
        .await
        .expect("deleted without ending the assignment");

    let resolved = item_in(&mut connection, vec![launch], ActionItemState::Open).await;
    assign(
        &mut connection,
        resolved,
        Assigner::person(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("assigned");
    action_item::transition(
        &mut connection,
        resolved,
        Transition::Resolve,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("resolved before the turn ran");

    let assigned: Vec<Uuid> = assigned_among(&mut connection, launch, &[kept, deleted, resolved])
        .await
        .expect("reads")
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert_eq!(assigned, [kept], "finished work is not planned");
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

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn only_the_company_members_assign_or_take_back() {
    let (_url, mut connection) = migrated_database().await;
    let guest = Uuid::now_v7();
    diesel::sql_query(
        "INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) \
         VALUES ($1, $2, 'guest@example.com', 'Guest', 'guest', 'active', now())",
    )
    .bind::<diesel::sql_types::Uuid, _>(guest)
    .bind::<diesel::sql_types::Uuid, _>(Uuid::now_v7())
    .execute(&mut connection)
    .await
    .expect("a guest");
    let launch = project_named(&mut connection, "Launch").await;
    let item = item_in(&mut connection, vec![launch], ActionItemState::Open).await;

    // A guest accepting Elysia's suggestion assigns nothing, as the routes refuse them.
    let suggested = Assigner {
        actor: Actor::Elysia,
        person: guest,
    };
    let refused = assign(&mut connection, item, suggested, Utc::now())
        .await
        .expect_err("a guest");
    assert!(matches!(refused, WorkError::Refused(_)));
    assert!(
        waiting_assignments(&mut connection, launch)
            .await
            .is_empty()
    );

    assign(
        &mut connection,
        item,
        Assigner::person(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("a member assigns");
    let refused = unassign(&mut connection, item, Actor::User(guest), Utc::now())
        .await
        .expect_err("a guest");
    assert!(matches!(refused, WorkError::Refused(_)));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn deleting_the_lead_project_ends_the_assignment_and_a_stray_one_is_taken_back() {
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

    // Deleting a project takes its items out first, which moves their lead and ends this.
    action_item::remove_all_from_project(&mut connection, launch, TEST_PERSON, Utc::now())
        .await
        .expect("taken out");
    let left = action_item::find(&mut connection, item)
        .await
        .expect("an item");
    assert_eq!(left.manager_assigned_at, None);
    assert!(
        kinds(&mut connection, item)
            .await
            .contains(&"manager_unassigned".to_owned())
    );

    // An assignment a stray delete left without a lead is still taken back.
    let website = project_named(&mut connection, "Website").await;
    let stray = item_in(&mut connection, vec![website], ActionItemState::Open).await;
    assign(
        &mut connection,
        stray,
        Assigner::person(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("assigned");
    diesel::update(action_items::table.find(stray))
        .set(action_items::lead_project_id.eq(None::<Uuid>))
        .execute(&mut connection)
        .await
        .expect("a stray lead");
    let taken_back = unassign(&mut connection, stray, TEST_PERSON, Utc::now())
        .await
        .expect("taken back");
    assert_eq!(taken_back.record.manager_assigned_at, None);
    assert_eq!(taken_back.history[0].data, json!({ "projectId": null }));
}
