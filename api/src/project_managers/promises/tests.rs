// Copyright © 2026 Jalapeno Labs

//! Promises against Postgres: found once per message, due when their work finishes, tied only to
//! work that can keep them, and followed up with a draft the person decides on.

use serde_json::json;

use super::*;
use crate::action_items::Transition;
use crate::database::schema::project_manager_wakes;
use crate::models::connector::{self, NewConnector};
use crate::models::draft::{Draft, DraftState};
use crate::models::project::{self, NewProject};
use crate::models::project_manager::WakeKind;
use crate::models::promise::NewPromise;
use crate::models::timeline_record::NewTimelineRecord;
use crate::models::user::SYSTEM_USER_ID;
use crate::models::work_batch::{NewWorkBatch, Priority};
use crate::test_support::{TEST_PERSON, TEST_PERSON_ID, approved_batch, migrated_database};

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

/// A message of the test person's in a Slack thread, through a Slack connector of theirs: what a
/// promise is made in, and what its follow-up answers.
async fn slack_message(connection: &mut AsyncPgConnection) -> TimelineRecord {
    let slack = connector::create(
        connection,
        NewConnector {
            id: Uuid::now_v7(),
            kind: "slack".to_owned(),
            browser_session_id: None,
            token_account_id: None,
            jira_credential_id: None,
            github_credential_id: None,
            storage_location_id: None,
            content: json!({ "conversations": "*" }),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a connector");
    timeline_record::insert_new(
        connection,
        NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: "slack".to_owned(),
            connector_id: Some(slack.id),
            external_id: Some(format!("T1/C1/{}", Uuid::now_v7())),
            occurred_at: Utc::now(),
            actor: "Tester".to_owned(),
            title: "Reply in #launch in Mooreslab".to_owned(),
            body: "On it, I'll ping you here once it's deployed.".to_owned(),
            url: None,
            media: None,
            metadata: json!({
                "team": "T1",
                "conversation": "C1",
                "ts": "1730000000.000200",
                "threadTs": "1730000000.000100",
            }),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("stored")
    .expect("new")
}

async fn promise_in(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    record: &TimelineRecord,
) -> Promise {
    promise::record(
        connection,
        &[NewPromise {
            project_id,
            timeline_record_id: record.id,
            recipient: "Ada".to_owned(),
            summary: "when the login fix is live".to_owned(),
        }],
    )
    .await
    .expect("recorded")
    .pop()
    .expect("new")
}

async fn queued_batch(connection: &mut AsyncPgConnection, project_id: Uuid) -> Uuid {
    approved_batch(
        connection,
        NewWorkBatch {
            id: Uuid::now_v7(),
            project_id,
            title: "Fix the login bug".to_owned(),
            goal: "Fix it, as Ada asked.".to_owned(),
            repositories: json!([]),
            priority: Priority::Asap,
            scheduled_for: None,
            turn_id: None,
            source_record_ids: json!([]),
            action_item_ids: json!([]),
            serves: None,
            source_message_ids: json!([]),
            lineage: crate::test_support::person_rooted(),
            created_by: user::PROJECT_MANAGER_USER_ID,
        },
    )
    .await
    .id
}

async fn item_in(connection: &mut AsyncPgConnection, project_id: Uuid) -> Uuid {
    action_item::create(
        connection,
        NewActionItem {
            title: "Fix the login bug".to_owned(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: vec![project_id],
            initiative_ids: Vec::new(),
        },
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("an item")
    .record
    .id
}

async fn promise_wakes(connection: &mut AsyncPgConnection, project_id: Uuid) -> Vec<Uuid> {
    project_manager_wakes::table
        .filter(project_manager_wakes::project_id.eq(project_id))
        .filter(project_manager_wakes::kind.eq(WakeKind::Promise))
        .filter(project_manager_wakes::turn_id.is_null())
        .select(project_manager_wakes::promise_id.assume_not_null())
        .load(connection)
        .await
        .expect("wakes")
}

async fn state_of(connection: &mut AsyncPgConnection, id: Uuid) -> PromiseState {
    promise::find_many(connection, &[id])
        .await
        .expect("read")
        .pop()
        .expect("there")
        .state
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_promise_is_recorded_once_per_message_and_wakes_its_project_manager() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let record = slack_message(&mut connection).await;

    let made = promise_in(&mut connection, launch, &record).await;
    assert_eq!(made.state, PromiseState::Waiting);
    assert_eq!(made.work(), None);
    assert_eq!(made.created_by, user::ELYSIA_USER_ID);
    assert_eq!(promise_wakes(&mut connection, launch).await, vec![made.id]);

    // The conversation read again as it grows finds the same promise: nothing is recorded twice.
    let again = promise::record(
        &mut connection,
        &[NewPromise {
            project_id: launch,
            timeline_record_id: record.id,
            recipient: "Ada Lovelace".to_owned(),
            summary: "once it ships".to_owned(),
        }],
    )
    .await
    .expect("recorded");
    assert!(again.is_empty());
    assert_eq!(promise_wakes(&mut connection, launch).await, vec![made.id]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_promise_comes_due_when_its_batch_succeeds_and_not_when_it_fails() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let kept_message = slack_message(&mut connection).await;
    let kept = promise_in(&mut connection, launch, &kept_message).await;
    let broken_message = slack_message(&mut connection).await;
    let broken = promise_in(&mut connection, launch, &broken_message).await;
    let succeeding = queued_batch(&mut connection, launch).await;
    let failing = queued_batch(&mut connection, launch).await;

    tie(
        &mut connection,
        &events,
        launch,
        kept.id,
        Work::Batch(succeeding),
    )
    .await
    .expect("tied");
    tie(
        &mut connection,
        &events,
        launch,
        broken.id,
        Work::Batch(failing),
    )
    .await
    .expect("tied");
    assert_eq!(
        state_of(&mut connection, kept.id).await,
        PromiseState::Waiting
    );

    for (batch_id, succeeded) in [(succeeding, true), (failing, false)] {
        let slot = work_batch::assign(&mut connection, &[batch_id], 2, Utc::now())
            .await
            .expect("assigns")
            .pop()
            .expect("a slot");
        work_batch::start(&mut connection, &slot)
            .await
            .expect("starts")
            .expect("assigned");
        work_batch::finish(&mut connection, &slot, succeeded, "Done.", SYSTEM_USER_ID)
            .await
            .expect("finishes")
            .expect("running");
    }

    let due = promise::find_many(&mut connection, &[kept.id])
        .await
        .expect("read")
        .pop()
        .expect("there");
    assert_eq!(due.state, PromiseState::Due);
    assert!(due.due_at.is_some());
    assert!(
        promise_wakes(&mut connection, launch)
            .await
            .contains(&kept.id)
    );
    // A batch that failed never keeps a promise: it waits for the work that replaces it.
    assert_eq!(
        state_of(&mut connection, broken.id).await,
        PromiseState::Waiting
    );
    let waiting = promise::waiting_on_batches(&mut connection, &[failing])
        .await
        .expect("read");
    assert_eq!(waiting.len(), 1);
    let refused = tie(
        &mut connection,
        &events,
        launch,
        broken.id,
        Work::Batch(failing),
    )
    .await
    .expect_err("a failed batch keeps nothing");
    assert!(matches!(refused, PromiseError::Refused(reason) if reason.contains("did not succeed")));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_promise_is_tied_only_to_its_own_projects_work_and_comes_due_with_its_item() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;
    let made_message = slack_message(&mut connection).await;
    let made = promise_in(&mut connection, launch, &made_message).await;

    let elsewhere = queued_batch(&mut connection, website).await;
    let refused = tie(
        &mut connection,
        &events,
        launch,
        made.id,
        Work::Batch(elsewhere),
    )
    .await
    .expect_err("another project's batch");
    assert!(matches!(refused, PromiseError::Refused(reason) if reason.contains("not one of")));
    let other_item = item_in(&mut connection, website).await;
    let refused = tie(
        &mut connection,
        &events,
        launch,
        made.id,
        Work::Item(other_item),
    )
    .await
    .expect_err("another project's item");
    assert!(matches!(refused, PromiseError::Refused(reason) if reason.contains("not in this")));

    // An item resolving is the promise's work finishing, whoever resolves it: the person, or the
    // watcher reading its pull request merged.
    let fix = item_in(&mut connection, launch).await;
    let tied = tie(&mut connection, &events, launch, made.id, Work::Item(fix))
        .await
        .expect("tied");
    assert_eq!(tied.work(), Some(Work::Item(fix)));
    action_item::transition(
        &mut connection,
        fix,
        Transition::Resolve,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("resolved");
    assert_eq!(state_of(&mut connection, made.id).await, PromiseState::Due);

    // Work already done makes a promise due as it is tied.
    let done_already_message = slack_message(&mut connection).await;
    let done_already = promise_in(&mut connection, launch, &done_already_message).await;
    let tied = tie(
        &mut connection,
        &events,
        launch,
        done_already.id,
        Work::Item(fix),
    )
    .await
    .expect("tied");
    assert_eq!(tied.state, PromiseState::Due);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario: a follow-up drafted, then decided on every way the person can"
)]
async fn a_follow_up_is_a_draft_of_the_persons_and_an_inbox_item_they_decide_on() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let record = slack_message(&mut connection).await;
    let made = promise_in(&mut connection, launch, &record).await;

    let refused = draft_follow_up(&mut connection, &events, launch, made.id, "It's live.")
        .await
        .expect_err("its work is not done");
    assert!(matches!(refused, PromiseError::Refused(_)));

    let fix = item_in(&mut connection, launch).await;
    tie(&mut connection, &events, launch, made.id, Work::Item(fix))
        .await
        .expect("tied");
    action_item::transition(
        &mut connection,
        fix,
        Transition::Resolve,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("resolved");

    let follow_up = draft_follow_up(
        &mut connection,
        &events,
        launch,
        made.id,
        "  The login fix is live. Let me know if you still see it.  ",
    )
    .await
    .expect("drafted");
    assert_eq!(follow_up.promise.state, PromiseState::Drafted);
    assert_eq!(follow_up.promise.follow_up_item_id, Some(follow_up.item.id));
    let item = follow_up.item;
    assert_eq!(item.state, ActionItemState::Inbox);
    assert_eq!(item.owner(), Owner::User);
    assert_eq!(item.created_by, user::PROJECT_MANAGER_USER_ID);
    assert_eq!(item.title, "Follow up with Ada: when the login fix is live");
    assert!(
        item.notes
            .contains("I'll ping you here once it's deployed.")
    );

    // The draft is the person's, answering the message they promised in, in its thread, and it
    // belongs to the item: the message's own reply box never shows it.
    let drafted: Vec<Draft> = crate::database::schema::drafts::table
        .filter(crate::database::schema::drafts::promise_id.eq(made.id))
        .select(Draft::as_select())
        .load(&mut connection)
        .await
        .expect("read");
    assert_eq!(drafted.len(), 1);
    let drafted = &drafted[0];
    assert_eq!(drafted.created_by, TEST_PERSON_ID);
    assert_eq!(drafted.state, DraftState::Draft);
    assert_eq!(drafted.reply_to_record_id, Some(record.id));
    assert_eq!(
        drafted.body,
        "The login fix is live. Let me know if you still see it."
    );
    assert_eq!(
        drafted.target,
        json!({ "team": "T1", "channel": "C1", "threadTs": "1730000000.000100" })
    );
    let in_reply_box = draft::list_unsent_for_record(&mut connection, record.id, TEST_PERSON_ID)
        .await
        .expect("listed");
    assert!(in_reply_box.is_empty());

    // Drafted once: the promise is no longer due.
    let refused = draft_follow_up(&mut connection, &events, launch, made.id, "Again.")
        .await
        .expect_err("drafted already");
    assert!(matches!(refused, PromiseError::Refused(_)));

    // Dismissing the item dismisses the promise; reopening and resolving it, having told them
    // some other way, keeps it.
    action_item::transition(
        &mut connection,
        item.id,
        Transition::Dismiss,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("dismissed");
    assert_eq!(
        state_of(&mut connection, made.id).await,
        PromiseState::Dismissed
    );
    action_item::transition(
        &mut connection,
        item.id,
        Transition::Reopen,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("reopened");
    assert_eq!(
        state_of(&mut connection, made.id).await,
        PromiseState::Dismissed
    );
    action_item::transition(
        &mut connection,
        item.id,
        Transition::Resolve,
        TEST_PERSON,
        Utc::now(),
    )
    .await
    .expect("resolved");
    assert_eq!(state_of(&mut connection, made.id).await, PromiseState::Kept);

    let following = promise::projects_following(&mut connection, &[item.id, fix])
        .await
        .expect("read");
    assert_eq!(following, vec![launch]);
}

fn promise() -> Promise {
    Promise {
        id: Uuid::now_v7(),
        project_id: Uuid::now_v7(),
        timeline_record_id: Uuid::now_v7(),
        recipient: "Ada".to_owned(),
        summary: "when the login fix is live".to_owned(),
        state: PromiseState::Due,
        work_batch_id: None,
        work_item_id: None,
        due_at: Some(Utc::now()),
        follow_up_item_id: None,
        created_by: user::ELYSIA_USER_ID,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[test]
fn a_follow_up_item_names_who_to_tell_and_quotes_the_promise() {
    let made = promise();
    let record = TimelineRecord {
        id: made.timeline_record_id,
        source_kind: "slack".to_owned(),
        connector_id: None,
        external_id: None,
        occurred_at: Utc::now(),
        actor: "Alex".to_owned(),
        title: "Ada in a direct message in Mooreslab".to_owned(),
        body: "On it.\nI'll ping you here once it's deployed.".to_owned(),
        url: None,
        media: None,
        metadata: json!({}),
        action_item_id: None,
        created_by: Uuid::now_v7(),
        created_at: Utc::now(),
        excerpt_mark_id: None,
        deleted_at: None,
    };

    assert_eq!(
        item_title(&made),
        "Follow up with Ada: when the login fix is live"
    );
    assert_eq!(
        item_notes(&made, &record),
        "You promised Ada in Ada in a direct message in Mooreslab:\n\n> On it.\n> I'll ping \
         you here once it's deployed."
    );
}
