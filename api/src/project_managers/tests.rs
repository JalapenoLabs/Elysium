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
use crate::models::action_item::{self, ActionItemPriority, ActionItemState, NewActionItem, Owner};
use crate::models::action_item_attribution;
use crate::models::agent::{Agent, Author};
use crate::models::attribution::{self, Attributor};
use crate::models::page::{self, PageEdit, PageError};
use crate::models::page_project;
use crate::models::project::{self, NewProject};
use crate::models::project_brief;
use crate::models::project_manager::{self, TurnStatus, WakeKind};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineQuery, TimelineRecord};
use crate::models::user::{CODING_AGENT_USER_ID, ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID};
use crate::models::work_batch::lineage::{MAX_GENERATION, Origin};
use crate::models::work_batch::{self, BatchState, Priority};
use crate::realtime::EventBus;
use crate::test_support::{
    TEST_PERSON, TEST_PERSON_ID, fetched_agent_configuration, migrated_database,
};
use crate::tools::grants::Grants;

/// A project whose brief names the goal "Annual pricing", written by its own Project Manager, which
/// wakes nothing.
async fn project_named(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    let project_id = project::create(
        connection,
        &NewProject {
            name: name.to_owned(),
            description: format!("{name}, the project."),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id;
    let brief = project_brief::find(connection, project_id)
        .await
        .expect("every project has a brief");
    let its_own = Agent::project_manager(project_id);
    let recorded = publish_brief(
        connection,
        &brief,
        "## Goals\n\n### Annual pricing\n\nAda asked for it.",
        Author {
            user_id: PROJECT_MANAGER_USER_ID,
            agent: Some(&its_own),
            at: Utc::now(),
        },
    )
    .await
    .expect("its Project Manager writes it");
    assert_eq!(recorded.woke, None, "its own edit wakes it for nothing");
    project_id
}

/// Publishes `body` over `brief` as `author`, against the version it was read at.
async fn publish_brief(
    connection: &mut AsyncPgConnection,
    brief: &page::Page,
    body: &str,
    author: Author<'_>,
) -> Result<page::Recorded, PageError> {
    let edit = PageEdit {
        title: brief.title.clone(),
        body: body.to_owned(),
        learning: None,
    };
    page::publish(connection, brief.id, edit, brief.current_version, author).await
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
    let (url, mut connection) = migrated_database().await;
    fetched_agent_configuration(&mut connection).await;
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

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn no_turn_starts_without_the_configuration_repository_and_its_wakes_wait() {
    let (context, script, mut connection) = scripted(vec![ElysiaOutcome::Completed {
        result: Some("Planned.\nSummary: Planned annual pricing.".to_owned()),
        session_id: Some("session-1".to_owned()),
    }])
    .await;
    // The repository is named but its last fetch failed.
    let source = crate::runner::configuration::Source {
        repository: "https://github.com/acme/agents".to_owned(),
        github_credential_id: None,
    };
    crate::models::agent_configuration::record_failed(
        &mut connection,
        &source,
        "remote: Repository not found.",
        Utc::now(),
    )
    .await
    .expect("recorded");
    let launch = project_named(&mut connection, "Launch").await;
    let record = segment(&mut connection, "annual pricing").await;
    attribution::attribute(&mut connection, record, launch, 0.8, Attributor::Elysia)
        .await
        .expect("attributes");
    go_quiet(&mut connection).await;

    dispatch_once(&context).await;
    assert!(asked(&script).is_empty(), "no turn starts");
    assert_eq!(
        waiting_kinds(&mut connection, launch).await,
        [WakeKind::Record],
        "what woke it waits for the repository"
    );
    let status = crate::models::agent_configuration::find(&mut connection)
        .await
        .expect("status");
    assert!(status.inbox_item_id.is_some(), "one item says why");

    fetched_agent_configuration(&mut connection).await;
    dispatch_once(&context).await;
    assert_eq!(
        asked(&script).len(),
        1,
        "it starts once the repository is fetched"
    );
}

/// A page linked to a project, as the test person.
async fn linked_page(connection: &mut AsyncPgConnection, project_id: Uuid, title: &str) -> Uuid {
    let author = Author {
        user_id: TEST_PERSON_ID,
        agent: None,
        at: Utc::now(),
    };
    let edit = PageEdit {
        title: title.to_owned(),
        body: format!("{title}, in full."),
        learning: None,
    };
    let page_id = page::create(connection, None, edit, author)
        .await
        .expect("created")
        .page
        .id;
    page_project::link(connection, page_id, project_id, author)
        .await
        .expect("linked");
    page_id
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_page_linked_to_a_project_wakes_nothing_and_is_named_on_its_next_turn() {
    let (context, script, mut connection) = scripted(vec![ElysiaOutcome::Completed {
        result: Some("Noted.\nSummary: Read the checklist.".to_owned()),
        session_id: Some("session-1".to_owned()),
    }])
    .await;
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;
    let page_id = linked_page(&mut connection, launch, "Launch checklist").await;
    linked_page(&mut connection, website, "Website style guide").await;

    assert!(
        waiting_kinds(&mut connection, launch).await.is_empty(),
        "a link is no wake: nothing has to happen because of it"
    );

    let record = segment(&mut connection, "annual pricing").await;
    attribution::attribute(&mut connection, record, launch, 0.8, Attributor::Elysia)
        .await
        .expect("attributes");
    go_quiet(&mut connection).await;
    dispatch_once(&context).await;

    let turns = asked(&script);
    assert_eq!(turns.len(), 1);
    assert!(
        turns[0].prompt.contains("Launch checklist")
            && turns[0].prompt.contains(&page_id.to_string()),
        "the turn names the project's docs: {}",
        turns[0].prompt
    );
    assert!(
        !turns[0].prompt.contains("Website style guide"),
        "and no other project's"
    );

    let linked = page_project::linked_page(&mut connection, launch, page_id)
        .await
        .expect("reads")
        .expect("linked here");
    assert_eq!(linked.body, "Launch checklist, in full.");
    assert!(
        page_project::linked_page(&mut connection, website, page_id)
            .await
            .expect("reads")
            .is_none(),
        "a Project Manager reads its own project's pages"
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
        serves: "annual pricing".to_owned(),
        repositories: Vec::new(),
        priority: Priority::Asap,
        scheduled_for: None,
        record_ids,
        action_item_ids: Vec::new(),
        message_ids: Vec::new(),
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn only_the_lead_projects_project_manager_submits_a_batch_for_an_item() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;
    let mut both = vec![launch, website];
    both.sort_unstable();
    let now = Utc::now();
    let item = action_item::create(
        &mut connection,
        NewActionItem {
            title: "Fix the login".to_owned(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: both.clone(),
            initiative_ids: Vec::new(),
        },
        TEST_PERSON,
        now,
    )
    .await
    .expect("created")
    .record;
    // In both, led by the first.
    let (lead, other) = (both[0], both[1]);
    assert_eq!(item.lead_project_id, Some(lead));
    // Something on the other project wakes its Project Manager, whose turn submits.
    let record = segment(&mut connection, "the login").await;
    attribution::attribute(&mut connection, record, other, 0.8, Attributor::Elysia)
        .await
        .expect("attributes");
    let (turn, _taken) = project_manager::start_turn(&mut connection, other, 40)
        .await
        .expect("starts")
        .expect("a turn");
    let for_item = Submission {
        action_item_ids: vec![item.id],
        ..submission(Vec::new())
    };

    // The other project's Project Manager is told whose the item is and how that changes.
    let refused = batches::submit(&mut connection, &events, other, turn.id, for_item.clone())
        .await
        .expect_err("another project leads it");
    let BatchError::Refused(reason) = refused else {
        panic!("a refusal the Project Manager reads");
    };
    let lead_name = project::find(&mut connection, lead)
        .await
        .expect("a project")
        .name;
    assert!(reason.contains(&format!("led by the project {lead_name}")));

    // Once a person makes its project the lead, its batch is taken.
    action_item_attribution::set_lead(&mut connection, item.id, other, TEST_PERSON, now)
        .await
        .expect("led");

    // An item it leads never vouches for the rest: a message sent to another project's Project
    // Manager is still refused beside it.
    let elsewhere =
        project_manager::wake_with_message(&mut connection, lead, "Fix it.", TEST_PERSON_ID)
            .await
            .expect("woken");
    let borrowing = Submission {
        message_ids: vec![elsewhere.id],
        ..for_item.clone()
    };
    let refused = batches::submit(&mut connection, &events, other, turn.id, borrowing)
        .await
        .expect_err("another project's message");
    let BatchError::Refused(reason) = refused else {
        panic!("a refusal the Project Manager reads");
    };
    assert!(reason.contains("not one sent to this project's Project Manager"));

    let batch = batches::submit(&mut connection, &events, other, turn.id, for_item)
        .await
        .expect("submitted");
    assert_eq!(batch.action_items(), vec![item.id]);
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
    assert_eq!(batch.state, BatchState::Proposed);
    assert_eq!(batch.turn_id, Some(turn.id));
    assert_eq!(batch.source_records(), vec![record]);
    assert_eq!(
        batch.serves.as_deref(),
        Some("Annual pricing"),
        "the goal is stored as the brief spells it"
    );
    assert_eq!(batch.origin, Origin::HumanRooted);
    assert_eq!(batch.generation, 0);
    let on_timeline = timeline_record::attributed_among(
        &mut connection,
        launch,
        &[batch.timeline_record_id.expect("a record")],
    )
    .await
    .expect("reads");
    assert_eq!(on_timeline[0].0.title, "Annual pricing");
    assert_eq!(on_timeline[0].0.metadata["priority"], "asap");
    assert_eq!(on_timeline[0].0.metadata["serves"], "Annual pricing");

    // Proposed, the pool hands it nothing. Once a person approves it, the pool hands it a slot,
    // and a worker takes it and finishes it: its Project Manager wakes with the result.
    let nothing = work_batch::assign(&mut connection, &[batch.id], 1, Utc::now())
        .await
        .expect("looks");
    assert!(
        nothing.is_empty(),
        "a proposed batch is never handed a slot"
    );
    work_batch::approval::approve(
        &mut connection,
        batch.id,
        work_batch::approval::Edits::default(),
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect("approved");
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
async fn a_scheduled_batch_is_stored_with_its_time_and_a_proposed_one_can_be_withdrawn() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let asked =
        project_manager::wake_with_message(&mut connection, launch, "Plan.", TEST_PERSON_ID)
            .await
            .expect("woken");
    let (turn, _taken) = project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("a turn");

    let due = Utc::now() + TimeDelta::days(1);
    let mut scheduled = submission(Vec::new());
    scheduled.message_ids = vec![asked.id];
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

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_batch_outside_the_brief_is_refused_naming_the_goals_there_are() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let (turn, _taken) = {
        project_manager::wake_with_message(&mut connection, launch, "Plan.", TEST_PERSON_ID)
            .await
            .expect("woken");
        project_manager::start_turn(&mut connection, launch, 40)
            .await
            .expect("starts")
            .expect("a turn")
    };

    let mut outside = submission(Vec::new());
    outside.serves = "Dark mode".to_owned();
    let refused = batches::submit(&mut connection, &events, launch, turn.id, outside)
        .await
        .expect_err("no such goal");
    assert!(
        matches!(&refused, BatchError::Refused(reason)
            if reason.contains("\"Dark mode\" is not a goal")
                && reason.contains("\"Annual pricing\"")
                && reason.contains("ask the person")),
        "{refused:?}"
    );

    // A project whose brief is still the skeleton names no goals, so nothing is submitted.
    let website = project::create(
        &mut connection,
        &NewProject {
            name: "Website".to_owned(),
            description: String::new(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a project")
    .id;
    project_manager::wake_with_message(&mut connection, website, "Plan.", TEST_PERSON_ID)
        .await
        .expect("woken");
    let (website_turn, _taken) = project_manager::start_turn(&mut connection, website, 40)
        .await
        .expect("starts")
        .expect("a turn");
    let refused = batches::submit(
        &mut connection,
        &events,
        website,
        website_turn.id,
        submission(Vec::new()),
    )
    .await
    .expect_err("no goals yet");
    assert!(
        matches!(&refused, BatchError::Refused(reason) if reason.contains("names no goals yet")),
        "{refused:?}"
    );
    let stored = work_batch::list_for_project(&mut connection, website, &[], 10)
        .await
        .expect("lists");
    assert!(stored.is_empty(), "a refused batch stores nothing");
}

/// A person approves batch `batch_id`, the pool hands it a slot, and a worker starts it and
/// finishes it reporting `result`, which wakes its Project Manager with what the worker did.
async fn worked_and_finished(connection: &mut AsyncPgConnection, batch_id: Uuid, result: &str) {
    work_batch::approval::approve(
        connection,
        batch_id,
        work_batch::approval::Edits::default(),
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect("approved");
    let slot = work_batch::assign(connection, &[batch_id], 1, Utc::now())
        .await
        .expect("assigns")
        .into_iter()
        .next()
        .expect("a slot");
    work_batch::start(connection, &slot)
        .await
        .expect("starts")
        .expect("it was assigned");
    work_batch::finish(connection, &slot, true, result, CODING_AGENT_USER_ID)
        .await
        .expect("finishes")
        .expect("it was running");
}

/// Ends turn `turn_id` and starts `project_id`'s next, which takes whatever woke it since.
async fn next_turn(connection: &mut AsyncPgConnection, project_id: Uuid, turn_id: Uuid) -> Uuid {
    let done = project_manager::Finished::Succeeded {
        summary: None,
        reply: None,
        timeline_record_id: None,
    };
    project_manager::finish_turn(connection, turn_id, done, "")
        .await
        .expect("finished");
    project_manager::start_turn(connection, project_id, 40)
        .await
        .expect("starts")
        .expect("something woke it")
        .0
        .id
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_workers_follow_up_is_found_work_that_cannot_queue_itself() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let record = segment(&mut connection, "annual pricing").await;
    attribution::attribute(&mut connection, record, launch, 0.8, Attributor::Elysia)
        .await
        .expect("attributes");
    let (turn, _taken) = project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("a turn");

    // The meeting asked for it: a person's request, and nothing led to it.
    let asked = batches::submit(
        &mut connection,
        &events,
        launch,
        turn.id,
        submission(vec![record]),
    )
    .await
    .expect("submitted");
    assert_eq!(asked.origin, Origin::HumanRooted);
    assert_eq!(asked.generation, 0);
    assert!(asked.parents().is_empty());

    // Its worker turns up more work as it finishes, which wakes the Project Manager.
    worked_and_finished(
        &mut connection,
        asked.id,
        "Annual pricing is live. The invoices page still shows monthly prices; it should follow.",
    )
    .await;
    let after_the_result = next_turn(&mut connection, launch, turn.id).await;

    // What the worker turned up, naming only the batch it came from, is found work, one
    // generation on. It is proposed, and the pool never hands it a slot.
    let mut follow_up = submission(vec![asked.timeline_record_id.expect("on the timeline")]);
    follow_up.title = "Annual prices on the invoices page".to_owned();
    let found = batches::submit(
        &mut connection,
        &events,
        launch,
        after_the_result,
        follow_up,
    )
    .await
    .expect("submitted");
    assert_eq!(found.origin, Origin::Found);
    assert_eq!(found.generation, 1);
    assert_eq!(found.parents(), vec![asked.id]);
    assert_eq!(found.state, BatchState::Proposed);
    let handed = work_batch::assign(&mut connection, &[found.id], 4, Utc::now())
        .await
        .expect("looks");
    assert!(handed.is_empty(), "a follow-up never queues itself");
    let queue = crate::worker_pool::queue::read(&mut connection, Utc::now())
        .await
        .expect("reads");
    assert!(queue.proposed.iter().any(|batch| batch.id == found.id));
    assert!(!queue.next.iter().any(|batch| batch.id == found.id));

    // Naming the meeting that asked for the first batch does not hide what led to this one.
    let named_the_meeting = batches::submit(
        &mut connection,
        &events,
        launch,
        after_the_result,
        submission(vec![record]),
    )
    .await
    .expect("submitted");
    assert_eq!(named_the_meeting.origin, Origin::HumanRooted);
    assert_eq!(named_the_meeting.generation, 1);
    assert_eq!(named_the_meeting.parents(), vec![asked.id]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_chain_of_follow_ups_stops_at_the_generation_limit() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let record = segment(&mut connection, "annual pricing").await;
    attribution::attribute(&mut connection, record, launch, 0.8, Attributor::Elysia)
        .await
        .expect("attributes");
    let mut turn_id = project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("a turn")
        .0
        .id;
    let mut batch = batches::submit(
        &mut connection,
        &events,
        launch,
        turn_id,
        submission(vec![record]),
    )
    .await
    .expect("submitted");

    // Each batch planned from what the one before's worker did is one generation deeper.
    for generation in 1..=MAX_GENERATION {
        worked_and_finished(&mut connection, batch.id, "Done, and more turned up.").await;
        turn_id = next_turn(&mut connection, launch, turn_id).await;
        batch = batches::submit(
            &mut connection,
            &events,
            launch,
            turn_id,
            submission(vec![record]),
        )
        .await
        .expect("within the limit");
        assert_eq!(batch.generation, generation);
        assert_eq!(batch.origin, Origin::HumanRooted);
    }

    // One more is refused, saying why and what to do, and nothing is stored.
    worked_and_finished(&mut connection, batch.id, "Done, and more turned up.").await;
    // A person writes in the same moment, so one debounced turn takes both.
    let asked = project_manager::wake_with_message(
        &mut connection,
        launch,
        "Keep going on pricing.",
        TEST_PERSON_ID,
    )
    .await
    .expect("woken");
    turn_id = next_turn(&mut connection, launch, turn_id).await;
    let stored = work_batch::list_for_project(&mut connection, launch, &[], 50)
        .await
        .expect("lists")
        .len();
    let refused = batches::submit(
        &mut connection,
        &events,
        launch,
        turn_id,
        submission(vec![record]),
    )
    .await
    .expect_err("past the limit");
    let BatchError::Refused(reason) = refused else {
        panic!("a refusal the Project Manager is told, not {refused:?}");
    };
    assert!(
        reason.contains(&format!("generation {}", MAX_GENERATION + 1)),
        "{reason}"
    );
    assert!(reason.contains(&batch.id.to_string()), "{reason}");
    assert!(reason.contains("so the person decides"), "{reason}");
    let still_stored = work_batch::list_for_project(&mut connection, launch, &[], 50)
        .await
        .expect("lists")
        .len();
    assert_eq!(still_stored, stored);

    // Answering the person's own words starts the chain again, though the batch's result woke the
    // same turn: what they asked for is not the batch's follow-up.
    let mut by_request = submission(Vec::new());
    by_request.message_ids = vec![asked.id];
    let restarted = batches::submit(&mut connection, &events, launch, turn_id, by_request)
        .await
        .expect("submitted");
    assert_eq!(restarted.origin, Origin::HumanRooted);
    assert_eq!(restarted.generation, 0);
    assert!(restarted.parents().is_empty());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_project_manager_cannot_claim_a_human_origin_it_cannot_trace() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let website = project_named(&mut connection, "Website").await;
    let relayed = project_manager::wake_with_message(
        &mut connection,
        launch,
        "Leadership would like the invoices redone.",
        ELYSIA_USER_ID,
    )
    .await
    .expect("woken");
    let elsewhere = project_manager::wake_with_message(
        &mut connection,
        website,
        "Redo the footer.",
        TEST_PERSON_ID,
    )
    .await
    .expect("woken");
    let (turn, _taken) = project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("a turn");
    let refusal = |error: BatchError| match error {
        BatchError::Refused(reason) => reason,
        BatchError::Database(error) => panic!("a refusal, not {error}"),
    };

    // Naming nothing is no origin at all.
    let nothing = batches::submit(
        &mut connection,
        &events,
        launch,
        turn.id,
        submission(Vec::new()),
    )
    .await
    .expect_err("names nothing");
    assert!(refusal(nothing).contains("name what called for this batch"));

    // A person's message to another project's Project Manager is not this one's to name.
    let mut borrowed = submission(Vec::new());
    borrowed.message_ids = vec![elsewhere.id];
    let borrowed = batches::submit(&mut connection, &events, launch, turn.id, borrowed)
        .await
        .expect_err("another project's message");
    assert!(refusal(borrowed).contains("not one sent to this project's Project Manager"));

    // Elysia's word is an agent's: work only she asked for is found.
    let mut hearsay = submission(Vec::new());
    hearsay.message_ids = vec![relayed.id];
    let hearsay = batches::submit(&mut connection, &events, launch, turn.id, hearsay)
        .await
        .expect("submitted");
    assert_eq!(hearsay.origin, Origin::Found);

    // So is the Project Manager's own record, and an item it wrote that no person accepted.
    let own_record = submission(vec![hearsay.timeline_record_id.expect("on the timeline")]);
    let own_record = batches::submit(&mut connection, &events, launch, turn.id, own_record)
        .await
        .expect("submitted");
    assert_eq!(own_record.origin, Origin::Found);
    let written = crate::models::action_item::create(
        &mut connection,
        crate::models::action_item::NewActionItem {
            title: "Redo the invoices".to_owned(),
            notes: String::new(),
            state: crate::models::action_item::ActionItemState::Inbox,
            priority: crate::models::action_item::ActionItemPriority::Normal,
            due_at: None,
            owner: crate::models::action_item::Owner::User,
            project_ids: vec![launch],
            initiative_ids: Vec::new(),
        },
        crate::action_items::Actor::ProjectManager(launch),
        Utc::now(),
    )
    .await
    .expect("written")
    .record
    .id;
    let mut own_item = submission(Vec::new());
    own_item.action_item_ids = vec![written];
    let own_item = batches::submit(&mut connection, &events, launch, turn.id, own_item)
        .await
        .expect("submitted");
    assert_eq!(own_item.origin, Origin::Found);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_persons_edit_of_the_brief_wakes_its_project_manager_and_wins_over_a_stale_save() {
    let (_url, mut connection) = migrated_database().await;
    let launch = project_named(&mut connection, "Launch").await;
    assert!(
        waiting_kinds(&mut connection, launch).await.is_empty(),
        "creating the project and its Project Manager's own edit wake nothing"
    );

    // The Project Manager read version 2; a person publishes version 3 meanwhile.
    let read = project_brief::find(&mut connection, launch)
        .await
        .expect("a brief");
    let person = Author {
        user_id: TEST_PERSON_ID,
        agent: None,
        at: Utc::now(),
    };
    let edited = publish_brief(
        &mut connection,
        &read,
        "## Goals\n\n### Annual pricing\n\n## Out of scope\n\nMonthly discounts.",
        person,
    )
    .await
    .expect("a person edits it");
    assert_eq!(edited.woke, Some(launch));
    let wakes: Vec<(WakeKind, Option<i32>, Uuid)> = project_manager_wakes::table
        .filter(project_manager_wakes::project_id.eq(launch))
        .filter(project_manager_wakes::turn_id.is_null())
        .select((
            project_manager_wakes::kind,
            project_manager_wakes::page_version,
            project_manager_wakes::created_by,
        ))
        .load(&mut connection)
        .await
        .expect("wakes");
    assert_eq!(wakes, vec![(WakeKind::Brief, Some(3), TEST_PERSON_ID)]);

    // Its save against the version it read is refused, and nothing the person wrote is lost.
    let its_own = Agent::project_manager(launch);
    let stale = publish_brief(
        &mut connection,
        &read,
        "## Goals\n\n### Annual pricing\n\n## Decisions\n\nShip in March.",
        Author {
            user_id: PROJECT_MANAGER_USER_ID,
            agent: Some(&its_own),
            at: Utc::now(),
        },
    )
    .await
    .expect_err("a version was published since");
    assert!(matches!(stale, PageError::Changed), "{stale:?}");
    let brief = project_brief::find(&mut connection, launch)
        .await
        .expect("a brief");
    assert_eq!(brief.current_version, 3);
    assert!(brief.body.contains("Monthly discounts."));

    // A person restoring an earlier version is an edit too.
    let restored = page::restore_version(&mut connection, brief.id, 2, person)
        .await
        .expect("restores");
    assert_eq!(restored.woke, Some(launch));
    assert_eq!(
        waiting_kinds(&mut connection, launch).await,
        vec![WakeKind::Brief, WakeKind::Brief]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn every_turn_carries_the_brief_and_names_who_edited_it() {
    let (context, script, mut connection) = scripted(vec![ElysiaOutcome::Completed {
        result: Some("Read the brief.\nSummary: Followed the new scope.".to_owned()),
        session_id: Some("session-1".to_owned()),
    }])
    .await;
    let launch = project_named(&mut connection, "Launch").await;
    let brief = project_brief::find(&mut connection, launch)
        .await
        .expect("a brief");
    publish_brief(
        &mut connection,
        &brief,
        "## Goals\n\n### Annual pricing\n\n## Out of scope\n\nMonthly discounts.",
        Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: Utc::now(),
        },
    )
    .await
    .expect("a person edits it");
    go_quiet(&mut connection).await;

    dispatch_once(&context).await;

    let turns = asked(&script);
    let prompt = &turns[0].prompt;
    assert!(prompt.contains(&format!(
        "## The project's brief (page {}, version 3)",
        brief.id
    )));
    assert!(prompt.contains("Monthly discounts."));
    assert!(prompt.contains("## The brief changed"));
    assert!(prompt.contains("published version 3"));
    assert!(
        !prompt.contains(&format!("- {} (page", brief.title)),
        "the brief is carried whole, not listed again among the docs"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_rejected_batch_is_replaced_where_it_stood() {
    let (_url, mut connection) = migrated_database().await;
    let events = EventBus::in_process();
    let launch = project_named(&mut connection, "Launch").await;
    let record = segment(&mut connection, "annual pricing").await;
    attribution::attribute(&mut connection, record, launch, 0.8, Attributor::Elysia)
        .await
        .expect("attributes");
    let (turn, _taken) = project_manager::start_turn(&mut connection, launch, 40)
        .await
        .expect("starts")
        .expect("a turn");
    let first = batches::submit(
        &mut connection,
        &events,
        launch,
        turn.id,
        submission(vec![record]),
    )
    .await
    .expect("submitted");

    // A person's rejection wakes the Project Manager, which plans the work again: no worker did
    // anything, so the replacement stands where the first stood.
    work_batch::approval::reject(
        &mut connection,
        first.id,
        "Only the yearly plan, not every plan.",
        TEST_PERSON_ID,
        Utc::now(),
    )
    .await
    .expect("rejected");
    let after_the_rejection = next_turn(&mut connection, launch, turn.id).await;
    let replacement = batches::submit(
        &mut connection,
        &events,
        launch,
        after_the_rejection,
        submission(vec![record]),
    )
    .await
    .expect("submitted");
    assert_eq!(replacement.generation, 0);
    assert_eq!(replacement.parents(), vec![first.id]);
}
