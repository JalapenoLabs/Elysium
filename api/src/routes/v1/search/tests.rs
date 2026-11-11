// Copyright © 2026 Jalapeno Labs

//! The search route driven through the router (#175): each kind found by words that begin its
//! title, only among what its own list shows, closest first and no more than asked; queries
//! refused or matching nothing; guests answered as the lists answer them; and a person held
//! to their budget.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::models::action_item::{self, ActionItemPriority, ActionItemState, NewActionItem, Owner};
use crate::models::coding_session::{self, NewCodingSession};
use crate::models::initiative::{self, NewInitiative};
use crate::models::project::{self, NewProject};
use crate::routes;
use crate::search::PER_PERSON;
use crate::test_support::{
    TEST_PERSON, TEST_PERSON_ID, app_state, cipher, hold_budgets_still, migrated_database,
    signed_in_cookie, studio_item,
};

/// The router's answer to `GET path`, as the test person.
async fn get(state: &crate::state::AppState, path: &str) -> (StatusCode, Value) {
    let cookie = signed_in_cookie(state).await;
    let request = Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .expect("a request builds");
    let response = routes::router(state)
        .with_state(state.clone())
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The titles found for `query`, in the order the route answered them.
async fn titles(state: &crate::state::AppState, query: &str) -> Vec<String> {
    let (status, body) = get(state, &format!("/api/v1/search?q={query}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["results"]
        .as_array()
        .expect("results are a list")
        .iter()
        .map(|result| result["title"].as_str().expect("a title").to_owned())
        .collect()
}

async fn item(connection: &mut AsyncPgConnection, title: &str) -> Uuid {
    let new_item = NewActionItem {
        title: title.to_owned(),
        notes: String::new(),
        state: ActionItemState::Open,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    };
    action_item::create(connection, new_item, TEST_PERSON, Utc::now())
        .await
        .expect("the item is created")
        .record
        .id
}

async fn project(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    let new_project = NewProject {
        created_by: TEST_PERSON_ID,
        name: name.to_owned(),
        description: String::new(),
    };
    project::create(connection, &new_project)
        .await
        .expect("the project is created")
        .id
}

async fn initiative(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    let new_initiative = NewInitiative {
        name: name.to_owned(),
        description: String::new(),
        target_at: None,
        project_ids: Vec::new(),
    };
    initiative::create(connection, new_initiative, TEST_PERSON, Utc::now())
        .await
        .expect("the initiative is created")
        .record
        .id
}

/// A Coding session in a project of its own, or with `studio_item_id`, a Studio item's
/// session, which needs none.
async fn session(
    connection: &mut AsyncPgConnection,
    title: &str,
    studio_item_id: Option<Uuid>,
) -> i64 {
    let project_id = match studio_item_id {
        Some(_) => None,
        // Named so that no query below matches it.
        None => Some(project(connection, &format!("Holder {}", Uuid::now_v7().simple())).await),
    };
    let id = coding_session::reserve_id(connection)
        .await
        .expect("a number is reserved");
    let new_session = NewCodingSession {
        id,
        project_id,
        title: title.to_owned(),
        github_credential_id: None,
        action_item_id: None,
        studio_item_id,
        created_by: TEST_PERSON_ID,
    };
    coding_session::create(connection, &new_session)
        .await
        .expect("the session is created");
    id
}

async fn set_role(connection: &mut AsyncPgConnection, role: &str) {
    diesel::sql_query(format!(
        "UPDATE users SET role = '{role}' WHERE id = '{TEST_PERSON_ID}'"
    ))
    .execute(connection)
    .await
    .expect("the role changes");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn finds_each_kind_by_words_that_begin_its_title() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let item_id = item(&mut connection, "Fix WAY-123 sign-in loop").await;
    let project_id = project(&mut connection, "Platform").await;
    let brief_id = crate::models::project_brief::find(&mut connection, project_id)
        .await
        .expect("every project has a brief")
        .id;
    let session_id = session(&mut connection, "Sign-in redirect", None).await;
    let initiative_id = initiative(&mut connection, "Launch the platform").await;

    let (status, body) = get(&state, "/api/v1/search?q=sign").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["results"],
        json!([
            { "kind": "actionItem", "id": item_id, "title": "Fix WAY-123 sign-in loop", "state": "open" },
            { "kind": "codingSession", "id": session_id, "title": "Sign-in redirect" },
        ]),
        "each kind in its own order, carrying only what a row shows"
    );

    let (_, body) = get(&state, "/api/v1/search?q=PLAT").await;
    assert_eq!(
        body["results"],
        json!([
            { "kind": "project", "id": project_id, "title": "Platform" },
            { "kind": "initiative", "id": initiative_id, "title": "Launch the platform", "state": "active" },
            { "kind": "page", "id": brief_id, "title": "Platform brief", "pendingSuggestions": 0 },
        ]),
        "case is ignored"
    );

    assert_eq!(
        titles(&state, "way%20123").await,
        ["Fix WAY-123 sign-in loop"],
        "punctuation separates words, so the parts of a ticket key find it"
    );
    assert_eq!(
        titles(&state, "loop%20fix").await,
        ["Fix WAY-123 sign-in loop"],
        "every word must match, in any order"
    );
    assert!(
        titles(&state, "fix%20nothing").await.is_empty(),
        "a word found nowhere rules the title out"
    );
    assert!(
        titles(&state, "orm").await.is_empty(),
        "inside a word is not the start of one"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn searches_only_what_each_list_shows() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    item(&mut connection, "Archive the kept item").await;
    let deleted_item = item(&mut connection, "Archive the deleted item").await;
    action_item::soft_delete(&mut connection, deleted_item, TEST_PERSON, Utc::now())
        .await
        .expect("the item is deleted");
    let deleted_initiative = initiative(&mut connection, "Archive initiative").await;
    initiative::soft_delete(&mut connection, deleted_initiative, TEST_PERSON, Utc::now())
        .await
        .expect("the initiative is deleted");
    session(&mut connection, "Archive coding work", None).await;
    let banana = studio_item(&mut connection, &cipher()).await;
    session(&mut connection, "Archive a banana model", Some(banana.id)).await;

    assert_eq!(
        titles(&state, "archive").await,
        ["Archive the kept item", "Archive coding work"],
        "deleted items and initiatives, and Studio's sessions, are left out"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn ranks_titles_that_begin_with_the_query_first_and_keeps_to_the_limit() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    for title in [
        "Read the release notes",
        "Release checklist for the new installer",
        "Release",
    ] {
        item(&mut connection, title).await;
    }

    assert_eq!(
        titles(&state, "release").await,
        [
            "Release",
            "Release checklist for the new installer",
            "Read the release notes"
        ],
        "a title beginning with the words first, the closest of those first"
    );
    assert_eq!(
        titles(&state, "release&limit=2").await,
        ["Release", "Release checklist for the new installer"],
        "no more of a kind than asked"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn refuses_malformed_queries_and_finds_nothing_without_words() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    item(&mut connection, "Percent sign % in a title").await;

    assert!(
        titles(&state, "%25%20_%20.*").await.is_empty(),
        "only punctuation is no word, so nothing matches"
    );
    let too_long = "a".repeat(201);
    for path in [
        format!("/api/v1/search?q={too_long}"),
        "/api/v1/search?q=a&limit=0".to_owned(),
        "/api/v1/search?q=a&limit=51".to_owned(),
    ] {
        let (status, _) = get(&state, &path).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{path}");
    }
    for path in ["/api/v1/search", "/api/v1/search?q=a&kind=projects"] {
        let (status, _) = get(&state, path).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guest_finds_what_the_lists_show_a_guest() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    item(&mut connection, "Guest-visible item").await;
    project(&mut connection, "Guest-visible project").await;
    set_role(&mut connection, "guest").await;

    let (status, _) = get(&state, "/api/v1/action-items").await;
    assert_eq!(status, StatusCode::OK, "the list answers a guest");
    assert_eq!(
        titles(&state, "guest").await,
        ["Guest-visible item", "Guest-visible project"]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn holds_a_person_to_their_search_budget() {
    let (url, _connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    // Time held still, so the bucket cannot refill while the requests spend it.
    hold_budgets_still(&mut state);

    for spent in 0..PER_PERSON.burst {
        let (status, body) = get(&state, "/api/v1/search?q=anything").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "search {spent} of the burst: {body}"
        );
    }
    let (status, refusal) = get(&state, "/api/v1/search?q=anything").await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refusal}");
    assert_eq!(refusal["code"], "search_rate_limited");
}
