// Copyright © 2026 Jalapeno Labs

//! A batch's lineage: the rules alone, then what its provenance proves against Postgres. How a
//! Project Manager's submission is held to them is tested with the submission
//! (`crate::project_managers::tests`).

use chrono::Utc;

use super::*;
use crate::action_items::{Actor, Transition};
use crate::models::action_item::{self, ActionItemPriority, ActionItemState, NewActionItem, Owner};
use crate::models::project::{self, NewProject};
use crate::models::project_manager;
use crate::models::timeline_record::{self, NewTimelineRecord};
use crate::models::user::{ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID};
use crate::test_support::{TEST_PERSON_ID, migrated_database};

fn parent(generation: i32, worked_on: bool) -> Parent {
    Parent {
        id: Uuid::now_v7(),
        generation,
        worked_on,
    }
}

#[test]
fn a_batch_that_names_nothing_has_no_origin() {
    assert_eq!(decide(&[], &[]), Err(Refusal::NoOrigin));
    // What led to it is no origin either: a follow-up must still name what it is for.
    assert_eq!(decide(&[], &[parent(0, true)]), Err(Refusal::NoOrigin));
}

#[test]
fn anything_a_person_stands_behind_roots_the_batch() {
    let rooted = decide(&[Root::Agent, Root::Person], &[]).expect("a lineage");
    assert_eq!(rooted.origin(), Origin::HumanRooted);
    assert_eq!(rooted.generation(), 0);

    let found = decide(&[Root::Agent, Root::Agent], &[]).expect("a lineage");
    assert_eq!(found.origin(), Origin::Found);
    assert_eq!(found.generation(), 0);
}

#[test]
fn each_batch_a_worker_worked_on_adds_a_generation() {
    let deepest = parent(2, true);
    let shallow = parent(0, true);
    let lineage = decide(&[Root::Person], &[shallow, deepest, shallow]).expect("a lineage");
    assert_eq!(lineage.generation(), 3);
    let mut expected = vec![shallow.id, deepest.id];
    expected.sort_unstable();
    assert_eq!(
        lineage.parent_batch_ids,
        json!(expected),
        "each once, in order"
    );

    // A batch no worker touched, one a person rejected, is replaced where it stood.
    let replacing = decide(&[Root::Person], &[parent(2, false)]).expect("a lineage");
    assert_eq!(replacing.generation(), 2);
}

#[test]
fn a_batch_past_the_limit_is_refused() {
    let last = parent(MAX_GENERATION - 1, true);
    assert_eq!(
        decide(&[Root::Person], &[last]).map(|lineage| lineage.generation()),
        Ok(MAX_GENERATION)
    );

    let past = parent(MAX_GENERATION, true);
    assert_eq!(
        decide(&[Root::Person], &[past]),
        Err(Refusal::PastLimit {
            generation: MAX_GENERATION + 1,
            parents: vec![past.id],
        })
    );
}

#[test]
fn accepting_a_finding_starts_the_chain_again() {
    let accepted = Lineage::accepted_finding();
    assert_eq!(accepted.origin(), Origin::HumanRooted);
    assert_eq!(accepted.generation(), 0);
    assert_eq!(accepted.parent_batch_ids, json!([]));
}

fn event(kind: &str, actor: &str, data: Value, decided_by: Option<Uuid>) -> ItemEvent {
    ItemEvent {
        action_item_id: Uuid::nil(),
        kind: kind.to_owned(),
        actor: actor.to_owned(),
        data,
        decided_by,
    }
}

#[test]
fn an_item_is_a_persons_when_a_person_or_a_watcher_stands_behind_it() {
    let person = Uuid::now_v7();
    let people = HashSet::from([person]);
    let accepted = json!({ "from": "inbox", "to": "open" });
    let roots = |entry: &ItemEvent| entry.roots_in_a_person(&people);

    // Created by a person, by a person before accounts, or brought in by a provider's watcher.
    assert!(roots(&event(
        "created",
        &format!("user:{person}"),
        json!({}),
        None
    )));
    assert!(roots(&event("created", "user", json!({}), None)));
    assert!(roots(&event("created", "watcher:jira", json!({}), None)));
    // Created by an agent, unreviewed: Elysia from a conversation, a Project Manager directly.
    assert!(!roots(&event("created", "elysia", json!({}), None)));
    assert!(!roots(&event(
        "created",
        "project_manager:x",
        json!({}),
        None
    )));
    // Created by a changeset: whoever decided it is what counts.
    assert!(roots(&event(
        "created",
        "project_manager:x",
        json!({}),
        Some(person)
    )));
    let machine = Some(PROJECT_MANAGER_USER_ID);
    assert!(!roots(&event(
        "created",
        "project_manager:x",
        json!({}),
        machine
    )));
    // Accepted from the inbox by a person, and only by a person, and only into open.
    assert!(roots(&event(
        "state_changed",
        &format!("user:{person}"),
        accepted.clone(),
        None
    )));
    assert!(!roots(&event("state_changed", "elysia", accepted, None)));
    let dismissed = json!({ "from": "inbox", "to": "dismissed" });
    assert!(!roots(&event("state_changed", "user", dismissed, None)));
    assert!(!roots(&event("commented", "user", json!({}), None)));
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

async fn record_owned_by(connection: &mut AsyncPgConnection, owner: Uuid, metadata: Value) -> Uuid {
    timeline_record::insert_new(
        connection,
        NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: "meeting".to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: Utc::now(),
            actor: "Ada".to_owned(),
            title: "Annual pricing".to_owned(),
            body: "Ada asked for annual pricing.".to_owned(),
            url: None,
            media: None,
            metadata,
            created_by: owner,
        },
    )
    .await
    .expect("stored")
    .expect("new")
    .id
}

async fn item_by(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    state: ActionItemState,
    actor: Actor,
) -> Uuid {
    action_item::create(
        connection,
        NewActionItem {
            title: "Annual pricing".to_owned(),
            notes: String::new(),
            state,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: vec![project_id],
            initiative_ids: Vec::new(),
        },
        actor,
        Utc::now(),
    )
    .await
    .expect("created")
    .record
    .id
}

async fn roots_of(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    record_ids: &[Uuid],
    action_item_ids: &[Uuid],
    message_ids: &[Uuid],
) -> Vec<Root> {
    let provenance = Provenance {
        record_ids,
        action_item_ids,
        message_ids,
    };
    roots(connection, project_id, provenance)
        .await
        .expect("reads")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn what_a_batch_names_traces_to_a_person_or_to_an_agent() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;

    // A record ingested for a person is theirs; one a Project Manager made is a machine's.
    let segment = record_owned_by(&mut connection, TEST_PERSON_ID, json!({})).await;
    let batch_record = record_owned_by(&mut connection, PROJECT_MANAGER_USER_ID, json!({})).await;
    assert_eq!(
        roots_of(&mut connection, launch, &[segment], &[], &[]).await,
        [Root::Person]
    );
    assert_eq!(
        roots_of(&mut connection, launch, &[batch_record], &[], &[]).await,
        [Root::Agent]
    );

    // A message from a person roots it; Elysia's does not, and another project's names nothing.
    let asked =
        project_manager::wake_with_message(&mut connection, launch, "Ship it.", TEST_PERSON_ID)
            .await
            .expect("woken");
    let relayed =
        project_manager::wake_with_message(&mut connection, launch, "Ship it.", ELYSIA_USER_ID)
            .await
            .expect("woken");
    let elsewhere =
        project_manager::wake_with_message(&mut connection, website, "Ship it.", TEST_PERSON_ID)
            .await
            .expect("woken");
    assert_eq!(
        roots_of(&mut connection, launch, &[], &[], &[asked.id]).await,
        [Root::Person]
    );
    assert_eq!(
        roots_of(&mut connection, launch, &[], &[], &[relayed.id]).await,
        [Root::Agent]
    );
    assert!(
        roots_of(&mut connection, launch, &[], &[], &[elsewhere.id])
            .await
            .is_empty()
    );

    // An item a person typed is theirs. Elysia's from a conversation is her reading of it until a
    // person accepts it from the inbox.
    let typed = item_by(
        &mut connection,
        launch,
        ActionItemState::Open,
        Actor::User(TEST_PERSON_ID),
    )
    .await;
    let raised = item_by(
        &mut connection,
        launch,
        ActionItemState::Inbox,
        Actor::Elysia,
    )
    .await;
    assert_eq!(
        roots_of(&mut connection, launch, &[], &[typed], &[]).await,
        [Root::Person]
    );
    assert_eq!(
        roots_of(&mut connection, launch, &[], &[raised], &[]).await,
        [Root::Agent]
    );
    action_item::transition(
        &mut connection,
        raised,
        Transition::Accept,
        Actor::User(TEST_PERSON_ID),
        Utc::now(),
    )
    .await
    .expect("accepted");
    assert_eq!(
        roots_of(&mut connection, launch, &[], &[raised], &[]).await,
        [Root::Person],
        "one root per item, however many entries show a person"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn records_name_their_batch_only_when_elysium_made_them() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;
    let batch = crate::test_support::approved_batch(
        &mut connection,
        crate::models::work_batch::NewWorkBatch {
            id: Uuid::now_v7(),
            project_id: launch,
            title: "Annual pricing".to_owned(),
            goal: "Show annual pricing.".to_owned(),
            repositories: json!([]),
            priority: crate::models::work_batch::Priority::Normal,
            scheduled_for: None,
            turn_id: None,
            source_record_ids: json!([]),
            action_item_ids: json!([]),
            source_message_ids: json!([]),
            lineage: crate::test_support::person_rooted(),
            created_by: PROJECT_MANAGER_USER_ID,
        },
    )
    .await;

    // Elysium's own record of the batch leads back to it. A person's record saying the same in
    // its metadata is something ingested, which names no batch of Elysium's.
    let made = record_owned_by(
        &mut connection,
        PROJECT_MANAGER_USER_ID,
        json!({ "kind": "batch", "batchId": batch.id }),
    )
    .await;
    let claimed = record_owned_by(
        &mut connection,
        TEST_PERSON_ID,
        json!({ "batchId": batch.id }),
    )
    .await;
    let found = parents(&mut connection, launch, None, &[made, claimed])
        .await
        .expect("reads");
    assert_eq!(
        found,
        [Parent {
            id: batch.id,
            generation: 0,
            worked_on: false,
        }]
    );

    // A batch of another project leads to nothing here.
    assert!(
        parents(&mut connection, website, None, &[made])
            .await
            .expect("reads")
            .is_empty()
    );
}
