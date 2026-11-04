// Copyright © 2026 Jalapeno Labs

//! The scheduler's looks against Postgres: slots handed out in order and within the size, none
//! while every Claude account is at its usage limit, slots taken back when the pool shrinks, and
//! slots taken over by the next engine.

use chrono::TimeDelta;
use diesel_async::AsyncPgConnection;
use secrecy::SecretString;
use serde_json::json;

use super::*;
use crate::models::claude_account::{
    self, Identity, NewCredential, SignedIn, Tokens, UsageWindow, UsageWindows,
};
use crate::models::project::{self, NewProject};
use crate::models::user::{self, PROJECT_MANAGER_USER_ID, WorkspaceSettingsChanges};
use crate::models::work_batch::{NewWorkBatch, Priority, WorkBatch};
use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};

async fn scheduler(url: &SecretString, cpu_count: i32) -> Scheduler {
    Scheduler {
        database: crate::connections::connect_postgres(url, 4)
            .await
            .expect("Postgres accepts connections"),
        events: EventBus::in_process(),
        cpu_count,
        held: HashMap::new(),
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

async fn submitted(connection: &mut AsyncPgConnection, project_id: Uuid) -> WorkBatch {
    work_batch::submit(
        connection,
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
            created_by: PROJECT_MANAGER_USER_ID,
        },
    )
    .await
    .expect("submitted")
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
async fn no_slot_is_handed_out_while_every_claude_account_is_spent() {
    let (url, mut connection) = migrated_database().await;
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
