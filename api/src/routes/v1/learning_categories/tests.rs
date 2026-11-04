// Copyright © 2026 Jalapeno Labs

//! Learning categories against Postgres (#155): the nine built-in ones seeded and fixed, the
//! workspace's own added, edited, reordered, and recorded as workspace settings, learnings filed
//! under either, and removing a category that learnings use moving them first, each move a
//! revision of its own. The routes answer members' reads and admins' writes alone.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use diesel::QueryableByName;
use diesel::sql_types::{Jsonb, Text};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use crate::models::learning::{self, Author, HistoryKind, LearningContent, LearningError};
use crate::models::learning_category::{self, CategoryChanges, CategoryError, NewCategory};
use crate::routes;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

/// Elysium's nine, in the order they are offered.
const BUILT_IN: [&str; 9] = [
    "decision",
    "convention",
    "procedure",
    "pitfall",
    "reference",
    "environment",
    "people",
    "project",
    "incident",
];

#[derive(QueryableByName)]
struct Event {
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = Jsonb)]
    data: Value,
}

fn author() -> Author<'static> {
    Author {
        user_id: TEST_PERSON_ID,
        agent: None,
        at: Utc::now(),
    }
}

fn new_category(key: &str, label: &str) -> NewCategory {
    NewCategory {
        key: key.to_owned(),
        label: label.to_owned(),
        description: format!("Everything about {label}."),
    }
}

fn filed_under(title: &str, category: &str) -> LearningContent {
    LearningContent {
        title: title.to_owned(),
        body: "Lint every module before synthesis.".to_owned(),
        category: category.to_owned(),
        evidence: "Synthesis failed on an unlinted module in the March tapeout.".to_owned(),
    }
}

async fn keys(connection: &mut AsyncPgConnection) -> Vec<String> {
    learning_category::list(connection)
        .await
        .expect("listed")
        .into_iter()
        .map(|category| category.key)
        .collect()
}

/// The workspace-setting events learning categories recorded, oldest first.
async fn category_events(connection: &mut AsyncPgConnection) -> Vec<Event> {
    diesel::sql_query(
        "SELECT kind, data FROM user_events \
         WHERE user_id IS NULL AND kind LIKE 'learning_category_%' ORDER BY id",
    )
    .load(connection)
    .await
    .expect("the events load")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_built_in_categories_are_always_there_and_never_change() {
    let (_url, mut connection) = migrated_database().await;

    assert_eq!(keys(&mut connection).await, BUILT_IN);
    let pitfall = learning_category::find(&mut connection, "pitfall")
        .await
        .expect("seeded");
    assert!(pitfall.built_in);
    assert!(
        pitfall.description.contains("the way around it"),
        "agents read what learnings were built with: {}",
        pitfall.description
    );

    let edited = learning_category::update(
        &mut connection,
        "pitfall",
        CategoryChanges {
            label: Some("Gotcha".to_owned()),
            ..CategoryChanges::default()
        },
        TEST_PERSON_ID,
    )
    .await;
    assert!(matches!(edited, Err(CategoryError::BuiltIn)));
    let removed = learning_category::remove(&mut connection, "pitfall", None, author()).await;
    assert!(matches!(removed, Err(CategoryError::BuiltIn)));

    let taken = learning_category::create(
        &mut connection,
        new_category("pitfall", "Pitfall"),
        TEST_PERSON_ID,
    )
    .await;
    assert!(
        matches!(taken, Err(CategoryError::KeyTaken)),
        "a built-in key is taken"
    );
    assert!(category_events(&mut connection).await.is_empty());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_workspace_category_comes_after_the_built_in_ones_and_learnings_file_under_it() {
    let (_url, mut connection) = migrated_database().await;
    for (key, label) in [("rtl", "RTL"), ("eda_tools", "EDA tools")] {
        learning_category::create(&mut connection, new_category(key, label), TEST_PERSON_ID)
            .await
            .expect("added");
    }

    let listed = keys(&mut connection).await;
    assert_eq!(listed[..9], BUILT_IN, "the built-in ones come first");
    assert_eq!(
        listed[9..],
        ["rtl", "eda_tools"],
        "then the workspace's own, as added"
    );

    let saved = learning::save(
        &mut connection,
        filed_under("Lint before synthesis", "rtl"),
        author(),
    )
    .await
    .expect("filed under a workspace category");
    assert_eq!(saved.learning.category, "rtl");

    let unknown = learning::save(
        &mut connection,
        filed_under("Something else", "verification"),
        author(),
    )
    .await;
    let Err(LearningError::UnknownCategory { key, known }) = unknown else {
        panic!("an unknown category is refused: {unknown:?}");
    };
    assert_eq!(key, "verification");
    assert_eq!(known, listed, "the refusal names every category there is");

    let events = category_events(&mut connection).await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, "learning_category_added");
    assert_eq!(
        events[0].data["category"],
        json!({
            "key": "rtl",
            "label": "RTL",
            "description": "Everything about RTL.",
            "position": 0,
        })
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_edit_is_recorded_before_and_after_and_a_move_renumbers_the_rest() {
    let (_url, mut connection) = migrated_database().await;
    for (key, label) in [
        ("rtl", "RTL"),
        ("verification", "Verification"),
        ("soc", "SoC"),
    ] {
        learning_category::create(&mut connection, new_category(key, label), TEST_PERSON_ID)
            .await
            .expect("added");
    }

    let moved = learning_category::update(
        &mut connection,
        "soc",
        CategoryChanges {
            label: Some("Systems on chip".to_owned()),
            position: Some(0),
            ..CategoryChanges::default()
        },
        TEST_PERSON_ID,
    )
    .await
    .expect("edited");
    assert_eq!(moved.label, "Systems on chip");
    assert_eq!(moved.position, 0);
    assert_eq!(
        keys(&mut connection).await[9..],
        ["soc", "rtl", "verification"]
    );

    let last = learning_category::update(
        &mut connection,
        "soc",
        CategoryChanges {
            position: Some(99),
            ..CategoryChanges::default()
        },
        TEST_PERSON_ID,
    )
    .await
    .expect("moved");
    assert_eq!(last.position, 2, "past the end is last");

    let unchanged = learning_category::update(
        &mut connection,
        "soc",
        CategoryChanges {
            label: Some("Systems on chip".to_owned()),
            ..CategoryChanges::default()
        },
        TEST_PERSON_ID,
    )
    .await
    .expect("nothing to change");
    assert_eq!(unchanged.label, "Systems on chip");

    let events = category_events(&mut connection).await;
    let changes: Vec<&Value> = events
        .iter()
        .filter(|event| event.kind == "learning_category_changed")
        .map(|event| &event.data)
        .collect();
    assert_eq!(
        changes.len(),
        2,
        "an edit that changes nothing records nothing"
    );
    assert_eq!(changes[0]["before"]["label"], "SoC");
    assert_eq!(changes[0]["after"]["label"], "Systems on chip");
    assert_eq!(changes[0]["after"]["position"], 0);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn removing_a_category_in_use_moves_its_learnings_first_each_as_a_revision() {
    let (_url, mut connection) = migrated_database().await;
    learning_category::create(&mut connection, new_category("rtl", "RTL"), TEST_PERSON_ID)
        .await
        .expect("added");
    let live = learning::save(&mut connection, filed_under("Lint first", "rtl"), author())
        .await
        .expect("saved");
    let deleted = learning::save(&mut connection, filed_under("Old advice", "rtl"), author())
        .await
        .expect("saved");
    learning::delete(&mut connection, deleted.learning.id, author())
        .await
        .expect("deleted");

    let refused = learning_category::remove(&mut connection, "rtl", None, author()).await;
    assert!(matches!(refused, Err(CategoryError::InUse)));
    let to_itself = learning_category::remove(&mut connection, "rtl", Some("rtl"), author()).await;
    assert!(matches!(to_itself, Err(CategoryError::InvalidMove(_))));
    let nowhere =
        learning_category::remove(&mut connection, "rtl", Some("verification"), author()).await;
    assert!(matches!(nowhere, Err(CategoryError::InvalidMove(_))));
    assert!(
        keys(&mut connection).await.contains(&"rtl".to_owned()),
        "nothing refused removed anything"
    );

    let moved = learning_category::remove(&mut connection, "rtl", Some("convention"), author())
        .await
        .expect("removed");
    assert_eq!(moved.len(), 2, "deleted learnings move too");
    assert!(!keys(&mut connection).await.contains(&"rtl".to_owned()));
    for id in [live.learning.id, deleted.learning.id] {
        let learning = learning::find(&mut connection, id).await.expect("kept");
        assert_eq!(learning.category, "convention");
        let history = learning::history(&mut connection, id)
            .await
            .expect("history");
        let last = history.last().expect("the move is recorded");
        assert_eq!(last.kind, HistoryKind::Revised.as_str());
        assert_eq!(
            last.data["changes"],
            json!({ "category": { "from": "rtl", "to": "convention" } })
        );
    }

    let revision = learning::history(&mut connection, live.learning.id)
        .await
        .expect("history")
        .last()
        .expect("the move")
        .id;
    let undo = learning::undo(&mut connection, live.learning.id, revision, author()).await;
    assert!(
        matches!(undo, Err(LearningError::NothingToUndo(_))),
        "a removed category is not put back: {undo:?}"
    );

    let events = category_events(&mut connection).await;
    let removal = events.last().expect("recorded");
    assert_eq!(removal.kind, "learning_category_removed");
    assert_eq!(removal.data["movedTo"], "convention");
    assert_eq!(removal.data["movedLearnings"], 2);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn an_unused_category_is_removed_without_moving_anything() {
    let (_url, mut connection) = migrated_database().await;
    learning_category::create(&mut connection, new_category("rtl", "RTL"), TEST_PERSON_ID)
        .await
        .expect("added");

    let moved = learning_category::remove(&mut connection, "rtl", None, author())
        .await
        .expect("removed");
    assert!(moved.is_empty());
    assert_eq!(keys(&mut connection).await, BUILT_IN);
    let events = category_events(&mut connection).await;
    assert_eq!(
        events.last().expect("recorded").data["movedTo"],
        Value::Null
    );
}

/// A router test's answer to `method path` with `body`, as the test person.
async fn call(
    state: &crate::state::AppState,
    method: Method,
    path: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let cookie = signed_in_cookie(state).await;
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost:4000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
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
async fn admins_manage_the_list_members_read_it_and_guests_reach_none_of_it() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let category = json!({
        "key": " eda_tools ",
        "label": "EDA tools",
        "description": "Simulators, synthesis, and their licenses.",
    });

    let (status, created) = call(
        &state,
        Method::POST,
        "/api/v1/learning-categories",
        Some(&category),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        created["category"]["key"], "eda_tools",
        "the key is trimmed"
    );
    assert_eq!(created["category"]["builtIn"], false);

    let learning = json!({
        "title": "Check the license server before a regression",
        "body": "Start the license daemon first.",
        "category": "eda_tools",
        "evidence": "Every job failed with a checkout error on November 3rd.",
    });
    let (status, saved) = call(&state, Method::POST, "/api/v1/learnings", Some(&learning)).await;
    assert_eq!(status, StatusCode::CREATED, "{saved}");

    let (status, refused) = call(
        &state,
        Method::DELETE,
        "/api/v1/learning-categories/eda_tools",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "category_in_use");

    set_role(&mut connection, "member").await;
    let (status, listed) = call(&state, Method::GET, "/api/v1/learning-categories", None).await;
    assert_eq!(status, StatusCode::OK, "a member reads the list");
    let last = listed["categories"]
        .as_array()
        .and_then(|categories| categories.last())
        .expect("listed");
    assert_eq!(last["key"], "eda_tools");
    assert_eq!(last["learningCount"], 1);
    for (method, path, body) in [
        (Method::POST, "/api/v1/learning-categories", Some(&category)),
        (
            Method::PATCH,
            "/api/v1/learning-categories/eda_tools",
            Some(&json!({ "position": 0 })),
        ),
        (
            Method::DELETE,
            "/api/v1/learning-categories/eda_tools?moveTo=reference",
            None,
        ),
    ] {
        let (status, refused) = call(&state, method.clone(), path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(refused["code"], "admin_only", "{method} {path}");
    }

    set_role(&mut connection, "guest").await;
    let (status, refused) = call(&state, Method::GET, "/api/v1/learning-categories", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "members_only");

    set_role(&mut connection, "admin").await;
    let (status, _) = call(
        &state,
        Method::DELETE,
        "/api/v1/learning-categories/eda_tools?moveTo=reference",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, listed) = call(&state, Method::GET, "/api/v1/learning-categories", None).await;
    assert_eq!(
        listed["categories"].as_array().map(Vec::len),
        Some(9),
        "only the built-in categories are left"
    );
}
