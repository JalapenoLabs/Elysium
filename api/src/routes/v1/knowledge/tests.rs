// Copyright © 2026 Jalapeno Labs

//! The knowledge routes driven through the router (#94): an exact key found by its words while
//! the embedding model is away, a paraphrase found by its meaning alone, each filter narrowing,
//! the same text read by two people folded into one result, a document opened whole by any
//! member whoever's record it is, guests refused, queries validated, and a person held to their
//! budget.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::{DateTime, Duration, Utc};
use diesel::QueryableByName;
use diesel::sql_types::{Text, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::connections::connect_postgres;
use crate::embeddings::DIMENSIONS;
use crate::embeddings::fake::FakeEmbeddings;
use crate::knowledge::embedder::Embedder;
use crate::knowledge::search::{KnowledgeSearch, PER_PERSON};
use crate::middleware::rate_limit::Decision;
use crate::models::learning::{self, Author, LearningContent};
use crate::models::project::{self, NewProject};
use crate::models::timeline_record::{self, NewTimelineRecord};
use crate::routes;
use crate::state::AppState;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

/// The router's answer to `GET path`, as the test person.
async fn get(state: &AppState, path: &str) -> (StatusCode, Value) {
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
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("the body reads");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// What `GET /api/v1/knowledge/search?<query>` answered, which must be `200`.
async fn search(state: &AppState, query: &str) -> Value {
    let (status, body) = get(state, &format!("/api/v1/knowledge/search?{query}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// The titles an answer cites, best first.
fn titles(answer: &Value) -> Vec<String> {
    answer["results"]
        .as_array()
        .expect("results are a list")
        .iter()
        .map(|result| {
            result["citation"]["title"]
                .as_str()
                .expect("a title")
                .to_owned()
        })
        .collect()
}

/// A moment `hours` after a fixed one, so periods are exact.
fn at(hours: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(1_790_000_000, 0).expect("a fixed moment") + Duration::hours(hours)
}

/// A query value naming `moment`, offset and all.
fn query_moment(moment: DateTime<Utc>) -> String {
    moment.to_rfc3339().replace('+', "%2B")
}

/// Another person of the company, who owns records of their own.
async fn coworker(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
    let id = Uuid::now_v7();
    diesel::sql_query(
        "INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) \
         VALUES ($1, $2, $3, $4, 'member', 'active', now())",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<SqlUuid, _>(Uuid::now_v7())
    .bind::<Text, _>(format!("{}@example.com", id.simple()))
    .bind::<Text, _>(name)
    .execute(connection)
    .await
    .expect("the coworker is created");
    id
}

/// A timeline record of `kind` on `owner`'s timeline, indexed as it is stored.
async fn record(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
    kind: &str,
    occurred_at: DateTime<Utc>,
    title: &str,
    body: &str,
) -> Uuid {
    let new_record = NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: kind.to_owned(),
        connector_id: None,
        external_id: None,
        occurred_at,
        actor: "Jordan Lee".to_owned(),
        title: title.to_owned(),
        body: body.to_owned(),
        url: Some("https://example.slack.com/archives/C1/p1".to_owned()),
        media: None,
        metadata: json!({}),
        created_by: owner,
    };
    timeline_record::insert_new(connection, new_record)
        .await
        .expect("the record is stored")
        .expect("the record is new")
        .id
}

/// The id of the document the index holds for the record `record_id`.
async fn document_of_record(connection: &mut AsyncPgConnection, record_id: Uuid) -> Uuid {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = SqlUuid)]
        id: Uuid,
    }
    diesel::sql_query("SELECT id FROM knowledge_documents WHERE timeline_record_id = $1")
        .bind::<SqlUuid, _>(record_id)
        .get_result::<Row>(connection)
        .await
        .expect("the record is indexed")
        .id
}

async fn save_learning(connection: &mut AsyncPgConnection, title: &str, body: &str) -> Uuid {
    learning::save(
        connection,
        LearningContent {
            title: title.to_owned(),
            body: body.to_owned(),
            category: "procedure".to_owned(),
            evidence: "Seen in the tests of #94.".to_owned(),
        },
        Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: at(0),
        },
    )
    .await
    .expect("the learning is saved")
    .learning
    .id
}

async fn set_role(connection: &mut AsyncPgConnection, role: &str) {
    diesel::sql_query(format!(
        "UPDATE users SET role = '{role}' WHERE id = '{TEST_PERSON_ID}'"
    ))
    .execute(connection)
    .await
    .expect("the role changes");
}

#[derive(QueryableByName)]
struct Content {
    #[diesel(sql_type = Text)]
    content: String,
}

/// A vector pointing one way alone, which only a passage pinned to the same one is near.
fn direction(dimension: usize) -> Vec<f32> {
    let mut vector = vec![0.0; DIMENSIONS];
    vector[dimension] = 1.0;
    vector
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_exact_key_is_found_by_its_words_while_meaning_is_away() {
    let (url, mut connection) = migrated_database().await;
    // The test state's embedding service is an address nothing answers on.
    let state = app_state(&url).await;
    record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(1),
        "Sign-in loop",
        "WAY-123 is the sign-in loop on Safari; the fix is in review.",
    )
    .await;
    record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(2),
        "Export",
        "WAY-124 adds the CSV export.",
    )
    .await;
    record(
        &mut connection,
        TEST_PERSON_ID,
        "outlook",
        at(3),
        "Deploys",
        "The way we deploy is a tag on main.",
    )
    .await;

    let answer = search(&state, "q=WAY-123").await;

    assert_eq!(titles(&answer), ["Sign-in loop"], "{answer}");
    assert_eq!(answer["searchedByMeaning"], false);
    let result = &answer["results"][0];
    assert_eq!(result["citation"]["source"], "slack");
    assert_eq!(result["citation"]["person"]["name"], "Tester");
    assert!(
        result["passages"][0]["content"]
            .as_str()
            .expect("a passage")
            .contains("WAY-123"),
        "{result}"
    );

    let question = search(&state, "q=what+is+WAY-123+about%3F").await;
    assert_eq!(
        titles(&question),
        ["Sign-in loop"],
        "a question's stop words are dropped"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_paraphrase_is_found_by_its_meaning_alone() {
    let (url, mut connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    let fake = FakeEmbeddings::start().await;
    state.knowledge = KnowledgeSearch::new(fake.embeddings.clone());

    let releasing = save_learning(
        &mut connection,
        "Cutting a release",
        "Tag vX.Y.Z on main; the publish workflow pushes the images.",
    )
    .await;
    save_learning(
        &mut connection,
        "Rotating the encryption key",
        "Seal every secret again under the new key before removing the old one.",
    )
    .await;
    // The model puts the question beside the learning it paraphrases, with no word in common.
    let passage = diesel::sql_query(
        "SELECT content FROM knowledge_passages WHERE document_id = \
         (SELECT id FROM knowledge_documents WHERE learning_id = $1)",
    )
    .bind::<SqlUuid, _>(releasing)
    .get_result::<Content>(&mut connection)
    .await
    .expect("the learning is indexed")
    .content;
    fake.pin(&format!("search_document: {passage}"), direction(0));
    fake.pin("search_query: how do we ship", direction(0));
    let embedded = Embedder::new(
        connect_postgres(&url, 2).await.expect("a pool"),
        fake.embeddings,
    )
    .drain()
    .await
    .expect("the passages are embedded");
    assert_eq!(embedded, 2);

    let answer = search(&state, "q=how+do+we+ship").await;

    assert_eq!(answer["searchedByMeaning"], true);
    assert_eq!(titles(&answer)[0], "Cutting a release", "{answer}");

    // By its words alone, the same query finds nothing: it was the meaning that found it.
    let words_only = app_state(&url).await;
    let answer = search(&words_only, "q=how+do+we+ship").await;
    assert_eq!(answer["searchedByMeaning"], false);
    assert!(titles(&answer).is_empty(), "{answer}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn each_filter_narrows_and_none_hides_a_coworkers_record() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let jamie = coworker(&mut connection, "Jamie").await;
    let roadmap = project::create(
        &mut connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: "Roadmap".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("the project is created")
    .id;

    let in_project = record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(1),
        "Budget in Slack",
        "The budget for the offsite is approved.",
    )
    .await;
    timeline_record::attribute(&mut connection, in_project, roadmap, 0.9, "elysia")
        .await
        .expect("attributed");
    record(
        &mut connection,
        jamie,
        "outlook",
        at(5),
        "Budget by mail",
        "Jamie's budget question for finance.",
    )
    .await;
    save_learning(
        &mut connection,
        "Budget approvals",
        "Every budget over a thousand needs two approvals.",
    )
    .await;

    let everything = search(&state, "q=budget").await;
    assert_eq!(
        titles(&everything).len(),
        3,
        "a coworker's mail is everyone's: {everything}"
    );

    let by_source = search(&state, "q=budget&source=outlook,learning").await;
    let mut sources = titles(&by_source);
    sources.sort();
    assert_eq!(sources, ["Budget approvals", "Budget by mail"]);

    let by_project = search(&state, &format!("q=budget&project={roadmap}")).await;
    assert_eq!(titles(&by_project), ["Budget in Slack"]);

    let by_person = search(&state, &format!("q=budget&person={jamie}")).await;
    assert_eq!(titles(&by_person), ["Budget by mail"]);

    let by_period = search(
        &state,
        &format!(
            "q=budget&from={}&to={}",
            query_moment(at(4)),
            query_moment(at(6))
        ),
    )
    .await;
    assert_eq!(titles(&by_period), ["Budget by mail"]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_same_text_read_by_two_people_is_one_result_and_one_document_names_its_copy() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let jamie = coworker(&mut connection, "Jamie").await;
    let mine = record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(1),
        "#launch",
        "Launch moves to Thursday.",
    )
    .await;
    let theirs = record(
        &mut connection,
        jamie,
        "slack",
        at(1),
        "#launch",
        "Launch moves to Thursday.",
    )
    .await;
    let my_document = document_of_record(&mut connection, mine).await;
    let their_document = document_of_record(&mut connection, theirs).await;

    let answer = search(&state, "q=launch+thursday").await;

    assert_eq!(titles(&answer), ["#launch"], "{answer}");
    let result = &answer["results"][0];
    assert_eq!(
        result["citation"]["documentId"],
        my_document.to_string(),
        "the earliest indexed is cited"
    );
    assert_eq!(
        result["alsoRecordedBy"][0]["documentId"],
        their_document.to_string()
    );
    assert_eq!(result["alsoRecordedBy"][0]["person"]["name"], "Jamie");

    let (status, opened) = get(
        &state,
        &format!("/api/v1/knowledge/documents/{their_document}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    assert_eq!(
        opened["document"]["alsoRecordedBy"][0]["documentId"],
        my_document.to_string()
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_document_opens_whole_for_any_member_whoever_recorded_it() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let jamie = coworker(&mut connection, "Jamie").await;
    let roadmap = project::create(
        &mut connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: "Roadmap".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("the project is created")
    .id;
    let record_id = record(
        &mut connection,
        jamie,
        "outlook",
        at(2),
        "Quarterly plan",
        "First paragraph.\n\nSecond paragraph.",
    )
    .await;
    timeline_record::attribute(&mut connection, record_id, roadmap, 0.8, "elysia")
        .await
        .expect("attributed");
    let document_id = document_of_record(&mut connection, record_id).await;
    let learning_id = save_learning(&mut connection, "A takeaway", "Its body.").await;

    let (status, opened) = get(
        &state,
        &format!("/api/v1/knowledge/documents/{document_id}"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{opened}");
    let document = &opened["document"];
    assert_eq!(document["citation"]["title"], "Quarterly plan");
    assert_eq!(document["citation"]["person"]["name"], "Jamie");
    assert_eq!(document["origin"], json!({ "kind": "timelineRecord" }));
    assert_eq!(document["projects"][0]["name"], "Roadmap");
    let text: Vec<&str> = document["passages"]
        .as_array()
        .expect("passages")
        .iter()
        .map(|passage| passage["content"].as_str().expect("content"))
        .collect();
    assert!(text.concat().contains("Second paragraph."), "{document}");

    let found = search(&state, "q=takeaway").await;
    let learning_document = found["results"][0]["citation"]["documentId"]
        .as_str()
        .expect("the learning is found")
        .to_owned();
    let (_, opened) = get(
        &state,
        &format!("/api/v1/knowledge/documents/{learning_document}"),
    )
    .await;
    assert_eq!(
        opened["document"]["origin"],
        json!({ "kind": "learning", "learningId": learning_id })
    );

    let (status, _) = get(
        &state,
        &format!("/api/v1/knowledge/documents/{}", Uuid::now_v7()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guest_reaches_no_knowledge() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let record_id = record(
        &mut connection,
        TEST_PERSON_ID,
        "slack",
        at(1),
        "Plans",
        "Plans for the launch.",
    )
    .await;
    let document_id = document_of_record(&mut connection, record_id).await;
    set_role(&mut connection, "guest").await;

    for path in [
        "/api/v1/knowledge/search?q=launch".to_owned(),
        format!("/api/v1/knowledge/documents/{document_id}"),
    ] {
        let (status, refused) = get(&state, &path).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(refused["code"], "members_only", "{path}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn queries_are_checked_before_anything_is_searched() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;
    let long = "a".repeat(501);

    for query in [
        format!("q={long}"),
        "q=x&source=Slack".to_owned(),
        "q=x&limit=0".to_owned(),
        "q=x&limit=51".to_owned(),
        format!(
            "q=x&from={}&to={}",
            query_moment(at(2)),
            query_moment(at(1))
        ),
    ] {
        let (status, body) = get(&state, &format!("/api/v1/knowledge/search?{query}")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}: {body}");
    }
    for query in ["q=x&from=2026-11-04T09:00:00", "q=x&project=nope", "x=1"] {
        let (status, body) = get(&state, &format!("/api/v1/knowledge/search?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
    }

    let blank = search(&state, "q=++").await;
    assert_eq!(blank["results"], json!([]));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_person_is_held_to_their_budget() {
    let (url, _connection) = migrated_database().await;
    let state = app_state(&url).await;

    // Spent at once, so the bucket has no time to refill before the route spends it too.
    for _ in 0..PER_PERSON.burst {
        assert!(matches!(
            state.knowledge.admit(TEST_PERSON_ID),
            Decision::Admitted { .. }
        ));
    }
    let (status, refused) = get(&state, "/api/v1/knowledge/search?q=anything").await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refused["code"], "knowledge_search_rate_limited");
}
