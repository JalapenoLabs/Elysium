// Copyright © 2026 Jalapeno Labs

//! Findings end to end: what an agent may report and what it is told back, workers and Project
//! Managers reporting through their own tools, a person accepting one into an item, its issue,
//! and a proposed batch rooted in them, and dismissing one for good.
//!
//! The database tests are `#[ignore]`d so `cargo test` stays hermetic;
//! `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use secrecy::ExposeSecret as _;
use tower::ServiceExt as _;

use super::*;
use crate::action_items::links::tests::LinkFixture;
use crate::action_items::watcher::pass;
use crate::database::schema::work_batch_approval_items;
use crate::models::action_item_link::{self, LinkProvider};
use crate::models::project::{self, NewProject};
use crate::models::project_issue_tracker::{self, NewTracker};
use crate::models::timeline_record;
use crate::models::user::{self, CODING_AGENT_USER_ID, PROJECT_MANAGER_USER_ID};
use crate::models::work_batch::lineage::Origin;
use crate::models::work_batch::{self, BatchState, WorkBatch};
use crate::project_managers::trackers::NO_TRACKER;
use crate::routes;
use crate::runner::McpServer;
use crate::state::AppState;
use crate::test_support::{
    TEST_PERSON_ID, app_state, approved_batch, migrated_database, person_rooted,
};
use crate::tools::{CallScope, ProjectManagerScope, ToolServer, WorkerScope};

fn report_of(title: &str, repository_url: Option<&str>) -> Report {
    Report {
        title: title.to_owned(),
        detail: "The retry loop never gives up when the provider answers 429, so a worker \
            stalls until its lease lapses. Give it a ceiling and say so in the log."
            .to_owned(),
        repository_url: repository_url.map(str::to_owned),
        location: "api/src/retry.rs".to_owned(),
        cost: Cost::Small,
    }
}

#[test]
fn a_report_is_trimmed_and_held_to_its_columns() {
    let mut report = report_of("  Retry forever  ", Some(" https://github.com/acme/api "));
    report.location = " api/src/retry.rs \n".to_owned();
    let checked = check(report).expect("a good report");
    assert_eq!(checked.title, "Retry forever");
    assert_eq!(checked.location, "api/src/retry.rs");
    assert_eq!(
        checked.repository_url.as_deref(),
        Some("https://github.com/acme/api")
    );

    let mut blank = report_of("   ", None);
    blank.location = "src".to_owned();
    assert!(check(blank).is_err_and(|reason| reason.contains("title")));

    let mut long = report_of("Retry forever", None);
    long.detail = "a".repeat(DETAIL_MAX_CHARACTERS + 1);
    assert!(check(long).is_err_and(|reason| reason.contains("detail")));

    let unclonable = report_of("Retry forever", Some("file:///etc/passwd"));
    assert!(check(unclonable).is_err_and(|reason| reason.contains("repositoryUrl")));

    // A blank repository is no repository, rather than a refusal.
    let unnamed = report_of("Retry forever", Some("  "));
    assert_eq!(check(unnamed).expect("a good report").repository_url, None);
}

#[test]
fn the_report_schema_offers_every_cost_the_report_reads() {
    let schema = report_schema();
    for cost in schema["properties"]["cost"]["enum"]
        .as_array()
        .expect("an enum")
    {
        let parsed: Cost = serde_json::from_value(cost.clone()).expect("every listed cost reads");
        assert_eq!(json!(parsed), *cost);
    }
}

fn finding_in(state: FindingState) -> Finding {
    let now = Utc::now();
    let decided = state != FindingState::Open;
    Finding {
        id: Uuid::now_v7(),
        project_id: Uuid::now_v7(),
        fingerprint: "0".repeat(64),
        title: "Retry forever".to_owned(),
        detail: "It never gives up.".to_owned(),
        repository_url: None,
        location: "api/src/retry.rs".to_owned(),
        cost: Cost::Small,
        state,
        work_batch_id: None,
        project_manager_turn_id: None,
        reports: 2,
        last_reported_at: now,
        decided_by: decided.then_some(TEST_PERSON_ID),
        decided_at: decided.then_some(now),
        dismissal_reason: (state == FindingState::Dismissed)
            .then(|| "The provider never answers 429.".to_owned()),
        action_item_id: (state == FindingState::Accepted).then(Uuid::now_v7),
        accepted_batch_id: (state == FindingState::Accepted).then(Uuid::now_v7),
        created_by: CODING_AGENT_USER_ID,
        created_at: now,
    }
}

#[test]
fn an_agent_is_told_to_leave_a_finding_to_the_person_who_decides_it() {
    let new = answer(&Reported {
        finding: finding_in(FindingState::Open),
        is_new: true,
    });
    let next = new["next"].as_str().expect("words");
    assert!(next.contains("Do not file an issue"), "{next}");

    let accepted_finding = finding_in(FindingState::Accepted);
    let batch_id = accepted_finding.accepted_batch_id.expect("a batch");
    let accepted = answer(&Reported {
        finding: accepted_finding,
        is_new: false,
    });
    let next = accepted["next"].as_str().expect("words");
    assert!(next.contains(&batch_id.to_string()), "{next}");

    let dismissed = answer(&Reported {
        finding: finding_in(FindingState::Dismissed),
        is_new: false,
    });
    let next = dismissed["next"].as_str().expect("words");
    assert!(next.contains("The provider never answers 429."), "{next}");
    assert!(next.contains("Do not report it again"), "{next}");
    assert_eq!(dismissed["finding"]["state"], "dismissed");
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

async fn test_person(connection: &mut AsyncPgConnection) -> MemberUser {
    let person = user::find(connection, TEST_PERSON_ID)
        .await
        .expect("the test person");
    MemberUser::check(person).expect("a member")
}

/// A batch on `project_id` approved for a slot, which no worker has started.
async fn approved(connection: &mut AsyncPgConnection, project_id: Uuid) -> WorkBatch {
    approved_batch(
        connection,
        NewWorkBatch {
            id: Uuid::now_v7(),
            project_id,
            title: "Annual pricing".to_owned(),
            goal: "Show annual pricing on the pricing page.".to_owned(),
            repositories: json!([]),
            priority: Priority::Normal,
            scheduled_for: None,
            turn_id: None,
            source_record_ids: json!([]),
            action_item_ids: json!([]),
            source_message_ids: json!([]),
            lineage: person_rooted(),
            created_by: PROJECT_MANAGER_USER_ID,
        },
    )
    .await
}

/// The item asking a person to approve `project_id`'s proposed batches.
async fn approval_item_of(connection: &mut AsyncPgConnection, project_id: Uuid) -> ActionItem {
    let item_id: Uuid = work_batch_approval_items::table
        .find(project_id)
        .select(work_batch_approval_items::action_item_id)
        .first(connection)
        .await
        .expect("the project's approval item");
    action_item::find(connection, item_id).await.expect("read")
}

/// A project of the fixture's that files its issues in Jira's `ELY`.
async fn project_filing_in_jira(fixture: &mut LinkFixture) -> Uuid {
    let project_id = project_named(&mut fixture.connection, "Launch").await;
    project_issue_tracker::set(
        &mut fixture.connection,
        &NewTracker {
            project_id,
            provider: LinkProvider::Jira,
            jira_credential_id: Some(fixture.jira_credential.id),
            token_account_id: None,
            destination: "ELY".to_owned(),
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("named");
    project_id
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn accepting_files_its_issue_and_proposes_a_batch_rooted_in_the_person() {
    let mut fixture = LinkFixture::start().await;
    let project_id = project_filing_in_jira(&mut fixture).await;
    let reporting_batch = approved(&mut fixture.connection, project_id).await;
    let reported = report(
        &mut fixture.connection,
        &fixture.events,
        project_id,
        Reporter::Worker {
            batch_id: reporting_batch.id,
        },
        report_of("Retry forever", Some("https://github.com/acme/api")),
    )
    .await
    .expect("reported");

    let person = test_person(&mut fixture.connection).await;
    let accepted = accept(
        &mut fixture.connection,
        reported.finding.id,
        &person,
        Utc::now(),
    )
    .await
    .expect("accepted");

    assert_eq!(accepted.finding.state, FindingState::Accepted);
    assert_eq!(accepted.finding.decided_by, Some(TEST_PERSON_ID));
    let item = &accepted.item.record;
    assert_eq!(accepted.finding.action_item_id, Some(item.id));
    assert_eq!(
        item.state,
        ActionItemState::Open,
        "a person accepting it accepts the item"
    );
    assert_eq!(item.created_by, TEST_PERSON_ID);
    assert_eq!(
        item.lead_project_id,
        Some(project_id),
        "its project leads it, so its Project Manager works it"
    );
    assert!(
        item.notes
            .contains("api/src/retry.rs in https://github.com/acme/api")
    );
    let memberships = action_item::memberships(&mut fixture.connection, &[item.id])
        .await
        .expect("memberships");
    assert_eq!(memberships.project_ids(item.id), vec![project_id]);

    let batch = accepted.batch.batch();
    assert_eq!(accepted.finding.accepted_batch_id, Some(batch.id));
    assert_eq!(
        batch.state,
        BatchState::Proposed,
        "a person still approves its batch"
    );
    assert_eq!(batch.origin, Origin::HumanRooted);
    assert_eq!(batch.generation, 0, "accepting starts the chain again");
    assert_eq!(batch.created_by, TEST_PERSON_ID);
    assert_eq!(batch.action_item_ids, json!([item.id]));
    assert_eq!(
        batch.repositories,
        json!([{ "url": "https://github.com/acme/api" }])
    );
    let record_id = batch.timeline_record_id.expect("on the timeline");
    let record = timeline_record::find_many(&mut fixture.connection, &[record_id])
        .await
        .expect("read")
        .remove(0);
    assert_eq!(
        record.actor, "Tester",
        "the batch's record names who accepted it"
    );
    let approval_item = approval_item_of(&mut fixture.connection, project_id).await;
    assert_eq!(
        (approval_item.state, approval_item.created_by),
        (ActionItemState::Inbox, PROJECT_MANAGER_USER_ID),
        "the approval item asks the person, raised by the Project Manager as always"
    );

    let Ok(place) = &accepted.issue else {
        panic!("the project names Jira, so the issue is owed there");
    };
    assert_eq!(place.destination, "ELY");
    let filing = action_item_issue_filing::for_item(&mut fixture.connection, item.id)
        .await
        .expect("read")
        .expect("owed");
    assert_eq!(filing.filed_by, format!("user:{TEST_PERSON_ID}"));

    pass(&fixture.context()).await;
    let links = action_item_link::list_for_item(&mut fixture.connection, item.id)
        .await
        .expect("links");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].external_key, "ELY-1");
    assert!(links[0].is_primary);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_project_with_nowhere_to_file_still_gets_the_item_and_its_batch() {
    let (_url, mut connection) = migrated_database().await;
    let project_id = project_named(&mut connection, "Launch").await;
    let events = EventBus::in_process();
    let reporting_batch = approved(&mut connection, project_id).await;
    let worker = Reporter::Worker {
        batch_id: reporting_batch.id,
    };
    let reported = report(
        &mut connection,
        &events,
        project_id,
        worker,
        report_of("Retry forever", None),
    )
    .await
    .expect("reported");

    let person = test_person(&mut connection).await;
    let accepted = accept(&mut connection, reported.finding.id, &person, Utc::now())
        .await
        .expect("accepted");
    assert_eq!(accepted.issue, Err(NO_TRACKER));
    let item_id = accepted.item.record.id;
    assert!(
        action_item_issue_filing::for_item(&mut connection, item_id)
            .await
            .expect("read")
            .is_none()
    );
    let batch = accepted.batch.batch();
    assert_eq!(batch.state, BatchState::Proposed);
    assert_eq!(batch.repositories, json!([]), "no repository was named");

    let twice = accept(&mut connection, reported.finding.id, &person, Utc::now())
        .await
        .expect_err("decided once");
    assert!(matches!(
        twice,
        FindingError::Decide(DecideError::Decided(FindingState::Accepted))
    ));

    // The worker sees it again: it is told the work is already a batch's.
    let again = report(
        &mut connection,
        &events,
        project_id,
        worker,
        report_of("Retry forever", None),
    )
    .await
    .expect("reported");
    let next = answer(&again)["next"].as_str().expect("words").to_owned();
    assert!(next.contains(&batch.id.to_string()), "{next}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_dismissed_finding_is_never_raised_again_and_says_why() {
    let (_url, mut connection) = migrated_database().await;
    let project_id = project_named(&mut connection, "Launch").await;
    let events = EventBus::in_process();
    let reporting_batch = approved(&mut connection, project_id).await;
    let worker = Reporter::Worker {
        batch_id: reporting_batch.id,
    };
    let reported = report(
        &mut connection,
        &events,
        project_id,
        worker,
        report_of("Retry forever", None),
    )
    .await
    .expect("reported");
    let person = test_person(&mut connection).await;

    let too_long = "a".repeat(DISMISSAL_REASON_MAX_CHARACTERS + 1);
    let refused = dismiss(
        &mut connection,
        reported.finding.id,
        &person,
        Some(&too_long),
        Utc::now(),
    )
    .await
    .expect_err("a reason has a limit");
    assert!(matches!(refused, FindingError::Invalid(_)));

    let dismissed = dismiss(
        &mut connection,
        reported.finding.id,
        &person,
        Some("  The provider never answers 429.  "),
        Utc::now(),
    )
    .await
    .expect("dismissed");
    assert_eq!(dismissed.state, FindingState::Dismissed);
    assert_eq!(
        dismissed.dismissal_reason.as_deref(),
        Some("The provider never answers 429.")
    );

    let again = report(
        &mut connection,
        &events,
        project_id,
        worker,
        report_of("retry forever", None),
    )
    .await
    .expect("reported");
    assert!(!again.is_new);
    let next = answer(&again)["next"].as_str().expect("words").to_owned();
    assert!(next.contains("The provider never answers 429."), "{next}");
    assert_eq!(
        finding::count_open(&mut connection, project_id)
            .await
            .expect("counted"),
        0
    );

    // Dismissing without a reason is just as final.
    let other = report(
        &mut connection,
        &events,
        project_id,
        worker,
        report_of("No docs for the pricing page", None),
    )
    .await
    .expect("reported");
    let dismissed = dismiss(
        &mut connection,
        other.finding.id,
        &person,
        Some("  "),
        Utc::now(),
    )
    .await
    .expect("dismissed");
    assert_eq!(dismissed.dismissal_reason, None);
}

/// What an agent reads from calling `tool` on `server` with `arguments` under `scope`: whether it
/// failed, and the text.
async fn call(
    state: &AppState,
    scope: &CallScope,
    server: &'static ToolServer,
    tool: &str,
    arguments: Value,
) -> (bool, String) {
    let grant = state
        .grants
        .grant(scope, &[server])
        .await
        .expect("a grant is stored");
    let McpServer::Http { headers, .. } = &grant.mcp_servers()[0] else {
        panic!("tools are served over HTTP");
    };
    let bearer = headers[0].1.expose_secret().to_owned();
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/agent-tools/{}", server.name))
        .header(header::HOST, "api:8080")
        .header(header::AUTHORIZATION, bearer)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .body(Body::from(body.to_string()))
        .expect("a request builds");
    let response = routes::router(state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    assert_eq!(response.status(), StatusCode::OK, "{tool}");
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    let answer: Value = serde_json::from_slice(&bytes).expect("the server answers JSON");
    let result = &answer["result"];
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{tool} answers text: {answer}"))
        .to_owned();
    (result["isError"] == true, text)
}

fn tool_arguments(title: &str) -> Value {
    json!({
        "title": title,
        "detail": "The retry loop never gives up when the provider answers 429.",
        "location": "api/src/retry.rs:40",
        "cost": "small",
    })
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_running_worker_reports_through_its_tool_and_a_finished_one_cannot() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let project_id = project_named(&mut connection, "Launch").await;
    let batch = approved(&mut connection, project_id).await;
    let scope = CallScope::Worker(WorkerScope {
        batch_id: batch.id,
        project_id,
    });

    let (failed, text) = call(
        &state,
        &scope,
        &crate::tools::worker::SERVER,
        "worker_report_finding",
        tool_arguments("Retry forever"),
    )
    .await;
    assert!(failed, "a batch no worker runs reports nothing: {text}");
    assert!(text.contains("no longer running"), "{text}");

    let slot = work_batch::assign(&mut connection, &[batch.id], 1, Utc::now())
        .await
        .expect("assigns")
        .remove(0);
    work_batch::start(&mut connection, &slot)
        .await
        .expect("runs")
        .expect("started");
    let (failed, text) = call(
        &state,
        &scope,
        &crate::tools::worker::SERVER,
        "worker_report_finding",
        tool_arguments("Retry forever"),
    )
    .await;
    assert!(!failed, "{text}");
    let answered: Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(answered["finding"]["state"], "open");
    let listed = finding::list_open(&mut connection, project_id)
        .await
        .expect("listed");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].work_batch_id, Some(batch.id));

    let mut unknown_field = tool_arguments("Retry forever");
    unknown_field["origin"] = json!("human_rooted");
    let (failed, text) = call(
        &state,
        &scope,
        &crate::tools::worker::SERVER,
        "worker_report_finding",
        unknown_field,
    )
    .await;
    assert!(
        failed,
        "a report says what was seen and nothing more: {text}"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_project_manager_reports_through_its_tool_and_learns_it_was_seen_before() {
    use crate::models::project_manager;

    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let project_id = project_named(&mut connection, "Launch").await;
    project_manager::wake_with_message(&mut connection, project_id, "Plan it.", TEST_PERSON_ID)
        .await
        .expect("woken");
    let (turn, _wakes) = project_manager::start_turn(&mut connection, project_id, 40)
        .await
        .expect("runs")
        .expect("a turn");
    let scope = CallScope::ProjectManager(ProjectManagerScope {
        project_id,
        turn_id: turn.id,
    });

    let server = &crate::tools::project_manager::SERVER;
    let (failed, text) = call(
        &state,
        &scope,
        server,
        "manager_report_finding",
        tool_arguments("Retry forever"),
    )
    .await;
    assert!(!failed, "{text}");
    let (failed, text) = call(
        &state,
        &scope,
        server,
        "manager_report_finding",
        tool_arguments("Retry  forever!"),
    )
    .await;
    assert!(!failed, "{text}");
    let answered: Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(answered["finding"]["reports"], 2);
    assert!(
        answered["next"]
            .as_str()
            .is_some_and(|next| next.contains("already on the project's Findings list"))
    );
    let listed = finding::list_open(&mut connection, project_id)
        .await
        .expect("listed");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].project_manager_turn_id, Some(turn.id));
    assert_eq!(listed[0].created_by, PROJECT_MANAGER_USER_ID);
}
