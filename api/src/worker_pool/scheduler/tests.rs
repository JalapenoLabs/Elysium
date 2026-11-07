// Copyright © 2026 Jalapeno Labs

//! The scheduler's looks against Postgres: slots handed out in order and within the size, none
//! while every Claude account is at its usage limit, none ever to a proposed batch, slots taken
//! back when the pool shrinks, and slots taken over by the next engine.

use chrono::TimeDelta;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::SecretString;
use serde_json::json;

use super::*;
use crate::database::schema::work_batches;
use crate::models::claude_account::{
    self, Identity, NewCredential, SignedIn, Tokens, UsageWindow, UsageWindows,
};
use crate::models::project::{self, NewProject};
use crate::models::user::{self, PROJECT_MANAGER_USER_ID, WorkspaceSettingsChanges};
use crate::models::work_batch::{NewWorkBatch, Priority, WorkBatch};
use crate::test_support::{
    TEST_PERSON_ID, approved_batch, cipher, fetched_agent_configuration, migrated_database,
};

async fn scheduler(url: &SecretString, cpu_count: i32) -> Scheduler {
    Scheduler {
        database: crate::connections::connect_postgres(url, 4)
            .await
            .expect("Postgres accepts connections"),
        events: EventBus::in_process(),
        cpu_count,
        held: HashMap::new(),
        size: None,
    }
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

/// A batch its Project Manager proposed, which no person has approved yet.
async fn proposed(connection: &mut AsyncPgConnection, project_id: Uuid) -> WorkBatch {
    work_batch::submit(connection, new_batch(project_id))
        .await
        .expect("submitted")
        .batch
}

/// A batch a person approved, waiting for a slot.
async fn submitted(connection: &mut AsyncPgConnection, project_id: Uuid) -> WorkBatch {
    approved_batch(connection, new_batch(project_id)).await
}

fn new_batch(project_id: Uuid) -> NewWorkBatch {
    NewWorkBatch {
        id: Uuid::now_v7(),
        project_id,
        title: "Work".to_owned(),
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
    }
}

async fn size_pool(connection: &mut AsyncPgConnection, size: Option<i32>) {
    user::update_workspace_settings(
        connection,
        WorkspaceSettingsChanges {
            worker_pool_size: Some(size),
            ..WorkspaceSettingsChanges::default()
        },
        TEST_PERSON_ID,
    )
    .await
    .expect("sized");
}

async fn in_slots(connection: &mut AsyncPgConnection) -> Vec<Uuid> {
    work_batch::in_slots(connection)
        .await
        .expect("reads")
        .into_iter()
        .map(|batch| batch.id)
        .collect()
}

/// A Claude account named `name` whose five-hour window is at `utilization`, resetting in two hours.
async fn claude_account_at(connection: &mut AsyncPgConnection, name: &str, utilization: f64) {
    let saved = claude_account::save_sign_in(
        connection,
        &cipher(),
        NewCredential {
            name,
            priority: 0,
            created_by: TEST_PERSON_ID,
        },
        &SignedIn {
            tokens: Tokens {
                access_token: SecretString::from("access"),
                access_token_expires_at: Utc::now() + TimeDelta::hours(6),
                refresh_token: SecretString::from("refresh"),
                refresh_token_expires_at: None,
            },
            identity: Identity {
                account_uuid: Uuid::now_v7(),
                email: format!("{name}@example.com"),
                organization_uuid: None,
                organization_name: None,
            },
            subscription_type: None,
        },
    )
    .await
    .expect("signed in");
    claude_account::record_usage(
        connection,
        saved.llm.id,
        &UsageWindows {
            five_hour: Some(UsageWindow {
                utilization,
                resets_at: Some(Utc::now() + TimeDelta::hours(2)),
            }),
            seven_day: None,
        },
        Utc::now(),
    )
    .await
    .expect("usage stored");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn free_slots_go_round_robin_and_the_pool_follows_its_size() {
    let (url, mut connection) = migrated_database().await;
    fetched_agent_configuration(&mut connection).await;
    let busy = project_named(&mut connection, "Busy").await;
    let quiet = project_named(&mut connection, "Quiet").await;
    let busy_first = submitted(&mut connection, busy).await;
    let busy_second = submitted(&mut connection, busy).await;
    let quiet_first = submitted(&mut connection, quiet).await;
    size_pool(&mut connection, Some(2)).await;
    let mut pool = scheduler(&url, 8).await;

    pool.look().await.expect("looks");
    assert_eq!(
        in_slots(&mut connection).await,
        [busy_first.id, quiet_first.id],
        "one each, before the busy project's second"
    );
    let seen = worker_pool::find(&mut connection)
        .await
        .expect("reads")
        .expect("recorded");
    assert_eq!(seen.cpu_count, 8);

    // The pool shrinks: the slot handed out last goes back, and the queue keeps its order.
    size_pool(&mut connection, Some(1)).await;
    pool.look().await.expect("looks");
    assert_eq!(in_slots(&mut connection).await, [busy_first.id]);

    // Following the default again, eight cores leave six slots.
    size_pool(&mut connection, None).await;
    pool.look().await.expect("looks");
    let mut held = in_slots(&mut connection).await;
    held.sort();
    let mut every = vec![busy_first.id, busy_second.id, quiet_first.id];
    every.sort();
    assert_eq!(held, every);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn no_slot_is_handed_out_without_the_configuration_repository_and_one_item_says_why() {
    let (url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let first = submitted(&mut connection, launch).await;
    let second = submitted(&mut connection, launch).await;
    let mut pool = scheduler(&url, 8).await;

    pool.look().await.expect("looks");
    pool.look().await.expect("looks again");
    assert!(
        in_slots(&mut connection).await.is_empty(),
        "no batch holds a slot it could not use"
    );
    let raised = crate::models::agent_configuration::find(&mut connection)
        .await
        .expect("reads")
        .inbox_item_id
        .expect("an item says why");
    // Approving each batch raises items of its own; only the hold's are counted.
    let items: i64 = crate::database::schema::action_items::table
        .filter(
            crate::database::schema::action_items::title
                .eq("Choose a configuration repository for agents"),
        )
        .count()
        .get_result(&mut connection)
        .await
        .expect("counted");
    assert_eq!(items, 1, "one item for both batches and every look");

    fetched_agent_configuration(&mut connection).await;
    pool.look().await.expect("looks");
    let mut held = in_slots(&mut connection).await;
    held.sort_unstable();
    assert_eq!(held, [first.id, second.id]);
    let item = crate::models::action_item::find(&mut connection, raised)
        .await
        .expect("the item");
    assert_eq!(item.title, "Choose a configuration repository for agents");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn no_slot_is_handed_out_while_every_claude_account_is_spent() {
    let (url, mut connection) = migrated_database().await;
    fetched_agent_configuration(&mut connection).await;
    let launch = project_named(&mut connection, "Launch").await;
    let batch = submitted(&mut connection, launch).await;
    claude_account_at(&mut connection, "spent", 100.0).await;
    let mut pool = scheduler(&url, 8).await;

    let next = pool.look().await.expect("looks");
    assert!(in_slots(&mut connection).await.is_empty());
    let held_until = worker_pool::find(&mut connection)
        .await
        .expect("reads")
        .and_then(|seen| seen.usage_held_until)
        .expect("the hold is recorded for the queue's page");
    assert_eq!(next, Some(held_until), "it looks again at the reset");

    // Another account with room left lifts the hold.
    claude_account_at(&mut connection, "fresh", 10.0).await;
    pool.look().await.expect("looks");
    assert_eq!(in_slots(&mut connection).await, [batch.id]);
    assert!(
        worker_pool::find(&mut connection)
            .await
            .expect("reads")
            .and_then(|seen| seen.usage_held_until)
            .is_none()
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_stopping_engine_leaves_its_slots_for_the_next_to_take_at_once() {
    let (url, mut connection) = migrated_database().await;
    fetched_agent_configuration(&mut connection).await;
    let launch = project_named(&mut connection, "Launch").await;
    let batch = submitted(&mut connection, launch).await;
    let mut stopping = scheduler(&url, 4).await;
    stopping.look().await.expect("looks");
    let stopped_token = stopping.held[&batch.id];

    stopping.release().await;
    let mut next = scheduler(&url, 4).await;
    next.look().await.expect("looks");
    let adopted_token = next.held.get(&batch.id).expect("taken over at once");
    assert_ne!(*adopted_token, stopped_token);
    assert_eq!(
        in_slots(&mut connection).await,
        [batch.id],
        "the slot moved; no second was handed out"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_proposed_batch_never_gets_a_slot_through_restarts_and_lapsed_leases() {
    let (url, mut connection) = migrated_database().await;
    fetched_agent_configuration(&mut connection).await;
    let launch = project_named(&mut connection, "Launch").await;
    // Proposed first, with a pool of four slots and one other batch: nothing stands between it
    // and a slot but the gate.
    let waiting = proposed(&mut connection, launch).await;
    let approved = submitted(&mut connection, launch).await;
    size_pool(&mut connection, Some(4)).await;

    let mut first = scheduler(&url, 8).await;
    first.look().await.expect("looks");
    first.look().await.expect("looks again");
    assert_eq!(in_slots(&mut connection).await, [approved.id]);
    assert!(!first.held.contains_key(&waiting.id));

    // Handed its id directly, the pool still refuses it: only a queued batch is assigned.
    let assigned = work_batch::assign(&mut connection, &[waiting.id], 4, Utc::now())
        .await
        .expect("looks");
    assert!(assigned.is_empty());

    // The engine stops, letting its leases lapse, and the next takes over every lapsed slot.
    first.release().await;
    let mut second = scheduler(&url, 8).await;
    second.look().await.expect("looks");
    assert_eq!(in_slots(&mut connection).await, [approved.id]);
    assert!(!second.held.contains_key(&waiting.id));

    // An engine that dies stops renewing; whatever takes its slots over once the lease lapses,
    // well past it, adopts the approved batch alone.
    let adopted = work_batch::adopt(&mut connection, Utc::now() + TimeDelta::hours(1))
        .await
        .expect("adopts");
    let adopted: Vec<Uuid> = adopted.iter().map(|slot| slot.batch.id).collect();
    assert_eq!(adopted, [approved.id]);
    let mut third = scheduler(&url, 8).await;
    third.look().await.expect("looks");

    let still = work_batch::find(&mut connection, waiting.id)
        .await
        .expect("reads");
    assert_eq!(still.state, BatchState::Proposed);
    assert!(still.assigned_at.is_none());

    // Not even a write that skips every function here can claim it: the table refuses a claim on
    // anything but a batch in a slot.
    let claimed = diesel::update(work_batches::table.find(waiting.id))
        .set((
            work_batches::claim_token.eq(Some(Uuid::now_v7())),
            work_batches::lease_expires_at.eq(Some(Utc::now())),
        ))
        .execute(&mut connection)
        .await;
    assert!(
        claimed.is_err(),
        "work_batches_claimed_while_in_a_slot refuses it"
    );
}
