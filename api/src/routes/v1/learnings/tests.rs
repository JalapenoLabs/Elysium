// Copyright © 2026 Jalapeno Labs

//! Learnings against Postgres (#95): saving by title, every revision kept and undone, deleting
//! and restoring, the knowledge index following each change in the same transaction, and the
//! routes refusing guests and the feed refusing everyone but admins.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use diesel::QueryableByName;
use diesel::sql_types::{BigInt, Integer, Nullable, Text, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::models::knowledge_document::{self, IndexedDocument, KnowledgeSource};
use crate::models::learning::{
    self, Agent, Author, HistoryKind, LearningCategory, LearningChanges, LearningContent,
    LearningError,
};
use crate::routes;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

#[derive(QueryableByName)]
struct Passage {
    #[diesel(sql_type = Integer)]
    position: i32,
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = Nullable<Text>)]
    embedding_model: Option<String>,
}

fn content(title: &str, body: &str) -> LearningContent {
    LearningContent {
        title: title.to_owned(),
        body: body.to_owned(),
        category: LearningCategory::Pitfall,
        evidence: "`diesel migration revert --all` left schema.rs empty on WAY-123.".to_owned(),
    }
}

fn author(agent: Option<&Agent>) -> Author<'_> {
    Author {
        user_id: TEST_PERSON_ID,
        agent,
        at: Utc::now(),
    }
}

fn kinds(recorded: &learning::Recorded) -> Vec<&str> {
    recorded
        .history
        .iter()
        .map(|entry| entry.kind.as_str())
        .collect()
}

/// How many indexed passages of learning `id` match `query`, parsed as searches parse it.
async fn matches(connection: &mut AsyncPgConnection, id: Uuid, query: &str) -> i64 {
    diesel::sql_query(
        "SELECT count(*) AS count FROM knowledge_passages p \
         JOIN knowledge_documents d ON d.id = p.document_id \
         WHERE d.learning_id = $1 AND d.source = 'learning' \
           AND p.search_vector @@ websearch_to_tsquery('english', $2)",
    )
    .bind::<SqlUuid, _>(id)
    .bind::<Text, _>(query)
    .get_result::<Count>(connection)
    .await
    .expect("the search runs")
    .count
}

async fn documents_of(connection: &mut AsyncPgConnection, id: Uuid) -> i64 {
    diesel::sql_query("SELECT count(*) AS count FROM knowledge_documents WHERE learning_id = $1")
        .bind::<SqlUuid, _>(id)
        .get_result::<Count>(connection)
        .await
        .expect("the count runs")
        .count
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_saved_learning_is_searchable_at_once_by_exact_keys_and_by_sentences() {
    let (_url, mut connection) = migrated_database().await;
    let agent = Agent::mcp_client("claude-code-client", "Claude Code").expect("recordable");

    let saved = learning::save(
        &mut connection,
        content("Diesel's revert rewrites schema.rs", "Use redo instead."),
        author(Some(&agent)),
    )
    .await
    .expect("saved");

    assert_eq!(kinds(&saved), [HistoryKind::Created.as_str()]);
    assert_eq!(
        saved.learning.agent.as_deref(),
        Some("mcp:claude-code-client")
    );
    assert_eq!(saved.learning.agent_name.as_deref(), Some("Claude Code"));
    let id = saved.learning.id;
    assert_eq!(matches(&mut connection, id, "WAY-123").await, 1);
    assert_eq!(
        matches(&mut connection, id, "what does a revert rewrite").await,
        1
    );
    assert_eq!(
        matches(&mut connection, id, "kubernetes").await,
        0,
        "a word it does not hold finds nothing"
    );

    let unembedded: Count = diesel::sql_query(
        "SELECT count(*) AS count FROM knowledge_passages p \
         JOIN knowledge_documents d ON d.id = p.document_id \
         WHERE d.learning_id = $1 AND p.embedding IS NULL",
    )
    .bind::<SqlUuid, _>(id)
    .get_result(&mut connection)
    .await
    .expect("the count runs");
    assert_eq!(
        unembedded.count, 1,
        "its passages wait for the embedding model"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn saving_a_title_again_in_any_case_revises_it_and_keeps_every_revision() {
    let (_url, mut connection) = migrated_database().await;
    let first = learning::save(
        &mut connection,
        content("Diesel's revert rewrites schema.rs", "Use redo instead."),
        author(None),
    )
    .await
    .expect("saved");

    let again = learning::save(
        &mut connection,
        content(
            "DIESEL'S REVERT REWRITES SCHEMA.RS",
            "Use redo --all --locked-schema.",
        ),
        author(None),
    )
    .await
    .expect("revised");

    assert_eq!(
        again.learning.id, first.learning.id,
        "one learning per title"
    );
    assert_eq!(again.learning.title, "DIESEL'S REVERT REWRITES SCHEMA.RS");
    assert_eq!(kinds(&again), [HistoryKind::Revised.as_str()]);
    assert_eq!(
        again.history[0].data["changes"]["body"],
        json!({ "from": "Use redo instead.", "to": "Use redo --all --locked-schema." })
    );
    let history = learning::history(&mut connection, first.learning.id)
        .await
        .expect("history");
    assert_eq!(
        history.len(),
        2,
        "the first save and the revision are both kept"
    );
    assert_eq!(
        matches(&mut connection, first.learning.id, "locked-schema").await,
        1,
        "the index holds the revised text"
    );

    let unchanged = learning::save(
        &mut connection,
        content(
            "DIESEL'S REVERT REWRITES SCHEMA.RS",
            "Use redo --all --locked-schema.",
        ),
        author(None),
    )
    .await
    .expect("saved");
    assert!(
        unchanged.history.is_empty(),
        "saving the same words records nothing"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn undoing_a_revision_puts_back_what_it_replaced_and_is_itself_recorded() {
    let (_url, mut connection) = migrated_database().await;
    let saved = learning::save(
        &mut connection,
        content("Rebuild indexes after a collation change", "Run REINDEX."),
        author(None),
    )
    .await
    .expect("saved");
    let id = saved.learning.id;
    let corrected = learning::correct(
        &mut connection,
        id,
        LearningChanges {
            body: Some("Run REINDEX DATABASE on every database.".to_owned()),
            category: Some(LearningCategory::Procedure),
            ..LearningChanges::default()
        },
        author(None),
    )
    .await
    .expect("corrected");
    let revision = corrected.history[0].id;

    let undone = learning::undo(&mut connection, id, revision, author(None))
        .await
        .expect("undone");

    assert_eq!(undone.learning.body, "Run REINDEX.");
    assert_eq!(undone.learning.category, LearningCategory::Pitfall);
    assert_eq!(undone.history[0].data["undid"], json!(revision));
    assert_eq!(matches(&mut connection, id, "every database").await, 0);
    let history = learning::history(&mut connection, id)
        .await
        .expect("history");
    assert_eq!(history.len(), 3, "the undo is a change of its own");

    let again = learning::undo(&mut connection, id, revision, author(None)).await;
    assert!(
        matches!(again, Err(LearningError::NothingToUndo(_))),
        "a revision undone already has nothing left to put back"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_deleted_learning_leaves_the_index_until_it_comes_back() {
    let (_url, mut connection) = migrated_database().await;
    let saved = learning::save(
        &mut connection,
        content("Pin every tool version", "Never use latest."),
        author(None),
    )
    .await
    .expect("saved");
    let id = saved.learning.id;

    let deleted = learning::delete(&mut connection, id, author(None))
        .await
        .expect("deleted");
    assert!(deleted.learning.deleted_at.is_some());
    assert_eq!(documents_of(&mut connection, id).await, 0);
    let listed = learning::list(&mut connection, false)
        .await
        .expect("listed");
    assert!(listed.is_empty(), "a deleted learning is not listed");

    let undone = learning::undo(&mut connection, id, deleted.history[0].id, author(None))
        .await
        .expect("the deletion is undone");
    assert_eq!(kinds(&undone), [HistoryKind::Restored.as_str()]);
    assert_eq!(documents_of(&mut connection, id).await, 1);

    learning::delete(&mut connection, id, author(None))
        .await
        .expect("deleted again");
    let revived = learning::save(
        &mut connection,
        content("pin every tool version", "Never use latest; pin by digest."),
        author(None),
    )
    .await
    .expect("saving its title brings it back");
    assert_eq!(revived.learning.id, id);
    assert!(revived.learning.deleted_at.is_none());
    assert_eq!(
        kinds(&revived),
        [
            HistoryKind::Restored.as_str(),
            HistoryKind::Revised.as_str()
        ]
    );
    assert_eq!(matches(&mut connection, id, "digest").await, 1);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_correction_cannot_take_another_learnings_title_or_change_a_deleted_one() {
    let (_url, mut connection) = migrated_database().await;
    learning::save(
        &mut connection,
        content("First takeaway", "One."),
        author(None),
    )
    .await
    .expect("saved");
    let second = learning::save(
        &mut connection,
        content("Second takeaway", "Two."),
        author(None),
    )
    .await
    .expect("saved");
    let id = second.learning.id;

    let taken = learning::correct(
        &mut connection,
        id,
        LearningChanges {
            title: Some("first TAKEAWAY".to_owned()),
            ..LearningChanges::default()
        },
        author(None),
    )
    .await;
    assert!(matches!(taken, Err(LearningError::TitleTaken)));

    learning::delete(&mut connection, id, author(None))
        .await
        .expect("deleted");
    let deleted = learning::correct(
        &mut connection,
        id,
        LearningChanges {
            body: Some("Changed.".to_owned()),
            ..LearningChanges::default()
        },
        author(None),
    )
    .await;
    assert!(matches!(deleted, Err(LearningError::Deleted)));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn indexing_again_keeps_the_embedding_of_every_passage_whose_text_held() {
    let (_url, mut connection) = migrated_database().await;
    let saved = learning::save(
        &mut connection,
        content("A learning", "Body."),
        author(None),
    )
    .await
    .expect("saved");
    let source = KnowledgeSource::Learning(saved.learning.id);
    let document = |passages: &[&str]| IndexedDocument {
        source,
        title: "A learning".to_owned(),
        url: None,
        occurred_at: Utc::now(),
        created_by: TEST_PERSON_ID,
        passages: passages.iter().map(|&passage| passage.to_owned()).collect(),
    };

    knowledge_document::upsert(&mut connection, &document(&["kept", "moved", "dropped"]))
        .await
        .expect("indexed");
    let embedding = format!("[{}]", vec!["0.5"; 768].join(","));
    diesel::sql_query(
        "UPDATE knowledge_passages SET embedding = $1::vector, embedding_model = 'test' \
         WHERE document_id = (SELECT id FROM knowledge_documents WHERE learning_id = $2)",
    )
    .bind::<Text, _>(embedding)
    .bind::<SqlUuid, _>(saved.learning.id)
    .execute(&mut connection)
    .await
    .expect("embedded");

    knowledge_document::upsert(&mut connection, &document(&["kept", "changed"]))
        .await
        .expect("indexed again");

    let passages: Vec<Passage> = diesel::sql_query(
        "SELECT position, content, embedding_model FROM knowledge_passages \
         WHERE document_id = (SELECT id FROM knowledge_documents WHERE learning_id = $1) \
         ORDER BY position",
    )
    .bind::<SqlUuid, _>(saved.learning.id)
    .load(&mut connection)
    .await
    .expect("passages load");
    let found: Vec<(i32, &str, Option<&str>)> = passages
        .iter()
        .map(|passage| {
            (
                passage.position,
                passage.content.as_str(),
                passage.embedding_model.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [(0, "kept", Some("test")), (1, "changed", None)],
        "a held passage keeps its embedding, a moved one waits again, and the tail goes"
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
async fn members_save_and_revise_by_title_and_only_admins_read_the_feed() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let learning = json!({
        "title": "  Pin   every tool version ",
        "body": "Never use latest.",
        "category": "convention",
        "evidence": "CI drifted from production on an unpinned image.",
    });

    let (status, created) = call(&state, Method::POST, "/api/v1/learnings", Some(&learning)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["outcome"], "created");
    assert_eq!(
        created["learning"]["title"], "Pin every tool version",
        "the title is kept on one line, its spacing collapsed"
    );

    let (status, unchanged) =
        call(&state, Method::POST, "/api/v1/learnings", Some(&learning)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged["outcome"], "unchanged");

    let (status, feed) = call(&state, Method::GET, "/api/v1/learnings/feed", None).await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    assert_eq!(feed["entries"][0]["entry"]["kind"], "created");
    assert_eq!(
        feed["entries"][0]["learning"]["title"],
        "Pin every tool version"
    );
    assert_eq!(feed["nextBefore"], Value::Null, "a short page is the last");

    set_role(&mut connection, "member").await;
    let (status, refused) = call(&state, Method::GET, "/api/v1/learnings/feed", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(refused["code"], "admin_only");
    let (status, _) = call(&state, Method::GET, "/api/v1/learnings", None).await;
    assert_eq!(status, StatusCode::OK, "a member reads every learning");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_guest_reaches_no_learning() {
    let (url, mut connection) = migrated_database().await;
    let state = app_state(&url).await;
    let saved = learning::save(
        &mut connection,
        content("A takeaway", "Body."),
        author(None),
    )
    .await
    .expect("saved");
    set_role(&mut connection, "guest").await;

    let id = saved.learning.id;
    let entry = saved.history[0].id;
    let body = json!({
        "title": "Another takeaway",
        "body": "Body.",
        "category": "decision",
        "evidence": "Said in standup.",
    });
    for (method, path, body) in [
        (Method::GET, "/api/v1/learnings".to_owned(), None),
        (Method::POST, "/api/v1/learnings".to_owned(), Some(&body)),
        (Method::GET, format!("/api/v1/learnings/{id}"), None),
        (
            Method::PATCH,
            format!("/api/v1/learnings/{id}"),
            Some(&body),
        ),
        (Method::DELETE, format!("/api/v1/learnings/{id}"), None),
        (
            Method::POST,
            format!("/api/v1/learnings/{id}/restore"),
            None,
        ),
        (Method::GET, format!("/api/v1/learnings/{id}/history"), None),
        (
            Method::POST,
            format!("/api/v1/learnings/{id}/history/{entry}/undo"),
            None,
        ),
    ] {
        let (status, refused) = call(&state, method.clone(), &path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(refused["code"], "members_only", "{method} {path}");
    }
    assert_eq!(
        learning::list(&mut connection, false)
            .await
            .expect("listed")
            .len(),
        1,
        "nothing a guest sent was saved"
    );
}
