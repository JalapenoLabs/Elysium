// Copyright © 2026 Jalapeno Labs

//! Page labels against Postgres (#203): a label as history and no version, the knowledge index
//! following it without touching a passage, who may label what, and the search's filter in both
//! rankings.
//!
//! Those are `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs
//! them.

use axum::http::{Method, StatusCode};
use chrono::Utc;
use diesel::QueryableByName;
use diesel::sql_types::{BigInt, Nullable, Text, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ELYSIA_USER_ID, author, call, create, edit, execute, kinds, refused, set_role};
use crate::connections::connect_postgres;
use crate::embeddings::DIMENSIONS;
use crate::embeddings::fake::FakeEmbeddings;
use crate::knowledge::embedder::Embedder;
use crate::knowledge::search::KnowledgeSearch;
use crate::models::agent::{Agent, Author};
use crate::models::company_doc::Section;
use crate::models::learning::{self, LearningContent, LearningError};
use crate::models::page::{self, LEARNINGS_PAGE_ID, PageError};
use crate::models::page_label::{self, Label, MAX_LABELS_PER_PAGE};
use crate::routes::v1::knowledge::tests::get;
use crate::state::AppState;
use crate::test_support::{app_state, migrated_database};

fn label(text: &str) -> Label {
    Label::parse(text).expect("a well-formed label")
}

async fn add(connection: &mut AsyncPgConnection, id: Uuid, text: &str) {
    page_label::add(connection, id, &label(text), author(None))
        .await
        .expect("labeled");
}

#[derive(QueryableByName)]
struct IndexedPassages {
    #[diesel(sql_type = Text)]
    content_sha256: String,
    #[diesel(sql_type = BigInt)]
    embedded: i64,
    #[diesel(sql_type = Nullable<Text>)]
    model: Option<String>,
}

/// Page `id`'s document: the text it was indexed from, how many of its passages hold an embedding,
/// and the model that made one.
async fn indexed_passages(connection: &mut AsyncPgConnection, id: Uuid) -> IndexedPassages {
    diesel::sql_query(
        "SELECT d.content_sha256, count(p.embedding) AS embedded, \
             min(p.embedding_model) AS model \
         FROM knowledge_documents d JOIN knowledge_passages p ON p.document_id = d.id \
         WHERE d.page_id = $1 GROUP BY d.content_sha256",
    )
    .bind::<SqlUuid, _>(id)
    .get_result(connection)
    .await
    .expect("the page is indexed")
}

/// The titles a search answers, best first, which must be `200`.
async fn found(state: &AppState, query: &str) -> Vec<String> {
    let (status, answer) = get(state, &format!("/api/v1/knowledge/search?{query}")).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    answer["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|result| {
            let title = result["citation"]["title"].as_str().expect("a title");
            title.to_owned()
        })
        .collect()
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|item| item.as_str().expect("a string"))
        .collect()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_label_is_history_and_no_version_and_a_second_time_records_nothing() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "Deploying").await;
    let before = page::find(&mut connection, id).await.expect("found");
    let session = Agent::session(12);
    let by_an_agent = Author {
        agent: Some(&session),
        ..author(None)
    };

    let added = page_label::add(&mut connection, id, &label("runbook"), by_an_agent)
        .await
        .expect("labeled");
    assert_eq!(added.changed, [id], "the page is announced");

    let again = page_label::add(&mut connection, id, &label("Runbook"), author(None))
        .await
        .expect("answered");
    assert!(again.changed.is_empty(), "a label already there is silent");

    add(&mut connection, id, "oncall").await;
    assert_eq!(
        page_label::of_page(&mut connection, id)
            .await
            .expect("read"),
        ["oncall", "runbook"],
        "labels read in order"
    );

    let removed = page_label::remove(&mut connection, id, &label("runbook"), author(None))
        .await
        .expect("unlabeled");
    assert_eq!(removed.changed, [id]);
    let absent = page_label::remove(&mut connection, id, &label("runbook"), author(None))
        .await
        .expect("answered");
    assert!(absent.changed.is_empty(), "a label it lacks is silent");

    let history = page::history(&mut connection, id).await.expect("read");
    assert_eq!(
        kinds(&history),
        ["created", "labeled", "labeled", "unlabeled"]
    );
    assert_eq!(history[1].data, json!({ "label": "runbook" }));
    assert_eq!(history[1].agent.as_deref(), Some("session:12"));
    assert_eq!(history[3].data, json!({ "label": "runbook" }));

    let after = page::find(&mut connection, id).await.expect("found");
    assert_eq!(after.current_version, before.current_version);
    assert_eq!(after.updated_at, before.updated_at, "the page itself held");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn labeling_leaves_every_passage_and_its_embedding_alone() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "Deploying").await;
    execute(
        &mut connection,
        format!(
            "UPDATE knowledge_passages SET \
                 embedding = array_fill(0.1::real, ARRAY[{DIMENSIONS}])::vector, \
                 embedding_model = 'the-model' \
             WHERE document_id = (SELECT id FROM knowledge_documents WHERE page_id = '{id}')"
        ),
    )
    .await;
    let before = indexed_passages(&mut connection, id).await;
    assert_eq!(before.embedded, 1);

    add(&mut connection, id, "runbook").await;
    page_label::remove(&mut connection, id, &label("runbook"), author(None))
        .await
        .expect("unlabeled");

    let after = indexed_passages(&mut connection, id).await;
    assert_eq!(after.content_sha256, before.content_sha256);
    assert_eq!(after.embedded, 1, "the passage stays embedded");
    assert_eq!(after.model.as_deref(), Some("the-model"));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_page_carries_up_to_the_limit_and_still_answers_for_what_it_has() {
    let (_url, mut connection) = migrated_database().await;
    let id = create(&mut connection, None, "Deploying").await;
    for number in 0..MAX_LABELS_PER_PAGE {
        add(&mut connection, id, &format!("label-{number}")).await;
    }

    let outcome = page_label::add(&mut connection, id, &label("one-more"), author(None)).await;
    assert!(refused(&outcome, |error| matches!(
        error,
        PageError::TooManyLabels { .. }
    )));
    let carried = page_label::of_page(&mut connection, id)
        .await
        .expect("read");
    assert!(!carried.contains(&"one-more".to_owned()), "nothing stayed");

    let held = page_label::add(&mut connection, id, &label("label-0"), author(None))
        .await
        .expect("a label it has is no new label");
    assert!(held.changed.is_empty());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_deleted_page_takes_no_label_and_comes_back_with_its_own() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let id = create(&mut connection, None, "Deploying").await;
    add(&mut connection, id, "runbook").await;
    assert_eq!(found(&state, "q=body&label=runbook").await, ["Deploying"]);

    page::delete(&mut connection, id, author(None), None)
        .await
        .expect("deleted");
    assert!(found(&state, "q=body&label=runbook").await.is_empty());

    let adding = page_label::add(&mut connection, id, &label("later"), author(None)).await;
    assert!(refused(&adding, |error| matches!(
        error,
        PageError::Deleted
    )));
    let removing = page_label::remove(&mut connection, id, &label("runbook"), author(None)).await;
    assert!(refused(&removing, |error| matches!(
        error,
        PageError::Deleted
    )));

    page::restore(&mut connection, id, author(None), None)
        .await
        .expect("restored");
    assert_eq!(found(&state, "q=body&label=runbook").await, ["Deploying"]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn people_and_agents_label_a_proposal_only_or_fixed_page_all_the_same() {
    let (_url, mut connection) = migrated_database().await;
    let glossary = Section::Glossary.page_id();
    let client = Agent::mcp_client("claude-code-client", "Claude Code").expect("recordable");
    let elysia = Author {
        user_id: ELYSIA_USER_ID,
        agent: None,
        at: Utc::now(),
    };

    for (written_by, name) in [
        (author(None), "person"),
        (author(Some(&client)), "agent"),
        (elysia, "elysia"),
    ] {
        page_label::add(&mut connection, glossary, &label(name), written_by)
            .await
            .expect("a label is not what a proposal protects");
    }

    add(&mut connection, LEARNINGS_PAGE_ID, "index").await;
    assert_eq!(
        page_label::of_page(&mut connection, glossary)
            .await
            .expect("read"),
        ["agent", "elysia", "person"]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_label_stays_out_of_a_learnings_own_history_and_feed() {
    let (_url, mut connection) = migrated_database().await;
    let saved = learning::save(
        &mut connection,
        LearningContent {
            title: "Lint first".to_owned(),
            body: "Run the linter first.".to_owned(),
            category: "procedure".to_owned(),
            evidence: "It caught it on WAY-123.".to_owned(),
        },
        author(None),
    )
    .await
    .expect("saved");
    let id = saved.learning.id;
    add(&mut connection, id, "runbook").await;

    let history = learning::history(&mut connection, id).await.expect("read");
    assert_eq!(history.len(), 1, "the save alone");
    // The limit counts what the feed shows, so one entry is the save, not the label.
    let feed = learning::feed(&mut connection, None, 1)
        .await
        .expect("read");
    assert_eq!(feed[0].0.kind, "created");

    let entry = page::history(&mut connection, id).await.expect("read");
    let undoing = learning::undo(&mut connection, id, entry[1].id, author(None)).await;
    assert!(matches!(undoing, Err(LearningError::NothingToUndo(_))));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_routes_read_a_label_one_way_and_refuse_a_bad_one_clearly() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let id = create(&mut connection, None, "Deploying").await;
    let path = format!("/api/v1/pages/{id}/labels");

    let (status, answer) = call(
        &state,
        Method::POST,
        &path,
        Some(&json!({ "label": "  On  Call " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(strings(&answer["page"]["labels"]), ["on-call"]);

    let (status, read) = call(&state, Method::GET, &format!("/api/v1/pages/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(strings(&read["page"]["labels"]), ["on-call"]);
    assert!(read["page"]["updatedAt"].is_string());

    let (status, listed) = call(&state, Method::GET, "/api/v1/pages", None).await;
    assert_eq!(status, StatusCode::OK);
    let summaries = listed["pages"].as_array().expect("pages");
    let summary = summaries
        .iter()
        .find(|summary| summary["id"] == json!(id))
        .expect("listed");
    assert_eq!(strings(&summary["labels"]), ["on-call"]);
    assert!(summary["updatedAt"].is_string());

    let (status, refusal) = call(
        &state,
        Method::POST,
        &path,
        Some(&json!({ "label": "a/b" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert!(
        refusal
            .to_string()
            .contains("starting with a letter or digit")
    );

    // A slash or dots in the address would otherwise be a different address.
    let (status, _) = call(&state, Method::DELETE, &format!("{path}/a%2Fb"), None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, absent) = call(&state, Method::DELETE, &format!("{path}/never"), None).await;
    assert_eq!(status, StatusCode::OK, "{absent}");
    assert_eq!(strings(&absent["page"]["labels"]), ["on-call"]);

    let (status, removed) = call(&state, Method::DELETE, &format!("{path}/On%20Call"), None).await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    assert!(strings(&removed["page"]["labels"]).is_empty());

    for number in 0..MAX_LABELS_PER_PAGE {
        add(&mut connection, id, &format!("label-{number}")).await;
    }
    let (status, full) = call(
        &state,
        Method::POST,
        &path,
        Some(&json!({ "label": "one-more" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(full["code"], "page_label_limit");

    let unknown = Uuid::now_v7();
    let (status, _) = call(
        &state,
        Method::POST,
        &format!("/api/v1/pages/{unknown}/labels"),
        Some(&json!({ "label": "runbook" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guest_labels_nothing() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let id = create(&mut connection, None, "Deploying").await;
    add(&mut connection, id, "runbook").await;
    set_role(&mut connection, "guest").await;

    let body = json!({ "label": "mine" });
    for (method, path, body) in [
        (
            Method::POST,
            format!("/api/v1/pages/{id}/labels"),
            Some(&body),
        ),
        (
            Method::DELETE,
            format!("/api/v1/pages/{id}/labels/runbook"),
            None,
        ),
    ] {
        let (status, refusal) = call(&state, method.clone(), &path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(refusal["code"], "members_only", "{method} {path}");
    }
    assert_eq!(
        page_label::of_page(&mut connection, id)
            .await
            .expect("read"),
        ["runbook"]
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_search_filter_narrows_both_rankings_to_the_pages_carrying_a_label() {
    let (url, mut connection) = migrated_database().await;
    let mut state = app_state(&url).await;
    let fake = FakeEmbeddings::start().await;
    state.knowledge = KnowledgeSearch::new(fake.embeddings.clone());

    let labeled = page::create(
        &mut connection,
        None,
        edit("Restarting the cluster", "Drain each node, then reboot it."),
        author(None),
    )
    .await
    .expect("created")
    .page
    .id;
    page::create(
        &mut connection,
        None,
        edit("Restarting the printer", "Drain the tray, then reboot it."),
        author(None),
    )
    .await
    .expect("created");
    add(&mut connection, labeled, "On Call").await;

    // By words, both pages match, and the label keeps the one carrying it.
    assert_eq!(found(&state, "q=reboot").await.len(), 2);
    assert_eq!(
        found(&state, "q=reboot&label=on-call").await,
        ["Restarting the cluster"]
    );
    assert_eq!(
        found(&state, "q=reboot&label=On%20Call").await,
        ["Restarting the cluster"],
        "the filter reads a label as it is written"
    );
    assert!(found(&state, "q=reboot&label=never").await.is_empty());

    // By meaning alone, with no word of the query in either page, it narrows just the same.
    let mut unit = vec![0.0; DIMENSIONS];
    unit[0] = 1.0;
    for title_and_body in [
        "Restarting the cluster\n\nDrain each node, then reboot it.",
        "Restarting the printer\n\nDrain the tray, then reboot it.",
    ] {
        fake.pin(&format!("search_document: {title_and_body}"), unit.clone());
    }
    fake.pin("search_query: how do we bounce things", unit);
    let embedded = Embedder::new(
        connect_postgres(&url, 2).await.expect("a pool"),
        fake.embeddings,
    )
    .drain()
    .await
    .expect("the passages are embedded");
    assert_eq!(embedded, 2);
    assert_eq!(found(&state, "q=how+do+we+bounce+things").await.len(), 2);
    assert_eq!(
        found(&state, "q=how+do+we+bounce+things&label=on-call").await,
        ["Restarting the cluster"]
    );

    // The document carries its labels, and a label no page could carry is refused.
    let (_, answer) = get(&state, "/api/v1/knowledge/search?q=reboot&label=on-call").await;
    let document_id = answer["results"][0]["citation"]["documentId"]
        .as_str()
        .expect("a document");
    let (status, opened) = get(
        &state,
        &format!("/api/v1/knowledge/documents/{document_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    assert_eq!(strings(&opened["document"]["labels"]), ["on-call"]);

    let (status, refusal) = get(&state, "/api/v1/knowledge/search?q=reboot&label=a/b").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
}
