// Copyright © 2026 Jalapeno Labs

//! A person's decision on a proposed batch against Postgres: approving it as proposed, editing
//! it first, and rejecting it, each in its history as the person; the Project Manager woken by a
//! rejection; and the one approval item per project counting what waits.

use super::*;
use crate::action_items::Transition;
use crate::database::schema::{project_manager_wakes, work_batch_approval_items};
use crate::models::action_item::{self, ActionItemState};
use crate::models::project::{self, NewProject};
use crate::models::project_manager::WakeKind;
use crate::models::user::{ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID};
use crate::models::work_batch::{self, Changes, NewWorkBatch, history};
use crate::test_support::{TEST_PERSON, TEST_PERSON_ID, migrated_database};

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

async fn proposed(connection: &mut AsyncPgConnection, project_id: Uuid, title: &str) -> Written {
    work_batch::submit(
        connection,
        NewWorkBatch {
            id: Uuid::now_v7(),
            project_id,
            title: title.to_owned(),
            goal: "Show annual pricing on the pricing page.".to_owned(),
            repositories: json!([{ "url": "https://github.com/JalapenoLabs/Elysium.git" }]),
            priority: Priority::Normal,
            scheduled_for: None,
            turn_id: None,
            source_record_ids: json!([]),
            action_item_ids: json!([]),
            source_message_ids: json!([]),
            lineage: crate::test_support::person_rooted(),
            created_by: PROJECT_MANAGER_USER_ID,
        },
    )
    .await
    .expect("submitted")
}

/// Each entry of batch `id`'s history: its kind, its data, and who wrote it.
async fn entries(connection: &mut AsyncPgConnection, id: Uuid) -> Vec<(String, Value, Uuid)> {
    history(connection, id)
        .await
        .expect("history")
        .into_iter()
        .map(|entry| (entry.kind, entry.data, entry.created_by))
        .collect()
}

/// The item `project_id`'s approval items row names now, if any.
async fn approval_item_of(connection: &mut AsyncPgConnection, project_id: Uuid) -> Option<Uuid> {
    work_batch_approval_items::table
        .find(project_id)
        .select(work_batch_approval_items::action_item_id)
        .first(connection)
        .await
        .optional()
        .expect("reads")
}

async fn batch_wakes(connection: &mut AsyncPgConnection, batch_id: Uuid) -> i64 {
    project_manager_wakes::table
        .filter(project_manager_wakes::work_batch_id.eq(batch_id))
        .filter(project_manager_wakes::kind.eq(WakeKind::Batch))
        .count()
        .get_result(connection)
        .await
        .expect("counts")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn one_item_per_project_counts_what_waits_and_resolves_when_none_is_left() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;

    let first = proposed(&mut connection, launch, "Pricing page").await;
    assert_eq!(first.batch.state, BatchState::Proposed);
    let raised = first
        .approval_item
        .expect("the first proposal raises the item")
        .record;
    assert_eq!(raised.title, "Approve a work batch for Launch");
    assert_eq!(raised.state, ActionItemState::Inbox);
    assert_eq!(raised.created_by, PROJECT_MANAGER_USER_ID);
    let memberships = action_item::memberships(&mut connection, &[raised.id])
        .await
        .expect("reads");
    assert_eq!(memberships.project_ids(raised.id), [launch]);

    let second = proposed(&mut connection, launch, "Docs").await;
    let updated = second.approval_item.expect("the count moved").record;
    assert_eq!(updated.id, raised.id, "the same item, not a second one");
    assert_eq!(updated.title, "Approve 2 work batches for Launch");

    // Another project has its own item.
    let elsewhere = project_named(&mut connection, "Elsewhere").await;
    let other = proposed(&mut connection, elsewhere, "Other").await;
    let other_item = other.approval_item.expect("its own item").record;
    assert_ne!(other_item.id, raised.id);

    let approved = approve(
        &mut connection,
        first.batch.id,
        Edits::default(),
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect("approves");
    assert_eq!(approved.batch.state, BatchState::Queued);
    let counted_down = approved.approval_item.expect("one fewer waits").record;
    assert_eq!(counted_down.title, "Approve a work batch for Launch");

    let rejected = reject(
        &mut connection,
        second.batch.id,
        "Not this quarter.",
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect("rejects");
    let resolved = rejected.approval_item.expect("none is left").record;
    assert_eq!(resolved.id, raised.id);
    assert_eq!(resolved.state, ActionItemState::Resolved);
    let standing = action_item::find(&mut connection, other_item.id)
        .await
        .expect("reads");
    assert_eq!(
        standing.state,
        ActionItemState::Inbox,
        "the other project's still waits"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_decision_is_the_persons_and_a_rejection_wakes_its_project_manager_with_why() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let first = proposed(&mut connection, launch, "Pricing page")
        .await
        .batch;
    let second = proposed(&mut connection, launch, "Docs").await.batch;

    approve(
        &mut connection,
        first.id,
        Edits::default(),
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect("approves");
    let kinds: Vec<(String, Uuid)> = entries(&mut connection, first.id)
        .await
        .into_iter()
        .map(|(kind, _data, by)| (kind, by))
        .collect();
    assert_eq!(
        kinds,
        [
            ("submitted".to_owned(), PROJECT_MANAGER_USER_ID),
            ("approved".to_owned(), TEST_PERSON_ID),
        ],
        "approving unchanged records no edit"
    );
    assert_eq!(
        batch_wakes(&mut connection, first.id).await,
        0,
        "approving wakes nothing"
    );

    let rejected = reject(
        &mut connection,
        second.id,
        "We are not touching the docs this quarter.",
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect("rejects")
    .batch;
    assert_eq!(rejected.state, BatchState::Rejected);
    assert_eq!(
        rejected.result.as_deref(),
        Some("We are not touching the docs this quarter.")
    );
    assert!(rejected.finished_at.is_some());
    assert!(rejected.assigned_at.is_none());
    assert_eq!(
        entries(&mut connection, second.id).await.last(),
        Some(&(
            "rejected".to_owned(),
            json!({ "reason": "We are not touching the docs this quarter." }),
            TEST_PERSON_ID
        )),
        "the reason is recorded as the person's feedback"
    );
    assert_eq!(
        batch_wakes(&mut connection, second.id).await,
        1,
        "the rejection wakes its Project Manager with the reason"
    );

    // Decided, neither can be decided again.
    let again = approve(
        &mut connection,
        second.id,
        Edits::default(),
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect_err("already rejected");
    assert!(matches!(again, ChangeError::Refused(_)));
    let twice = reject(&mut connection, first.id, "No.", TEST_PERSON_ID, Utc::now())
        .await
        .expect_err("already approved");
    assert!(matches!(twice, ChangeError::Refused(_)));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn edits_apply_before_approval_and_only_what_changed_is_recorded() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let batch = proposed(&mut connection, launch, "Pricing page")
        .await
        .batch;
    let due = Utc::now() + chrono::TimeDelta::days(2);

    let edits = Edits {
        title: Some("Annual pricing page".to_owned()),
        // The same goal it was proposed with changes nothing.
        goal: Some(batch.goal.clone()),
        repositories: Some(vec![Repository {
            url: "https://github.com/JalapenoLabs/Website.git".to_owned(),
            base_branch: Some("develop".to_owned()),
        }]),
        priority: Some((Priority::Scheduled, Some(due))),
    };
    let approved = approve(&mut connection, batch.id, edits, TEST_PERSON_ID, Utc::now())
        .await
        .expect("approves")
        .batch;
    assert_eq!(approved.state, BatchState::Queued);
    assert_eq!(approved.title, "Annual pricing page");
    assert_eq!(approved.goal, batch.goal);
    assert_eq!(approved.priority, Priority::Scheduled);
    assert_eq!(
        approved.scheduled_for.map(|at| at.timestamp()),
        Some(due.timestamp())
    );
    assert_eq!(
        approved.repositories()[0].base_branch.as_deref(),
        Some("develop")
    );

    let recorded = entries(&mut connection, batch.id).await;
    let kinds: Vec<&str> = recorded.iter().map(|(kind, _, _)| kind.as_str()).collect();
    assert_eq!(kinds, ["submitted", "edited", "approved"]);
    let (_, edited, by) = &recorded[1];
    assert_eq!(*by, TEST_PERSON_ID);
    let mut changed: Vec<&String> = edited["after"].as_object().expect("after").keys().collect();
    changed.sort();
    assert_eq!(
        changed,
        ["priority", "repositories", "scheduledFor", "title"]
    );
    assert_eq!(edited["before"]["title"], "Pricing page");
    assert_eq!(edited["before"]["priority"], "normal");
    assert_eq!(edited["after"]["repositories"][0]["baseBranch"], "develop");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn nothing_but_a_decision_moves_a_proposal_and_withdrawing_one_counts_down() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let batch = proposed(&mut connection, launch, "Pricing page")
        .await
        .batch;

    // Elysia, the Project Manager, and a person alike cannot resume, pause, run next, or
    // reprioritize a proposal: resuming it would queue what nobody approved.
    for (changes, by) in [
        (
            Changes {
                paused: Some(false),
                ..Changes::default()
            },
            ELYSIA_USER_ID,
        ),
        (
            Changes {
                paused: Some(true),
                ..Changes::default()
            },
            PROJECT_MANAGER_USER_ID,
        ),
        (
            Changes {
                run_next: Some(true),
                ..Changes::default()
            },
            TEST_PERSON_ID,
        ),
        (
            Changes {
                priority: Some((Priority::Asap, None)),
                ..Changes::default()
            },
            ELYSIA_USER_ID,
        ),
    ] {
        let refused = work_batch::change(&mut connection, batch.id, changes, by, Utc::now())
            .await
            .expect_err("a proposal moves only by a decision");
        assert!(matches!(refused, ChangeError::Refused(_)), "{changes:?}");
    }
    let still = work_batch::find(&mut connection, batch.id)
        .await
        .expect("reads");
    assert_eq!(still.state, BatchState::Proposed);
    assert_eq!(still.priority, Priority::Normal);

    // Its Project Manager withdraws it, and the item it raised resolves with nothing left.
    let item = approval_item_of(&mut connection, launch)
        .await
        .expect("an item");
    let withdrawn = work_batch::cancel(
        &mut connection,
        batch.id,
        Some(launch),
        "Superseded by a smaller batch.",
        PROJECT_MANAGER_USER_ID,
    )
    .await
    .expect("withdrawn");
    assert_eq!(withdrawn.batch.state, BatchState::Cancelled);
    let resolved = withdrawn.approval_item.expect("none is left").record;
    assert_eq!(resolved.id, item);
    assert_eq!(resolved.state, ActionItemState::Resolved);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_item_its_person_closed_stays_closed_until_the_next_proposal() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let first = proposed(&mut connection, launch, "Pricing page").await;
    let second = proposed(&mut connection, launch, "Docs").await;
    let item = first.approval_item.expect("raised").record.id;
    action_item::transition(
        &mut connection,
        item,
        Transition::Dismiss,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("dismissed by hand");

    // Deciding what remains leaves the dismissed item alone, and raises nothing.
    let approved = approve(
        &mut connection,
        first.batch.id,
        Edits::default(),
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect("approves");
    assert!(approved.approval_item.is_none());
    let dismissed = action_item::find(&mut connection, item)
        .await
        .expect("reads");
    assert_eq!(dismissed.state, ActionItemState::Dismissed);
    assert_eq!(dismissed.title, "Approve 2 work batches for Launch");

    // A new proposal raises a new item, counting both that wait.
    let third = proposed(&mut connection, launch, "Changelog").await;
    let raised = third.approval_item.expect("a new item").record;
    assert_ne!(raised.id, item);
    assert_eq!(raised.title, "Approve 2 work batches for Launch");
    assert_eq!(
        approval_item_of(&mut connection, launch).await,
        Some(raised.id)
    );
    assert_eq!(second.batch.state, BatchState::Proposed);
}

/// A batch proposed without a submission, as the migration that began proposals left every batch
/// waiting for a slot: no history entry of a submission, and no approval item.
async fn held_for_approval(connection: &mut AsyncPgConnection, project_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    diesel::insert_into(work_batches::table)
        .values((
            NewWorkBatch {
                id,
                project_id,
                title: "Waiting since before proposals".to_owned(),
                goal: "Do the work.".to_owned(),
                repositories: json!([]),
                priority: Priority::Normal,
                scheduled_for: None,
                turn_id: None,
                source_record_ids: json!([]),
                action_item_ids: json!([]),
                source_message_ids: json!([]),
                lineage: crate::test_support::person_rooted(),
                created_by: PROJECT_MANAGER_USER_ID,
            },
            work_batches::state.eq(BatchState::Proposed),
        ))
        .execute(connection)
        .await
        .expect("held");
    id
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn proposals_that_never_raised_an_item_raise_one_once() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    held_for_approval(&mut connection, launch).await;
    held_for_approval(&mut connection, launch).await;
    // A project whose person closed its item by hand is left alone.
    let website = project_named(&mut connection, "Website").await;
    let told = proposed(&mut connection, website, "Footer").await;
    let closed = told.approval_item.expect("raised").record.id;
    action_item::transition(
        &mut connection,
        closed,
        Transition::Dismiss,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("dismissed by hand");

    let raised = approval_item::reconcile(&mut connection, Utc::now())
        .await
        .expect("reconciles");
    assert_eq!(raised.len(), 1, "only the project never told");
    let item = &raised[0].record;
    assert_eq!(item.title, "Approve 2 work batches for Launch");
    assert_eq!(item.state, ActionItemState::Inbox);
    assert_eq!(
        approval_item_of(&mut connection, launch).await,
        Some(item.id)
    );
    assert_eq!(
        approval_item_of(&mut connection, website).await,
        Some(closed)
    );

    let again = approval_item::reconcile(&mut connection, Utc::now())
        .await
        .expect("reconciles");
    assert!(again.is_empty(), "an item is raised once");
}
