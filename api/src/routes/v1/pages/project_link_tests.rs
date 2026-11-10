// Copyright © 2026 Jalapeno Labs

//! Links between pages and projects against Postgres (#206): any number of them, recorded in a
//! page's history and never as a version, by people and agents, a project listing its live pages,
//! a deleted project leaving each page's history saying so, and guests refused.
//!
//! Those are `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs
//! them.

use axum::http::{Method, StatusCode};
use diesel_async::AsyncPgConnection;
use serde_json::{Value, json};
use tokio::join;
use uuid::Uuid;

use super::tests::{author, call, create, set_role};
use crate::models::agent::Agent;
use crate::models::company_doc::Section;
use crate::models::page::{self, LEARNINGS_PAGE_ID, PageError};
use crate::models::page_project;
use crate::models::project::{self, NewProject};
use crate::realtime::EventBus;
use crate::state::AppState;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database};

async fn new_project(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    project::create(
        connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: name.to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("the project is created")
    .id
}

/// What a project lists, by title.
async fn linked_titles(state: &AppState, project_id: Uuid) -> Vec<String> {
    let (status, listed) = call(
        state,
        Method::GET,
        &format!("/api/v1/projects/{project_id}/pages"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    listed["pages"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|page| page["title"].as_str().expect("a title").to_owned())
        .collect()
}

async fn link(state: &AppState, page_id: Uuid, project_id: Uuid) -> Value {
    let (status, linked) = call(
        state,
        Method::PUT,
        &format!("/api/v1/pages/{page_id}/projects/{project_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    linked
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_page_links_to_any_number_of_projects_in_its_history_and_never_as_a_version() {
    let (url, mut connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    state.events = EventBus::in_process();
    let page_id = create(&mut connection, None, "Deploying").await;
    let other_id = create(&mut connection, None, "Rollbacks").await;
    let roadmap = new_project(&mut connection, "Roadmap").await;
    let billing = new_project(&mut connection, "Billing").await;
    let mut heard = state.events.subscribe();

    let linked = link(&state, page_id, roadmap).await;
    assert_eq!(linked["page"]["projectIds"], json!([roadmap]));
    assert_eq!(
        linked["page"]["currentVersion"], 1,
        "a link is no version of the page"
    );
    let linked = link(&state, page_id, billing).await;
    assert_eq!(linked["page"]["projectIds"], json!([roadmap, billing]));
    link(&state, other_id, roadmap).await;

    // Every project is linked to its own brief from the start.
    let mut listed = linked_titles(&state, roadmap).await;
    listed.sort();
    assert_eq!(listed, ["Deploying", "Roadmap brief", "Rollbacks"]);
    let mut listed = linked_titles(&state, billing).await;
    listed.sort();
    assert_eq!(listed, ["Billing brief", "Deploying"]);

    // Answered unchanged: nothing recorded, nothing announced.
    while heard.try_recv().is_ok() {}
    let again = link(&state, page_id, roadmap).await;
    assert_eq!(again["page"]["projectIds"], json!([roadmap, billing]));
    assert!(heard.try_recv().is_err(), "linking again announces nothing");

    let (status, unlinked) = call(
        &state,
        Method::DELETE,
        &format!("/api/v1/pages/{page_id}/projects/{roadmap}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unlinked}");
    assert_eq!(unlinked["page"]["projectIds"], json!([billing]));
    assert_eq!(unlinked["page"]["currentVersion"], 1);
    let announced: Value =
        serde_json::from_str(&heard.try_recv().expect("an event")).expect("JSON");
    assert_eq!(announced["type"], "page.changed");
    assert_eq!(announced["data"]["id"], json!(page_id));
    let mut listed = linked_titles(&state, roadmap).await;
    listed.sort();
    assert_eq!(listed, ["Roadmap brief", "Rollbacks"]);

    let (status, again) = call(
        &state,
        Method::DELETE,
        &format!("/api/v1/pages/{page_id}/projects/{roadmap}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");

    let (status, history) = call(
        &state,
        Method::GET,
        &format!("/api/v1/pages/{page_id}/history"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let entries = history["history"].as_array().expect("a list");
    let kinds: Vec<&str> = entries
        .iter()
        .map(|entry| entry["kind"].as_str().expect("a kind"))
        .collect();
    assert_eq!(
        kinds,
        [
            "created",
            "project_linked",
            "project_linked",
            "project_unlinked"
        ]
    );
    assert_eq!(
        entries[3]["data"],
        json!({ "projectId": roadmap, "projectName": "Roadmap" })
    );

    let versions = page::versions(&mut connection, page_id)
        .await
        .expect("reads");
    assert_eq!(versions.len(), 1);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn two_links_at_once_are_one_link_and_one_entry() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;
    let mut first = state.database.get().await.expect("a connection");
    let mut second = state.database.get().await.expect("a connection");
    let page_id = create(&mut first, None, "Deploying").await;
    let roadmap = new_project(&mut first, "Roadmap").await;

    let (one, other) = join!(
        page_project::link(&mut first, page_id, roadmap, author(None)),
        page_project::link(&mut second, page_id, roadmap, author(None)),
    );
    let recorded = one.expect("linked").history.len() + other.expect("linked").history.len();

    assert_eq!(recorded, 1, "one of them found the link already there");
    assert_eq!(
        page_project::projects_of(&mut first, page_id)
            .await
            .expect("reads"),
        [roadmap]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn agents_link_any_page_and_are_named_for_it() {
    let (url, mut connection) = migrated_database().await;
    let _state = app_state(&url).await;
    let roadmap = new_project(&mut connection, "Roadmap").await;
    let agent = Agent::session(12);

    // The company doc's rule guards what it says and where it sits, neither of which a link touches.
    for page_id in [Section::Glossary.page_id(), LEARNINGS_PAGE_ID] {
        let recorded = page_project::link(&mut connection, page_id, roadmap, author(Some(&agent)))
            .await
            .expect("an agent links a page it only proposes to");
        assert_eq!(recorded.changed, [page_id]);
        assert_eq!(recorded.history[0].agent.as_deref(), Some("session:12"));
    }

    let recorded = page_project::unlink(
        &mut connection,
        Section::Glossary.page_id(),
        roadmap,
        author(Some(&agent)),
    )
    .await
    .expect("and takes it off");
    assert_eq!(recorded.history[0].kind, "project_unlinked");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_deleted_page_keeps_its_links_takes_no_new_ones_and_leaves_the_list_until_restored() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let page_id = create(&mut connection, None, "Deploying").await;
    let roadmap = new_project(&mut connection, "Roadmap").await;
    let billing = new_project(&mut connection, "Billing").await;
    link(&state, page_id, roadmap).await;

    page::delete(&mut connection, page_id, author(None), None)
        .await
        .expect("deleted");
    assert_eq!(linked_titles(&state, roadmap).await, ["Roadmap brief"]);

    for (method, project_id) in [(Method::PUT, billing), (Method::DELETE, roadmap)] {
        let (status, refused) = call(
            &state,
            method,
            &format!("/api/v1/pages/{page_id}/projects/{project_id}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{refused}");
        assert_eq!(refused["code"], "page_deleted");
    }

    page::restore(&mut connection, page_id, author(None), None)
        .await
        .expect("restored");
    assert_eq!(
        linked_titles(&state, roadmap).await,
        ["Roadmap brief", "Deploying"]
    );
    assert_eq!(linked_titles(&state, billing).await, ["Billing brief"]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn linking_what_does_not_exist_answers_not_found() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let page_id = create(&mut connection, None, "Deploying").await;
    let roadmap = new_project(&mut connection, "Roadmap").await;
    let unknown = Uuid::now_v7();

    for path in [
        format!("/api/v1/pages/{page_id}/projects/{unknown}"),
        format!("/api/v1/pages/{unknown}/projects/{roadmap}"),
    ] {
        let (status, body) = call(&state, Method::PUT, &path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }
    let (status, body) = call(
        &state,
        Method::GET,
        &format!("/api/v1/projects/{unknown}/pages"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let error = page_project::link(&mut connection, page_id, unknown, author(None))
        .await
        .expect_err("no such project");
    assert!(matches!(error, PageError::Database(_)), "{error:?}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn deleting_a_project_leaves_each_pages_history_saying_so() {
    let (url, mut connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    state.events = EventBus::in_process();
    let page_id = create(&mut connection, None, "Deploying").await;
    let gone_page_id = create(&mut connection, None, "Rollbacks").await;
    let roadmap = new_project(&mut connection, "Roadmap").await;
    let billing = new_project(&mut connection, "Billing").await;
    link(&state, page_id, roadmap).await;
    link(&state, page_id, billing).await;
    link(&state, gone_page_id, roadmap).await;
    page::delete(&mut connection, gone_page_id, author(None), None)
        .await
        .expect("deleted");
    let mut heard = state.events.subscribe();

    let (status, body) = call(
        &state,
        Method::DELETE,
        &format!("/api/v1/projects/{roadmap}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let mut announced = Vec::new();
    while let Ok(payload) = heard.try_recv() {
        let event: Value = serde_json::from_str(&payload).expect("JSON");
        if event["type"] == "page.changed" {
            announced.push(event["data"]["id"].clone());
        }
    }
    assert!(announced.contains(&json!(page_id)), "{announced:?}");
    assert!(announced.contains(&json!(gone_page_id)), "{announced:?}");

    for id in [page_id, gone_page_id] {
        let history = page::history(&mut connection, id).await.expect("reads");
        let last = history.last().expect("entries");
        assert_eq!(last.kind, "project_unlinked");
        assert_eq!(
            last.data,
            json!({ "projectId": roadmap, "projectName": "Roadmap" }),
            "the entry names a project that is gone"
        );
    }
    assert_eq!(
        page_project::projects_of(&mut connection, page_id)
            .await
            .expect("reads"),
        [billing],
        "the page keeps its other link"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guest_on_a_project_reaches_none_of_its_pages() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let page_id = create(&mut connection, None, "Deploying").await;
    let roadmap = new_project(&mut connection, "Roadmap").await;
    link(&state, page_id, roadmap).await;
    set_role(&mut connection, "guest").await;

    for (method, path) in [
        (Method::GET, format!("/api/v1/projects/{roadmap}/pages")),
        (
            Method::PUT,
            format!("/api/v1/pages/{page_id}/projects/{roadmap}"),
        ),
        (
            Method::DELETE,
            format!("/api/v1/pages/{page_id}/projects/{roadmap}"),
        ),
    ] {
        let (status, refused) = call(&state, method, &path, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {refused}");
        assert_eq!(refused["code"], "members_only", "{path}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_projects_brief_stays_linked_to_it_and_may_be_linked_to_others() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let roadmap = new_project(&mut connection, "Roadmap").await;
    let billing = new_project(&mut connection, "Billing").await;
    let brief = crate::models::project_brief::find(&mut connection, roadmap)
        .await
        .expect("every project has a brief");

    let (status, refused) = call(
        &state,
        Method::DELETE,
        &format!("/api/v1/pages/{}/projects/{roadmap}", brief.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "page_brief_linked");

    let linked = link(&state, brief.id, billing).await;
    assert_eq!(linked["page"]["briefProjectId"], json!(roadmap));
    assert_eq!(linked["page"]["fixed"], true);
    let (status, unlinked) = call(
        &state,
        Method::DELETE,
        &format!("/api/v1/pages/{}/projects/{billing}", brief.id),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "another project's link comes off: {unlinked}"
    );
    assert_eq!(unlinked["page"]["projectIds"], json!([roadmap]));
}
