// Copyright © 2026 Jalapeno Labs

//! The Project Managers against Postgres: what wakes one, how a burst becomes one turn, what a
//! turn leaves behind, and that a restart or a usage limit loses nothing.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chrono::{TimeDelta, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::batches::{self, BatchError, Submission};
use super::dispatcher::{self, DispatchContext};
use super::runner::{ManagerRunner, Script};
use super::timeline::SOURCE_KIND;
use crate::database::schema::project_manager_wakes;
use crate::elysia::runner::ElysiaOutcome;
use crate::models::attribution::{self, Attributor};
use crate::models::project::{self, NewProject};
use crate::models::project_manager::{self, TurnStatus, WakeKind};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineQuery, TimelineRecord};
use crate::models::user::{ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID};
use crate::models::work_batch::{self, BatchState, Priority};
use crate::realtime::EventBus;
use crate::test_support::{TEST_PERSON_ID, migrated_database};
use crate::tools::grants::Grants;

async fn project_named(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    project::create(
        connection,
        &NewProject {
            name: name.to_owned(),
            description: format!("{name}, the project."),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id
}

/// A meeting segment on the test person's timeline, not yet on any project.
async fn segment(connection: &mut AsyncPgConnection, title: &str) -> Uuid {
    timeline_record::insert_new(
        connection,
        NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: "meeting".to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: Utc::now(),
            actor: "Ada".to_owned(),
            title: title.to_owned(),
            body: format!("[0:01] Ada: {title}."),
            url: None,
            media: None,
            metadata: json!({ "summary": format!("Ada asked for {title}.") }),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("stored")
    .expect("new")
    .id
}

async fn waiting_kinds(connection: &mut AsyncPgConnection, project_id: Uuid) -> Vec<WakeKind> {
    project_manager_wakes::table
        .filter(project_manager_wakes::project_id.eq(project_id))
        .filter(project_manager_wakes::turn_id.is_null())
        .select(project_manager_wakes::kind)
        .order(project_manager_wakes::created_at.asc())
        .load(connection)
        .await
        .expect("wakes")
}

/// Makes every waiting wake look older than the debounce, as if it went quiet long ago.
async fn go_quiet(connection: &mut AsyncPgConnection) {
    diesel::update(project_manager_wakes::table)
        .set(project_manager_wakes::created_at.eq(Utc::now() - TimeDelta::minutes(5)))
        .execute(connection)
        .await
        .expect("backdated");
}

/// A dispatcher against a migrated database whose runner plays back `outcomes`.
async fn scripted(
    outcomes: Vec<ElysiaOutcome>,
) -> (DispatchContext, Arc<Mutex<Script>>, AsyncPgConnection) {
    let (url, connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("Postgres accepts connections");
    let script = Arc::new(Mutex::new(Script {
        outcomes: VecDeque::from(outcomes),
        asked: Vec::new(),
    }));
    let context = DispatchContext {
        grants: Grants::new(database.clone()),
        database,
        events: EventBus::in_process(),
        runner: ManagerRunner::Scripted(Arc::clone(&script)),
    };
    (context, script, connection)
}

/// Runs one look of the dispatcher and waits for every turn it started.
async fn dispatch_once(context: &DispatchContext) {
    let mut running = tokio::task::JoinSet::new();
    dispatcher::dispatch(context, &mut running, &CancellationToken::new())
        .await
        .expect("the dispatcher looks");
    while running.join_next().await.is_some() {}
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_record_newly_on_a_project_wakes_its_project_manager_once() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;
    let record = segment(&mut connection, "annual pricing").await;

    attribution::attribute(&mut connection, record, launch, 0.6, Attributor::Elysia)
        .await
        .expect("attributes");
    assert_eq!(
        waiting_kinds(&mut connection, launch).await,
        [WakeKind::Record]
    );

    // How sure Elysia is changing is not news; neither is attributing it again.
    attribution::attribute(&mut connection, record, launch, 0.9, Attributor::Elysia)
        .await
        .expect("attributes");
    // Taken off and put back while it waits, it still waits once.
    attribution::unattribute(
        &mut connection,
        record,
        launch,
        Attributor::Person(TEST_PERSON_ID),
    )
    .await
    .expect("unattributes");
    attribution::attribute(
        &mut connection,
        record,
        launch,
        1.0,
        Attributor::Person(TEST_PERSON_ID),
    )
    .await
    .expect("attributes");
    assert_eq!(
        waiting_kinds(&mut connection, launch).await,
        [WakeKind::Record]
    );

    // A person putting it on another project wakes that project's Project Manager.
    attribution::attribute(
        &mut connection,
        record,
        website,
        1.0,
        Attributor::Person(TEST_PERSON_ID),
    )
    .await
    .expect("attributes");
    assert_eq!(
        waiting_kinds(&mut connection, website).await,
        [WakeKind::Record]
    );
}

/// What the scripted runner was asked to run so far.
fn asked(script: &Arc<Mutex<Script>>) -> Vec<super::runner::ManagerTurn> {
    script.lock().expect("script").asked.clone()
}

/// Record `record_id`, a Project Manager's, is on `project_id`'s timeline for every member,
/// though nobody owns it, and for no guest. Answers the record.
async fn assert_shared_on_timeline(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    record_id: Uuid,
) -> TimelineRecord {
    let someone_else = TimelineQuery {
        owner: Uuid::now_v7(),
        shared: true,
        project_id: Some(project_id),
        action_item_id: None,
        meeting_id: None,
        before: None,
        limit: 50,
    };
    let listed = timeline_record::list(connection, someone_else)
        .await
        .expect("listed");
    let (record, attributions) = listed
        .into_iter()
        .find(|(record, _)| record.id == record_id)
        .expect("listed for someone else");
    assert_eq!(record.source_kind, SOURCE_KIND);
    assert_eq!(record.created_by, PROJECT_MANAGER_USER_ID);
    assert_eq!(attributions[0].project_id, project_id);
    let a_guest = TimelineQuery {
        shared: false,
        ..someone_else
    };
    assert!(
        timeline_record::list(connection, a_guest)
            .await
            .expect("listed")
            .is_empty(),
        "a guest reads only their own records"
    );
    record
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_burst_of_segments_is_one_turn_that_lands_on_the_timeline_and_keeps_its_session() {
    let (context, script, mut connection) = scripted(vec![
        ElysiaOutcome::Completed {
            result: Some(
                "Submitted the pricing batch.\nSummary: Planned the pricing work.".to_owned(),
            ),
            session_id: Some("session-1".to_owned()),
        },
        ElysiaOutcome::Completed {
            result: Some("Noted.\nSummary: Nothing new to plan.".to_owned()),
            session_id: Some("session-1".to_owned()),
        },
    ])
    .await;
    let launch = project_named(&mut connection, "Launch").await;
    let mut titles = Vec::new();
    for topic in ["annual pricing", "a pricing FAQ", "the checkout copy"] {
        let record = segment(&mut connection, topic).await;
        attribution::attribute(&mut connection, record, launch, 0.8, Attributor::Elysia)
            .await
            .expect("attributes");
        titles.push(topic);
    }

    // Still arriving: nothing starts.
    dispatch_once(&context).await;
    assert!(script.lock().expect("script").asked.is_empty());

    go_quiet(&mut connection).await;
    dispatch_once(&context).await;
    let first = asked(&script);
    assert_eq!(first.len(), 1, "one turn for the whole burst");
    assert_eq!(first[0].resume, None, "its first turn starts the session");
    for title in &titles {
        assert!(first[0].prompt.contains(title), "{title} is in the prompt");
    }
    assert!(
        first[0]
            .prompt
            .contains("New on the project's timeline (3)")
    );

    let turns = project_manager::list_turns(&mut connection, launch, 10)
        .await
        .expect("turns");
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].status, TurnStatus::Succeeded);
    assert_eq!(
        turns[0].summary.as_deref(),
        Some("Planned the pricing work.")
    );
    assert_eq!(
        turns[0].reply.as_deref(),
        Some("Submitted the pricing batch.")
    );
    assert!(waiting_kinds(&mut connection, launch).await.is_empty());

    let record_id = turns[0].timeline_record_id.expect("a record of the turn");
    let record = assert_shared_on_timeline(&mut connection, launch, record_id).await;
    assert_eq!(record.title, "Planned the pricing work.");
    // Its own record woke nothing.
    assert!(waiting_kinds(&mut connection, launch).await.is_empty());

    // A person's message is due at once, and the turn resumes the session.
    super::message(
        &mut connection,
        &context.events,
        launch,
        "Ship the pricing change this week.",
        TEST_PERSON_ID,
    )
    .await
    .expect("sent");
    dispatch_once(&context).await;
    let second = asked(&script);
    assert_eq!(second.len(), 2);
    assert_eq!(second[1].resume.as_deref(), Some("session-1"));
    assert!(
        second[1]
            .prompt
            .contains("Ship the pricing change this week.")
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_projects_turns_run_one_at_a_time() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    for body in ["First.", "Second."] {
        project_manager::wake_with_message(&mut connection, launch, body, TEST_PERSON_ID)
            .await
            .expect("woken");
    }
    let (turn, taken) = project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("a turn");
    assert_eq!(taken.len(), 2, "every waiting message goes with it");

    project_manager::wake_with_message(&mut connection, launch, "Third.", TEST_PERSON_ID)
        .await
        .expect("woken");
    assert!(
        project_manager::start_turn(&mut connection, launch, 40)
            .await
            .expect("looks")
            .is_none(),
        "a second turn waits for the first"
    );
    assert_eq!(
        waiting_kinds(&mut connection, launch).await,
        [WakeKind::Message]
    );
    assert_eq!(turn.status, TurnStatus::Running);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_turn_a_restart_cut_short_gives_its_wakes_to_the_next_one() {
    let (context, script, mut connection) = scripted(vec![ElysiaOutcome::Completed {
        result: Some("Summary: Picked it back up.".to_owned()),
        session_id: None,
    }])
    .await;
    let launch = project_named(&mut connection, "Launch").await;
    super::message(
        &mut connection,
        &context.events,
        launch,
        "Plan the launch.",
        ELYSIA_USER_ID,
    )
    .await
    .expect("sent");
    // An engine takes it, then stops before the turn ends.
    project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("a turn");
    assert!(waiting_kinds(&mut connection, launch).await.is_empty());

    // The next engine starts: the turn fails and its message waits again.
    let failed = project_manager::fail_interrupted_turns(&mut connection, "stopped")
        .await
        .expect("recovered");
    assert_eq!(failed.len(), 1);
    assert_eq!(
        waiting_kinds(&mut connection, launch).await,
        [WakeKind::Message]
    );

    dispatch_once(&context).await;
    let script = script.lock().expect("script");
    let prompt = &script.asked[0].prompt;
    assert!(prompt.contains("Plan the launch."));
    assert!(prompt.contains("From Elysia"));
    assert!(
        prompt.contains("did not finish"),
        "it is told to check first"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_usage_limit_holds_the_project_and_keeps_what_woke_it() {
    let resets_at = Utc::now() + TimeDelta::hours(2);
    let (context, script, mut connection) = scripted(vec![ElysiaOutcome::UsageLimited {
        resets_at,
        session_id: Some("session-9".to_owned()),
    }])
    .await;
    let launch = project_named(&mut connection, "Launch").await;
    super::message(
        &mut connection,
        &context.events,
        launch,
        "Go.",
        TEST_PERSON_ID,
    )
    .await
    .expect("sent");

    dispatch_once(&context).await;
    let turns = project_manager::list_turns(&mut connection, launch, 10)
        .await
        .expect("turns");
    assert_eq!(turns[0].status, TurnStatus::Failed);
    assert_eq!(
        waiting_kinds(&mut connection, launch).await,
        [WakeKind::Message]
    );
    let manager = project_manager::find(&mut connection, launch)
        .await
        .expect("reads")
        .expect("a row");
    assert_eq!(manager.runner_session_id.as_deref(), Some("session-9"));
    assert!(
        manager
            .usage_paused_until
            .is_some_and(|until| until > Utc::now())
    );

    // Held: the next look starts nothing.
    dispatch_once(&context).await;
    assert_eq!(script.lock().expect("script").asked.len(), 1);
    let snapshot = super::snapshot(&mut connection, launch)
        .await
        .expect("reads");
    assert_eq!(snapshot.status, super::status::Status::Held);
}

fn submission(record_ids: Vec<Uuid>) -> Submission {
    Submission {
        title: "Annual pricing".to_owned(),
        goal: "Show annual pricing on the pricing page, as Ada asked.".to_owned(),
        repositories: Vec::new(),
        priority: Priority::Asap,
        scheduled_for: None,
        record_ids,
        action_item_ids: Vec::new(),
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_submitted_batch_lands_on_the_timeline_and_its_result_wakes_its_project_manager() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;
    let record = segment(&mut connection, "annual pricing").await;
    attribution::attribute(&mut connection, record, launch, 0.8, Attributor::Elysia)
        .await
        .expect("attributes");
    let other = segment(&mut connection, "the website's footer").await;
    attribution::attribute(&mut connection, other, website, 0.8, Attributor::Elysia)
        .await
        .expect("attributes");
    let (turn, _taken) = project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("a turn");

    // Provenance must be on the project.
    let refused = batches::submit(
        &mut connection,
        &events,
        launch,
        turn.id,
        submission(vec![other]),
    )
    .await
    .expect_err("a record of another project");
    assert!(
        matches!(refused, BatchError::Refused(reason) if reason.contains("not on this project"))
    );

    let batch = batches::submit(
        &mut connection,
        &events,
        launch,
        turn.id,
        submission(vec![record]),
    )
    .await
    .expect("submitted");
    assert_eq!(batch.state, BatchState::Queued);
    assert_eq!(batch.turn_id, Some(turn.id));
    assert_eq!(batch.source_records(), vec![record]);
    let on_timeline = timeline_record::attributed_among(
        &mut connection,
        launch,
        &[batch.timeline_record_id.expect("a record")],
    )
    .await
    .expect("reads");
    assert_eq!(on_timeline[0].0.title, "Annual pricing");
    assert_eq!(on_timeline[0].0.metadata["priority"], "asap");

    // The pool hands it a slot, and a worker takes it and finishes it: its Project Manager
    // wakes with the result.
    let slots = work_batch::assign(&mut connection, &[batch.id], 1, Utc::now())
        .await
        .expect("assigns");
    let slot = slots.into_iter().next().expect("a slot");
    work_batch::start(&mut connection, &slot)
        .await
        .expect("starts")
        .expect("it was assigned");
    let finished = work_batch::finish(
        &mut connection,
        &slot,
        true,
        "Annual pricing is live behind a flag.",
        crate::models::user::SYSTEM_USER_ID,
    )
    .await
    .expect("finishes")
    .expect("it was running");
    assert_eq!(finished.state, BatchState::Succeeded);
    let waiting = waiting_kinds(&mut connection, launch).await;
    assert!(waiting.contains(&WakeKind::Batch), "{waiting:?}");

    // A finished batch is not withdrawn.
    let not_queued = batches::cancel(&mut connection, &events, launch, batch.id, "Changed plans")
        .await
        .expect_err("finished");
    assert!(matches!(not_queued, BatchError::Refused(_)));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_scheduled_batch_is_stored_with_its_time_and_a_queued_one_can_be_withdrawn() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    project_manager::wake_with_message(&mut connection, launch, "Plan.", TEST_PERSON_ID)
        .await
        .expect("woken");
    let (turn, _taken) = project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("a turn");

    let due = Utc::now() + TimeDelta::days(1);
    let mut scheduled = submission(Vec::new());
    scheduled.priority = Priority::Scheduled;
    scheduled.scheduled_for = Some(due);
    let batch = batches::submit(&mut connection, &events, launch, turn.id, scheduled)
        .await
        .expect("submitted");
    assert_eq!(batch.priority, Priority::Scheduled);
    assert_eq!(
        batch.scheduled_for.map(|at| at.timestamp()),
        Some(due.timestamp())
    );

    let cancelled = batches::cancel(&mut connection, &events, launch, batch.id, "Launch moved")
        .await
        .expect("withdrawn");
    assert_eq!(cancelled.state, BatchState::Cancelled);
    assert_eq!(cancelled.result.as_deref(), Some("Launch moved"));
}
