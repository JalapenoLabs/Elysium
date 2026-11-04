// Copyright © 2026 Jalapeno Labs

//! The knowledge base's index tables (#91): documents live and die with their source, passages
//! with their document, each source upserts its one document, and the full-text and embedding
//! columns answer the queries the search is built on. See `docs/knowledge.md`.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs it.

use diesel::QueryableByName;
use diesel::sql_types::{BigInt, Integer, Text, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl, SimpleAsyncConnection};
use uuid::{Uuid, uuid};

use crate::test_support::migrated_database;

const RECORD: Uuid = uuid!("00000000-0000-7000-8000-0000000000e1");
const DOCUMENT: Uuid = uuid!("00000000-0000-7000-8000-0000000000d1");

/// A Slack message on the test person's timeline, indexed as one document of two passages.
const INDEXED_MESSAGE: &str = "
INSERT INTO timeline_records (id, source_kind, external_id, occurred_at, title, body, created_by)
VALUES ('00000000-0000-7000-8000-0000000000e1', 'slack', 'C1/1730000000.000100', now(), '',
        'Deploying WAY-123 failed: ECONNREFUSED at ELYSIUM_PUBLIC_URL. Ada is rolling it back.',
        '0199a3c4-0000-7000-8000-00000000beef');
INSERT INTO knowledge_documents (id, source, timeline_record_id, occurred_at, content_sha256,
                                 created_by)
VALUES ('00000000-0000-7000-8000-0000000000d1', 'slack', '00000000-0000-7000-8000-0000000000e1',
        now(), repeat('a', 64), '0199a3c4-0000-7000-8000-00000000beef');
INSERT INTO knowledge_passages (document_id, position, content) VALUES
 ('00000000-0000-7000-8000-0000000000d1', 0, 'Deploying WAY-123 failed: ECONNREFUSED at ELYSIUM_PUBLIC_URL.'),
 ('00000000-0000-7000-8000-0000000000d1', 1, 'Ada is rolling it back.');
";

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

#[derive(QueryableByName)]
struct Position {
    #[diesel(sql_type = Integer)]
    position: i32,
}

async fn count(connection: &mut AsyncPgConnection, query: &str) -> i64 {
    diesel::sql_query(format!(
        "SELECT count(*) AS count FROM ({query}) AS matched"
    ))
    .get_result::<Count>(connection)
    .await
    .expect("the query runs")
    .count
}

/// An embedding of the model's size with every component `value`, as pgvector reads text.
fn embedding_of(value: f32) -> String {
    let components = vec![value.to_string(); 768];
    format!("[{}]", components.join(","))
}

async fn indexed_message() -> AsyncPgConnection {
    let (_url, mut connection) = migrated_database().await;
    connection
        .batch_execute(INDEXED_MESSAGE)
        .await
        .expect("a record, its document, and its passages are stored");
    connection
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn deleting_a_record_removes_its_document_and_passages() {
    let mut connection = indexed_message().await;

    diesel::sql_query("DELETE FROM timeline_records WHERE id = $1")
        .bind::<SqlUuid, _>(RECORD)
        .execute(&mut connection)
        .await
        .expect("the record is deleted");

    assert_eq!(
        count(&mut connection, "SELECT 1 FROM knowledge_documents").await,
        0
    );
    assert_eq!(
        count(&mut connection, "SELECT 1 FROM knowledge_passages").await,
        0
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_document_needs_a_source_that_exists() {
    let (_url, mut connection) = migrated_database().await;

    let without_source = connection
        .batch_execute(
            "INSERT INTO knowledge_documents (id, source, occurred_at, content_sha256, created_by) \
             VALUES (gen_random_uuid(), 'slack', now(), repeat('a', 64), \
                     '0199a3c4-0000-7000-8000-00000000beef')",
        )
        .await;
    assert!(without_source.is_err(), "a document names its source");

    let gone_source = connection
        .batch_execute(
            "INSERT INTO knowledge_documents (id, source, timeline_record_id, occurred_at, \
                                              content_sha256, created_by) \
             VALUES (gen_random_uuid(), 'slack', gen_random_uuid(), now(), repeat('a', 64), \
                     '0199a3c4-0000-7000-8000-00000000beef')",
        )
        .await;
    assert!(
        gone_source.is_err(),
        "a document for a deleted record is refused"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn indexing_a_source_again_upserts_its_one_document() {
    let mut connection = indexed_message().await;

    diesel::sql_query(
        "INSERT INTO knowledge_documents (id, source, timeline_record_id, occurred_at, \
                                          content_sha256, created_by) \
         VALUES (gen_random_uuid(), 'slack', $1, now(), repeat('b', 64), \
                 '0199a3c4-0000-7000-8000-00000000beef') \
         ON CONFLICT (timeline_record_id) DO UPDATE SET content_sha256 = excluded.content_sha256",
    )
    .bind::<SqlUuid, _>(RECORD)
    .execute(&mut connection)
    .await
    .expect("the second indexing upserts");

    let query = format!(
        "SELECT 1 FROM knowledge_documents WHERE id = '{DOCUMENT}' AND content_sha256 = repeat('b', 64)"
    );
    assert_eq!(
        count(&mut connection, &query).await,
        1,
        "one document, updated in place"
    );
    assert_eq!(
        count(&mut connection, "SELECT 1 FROM knowledge_documents").await,
        1
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn full_text_finds_exact_keys_and_sentences() {
    let mut connection = indexed_message().await;

    for (query, expected) in [
        ("WAY-123", 0),
        ("ELYSIUM_PUBLIC_URL", 0),
        ("econnrefused", 0),
        ("who is rolling it back", 1),
    ] {
        let matches: Vec<Position> = diesel::sql_query(
            "SELECT position FROM knowledge_passages \
             WHERE search_vector @@ websearch_to_tsquery('english', $1) ORDER BY position",
        )
        .bind::<Text, _>(query)
        .load(&mut connection)
        .await
        .expect("the search runs");
        let positions: Vec<i32> = matches.iter().map(|matched| matched.position).collect();
        assert_eq!(positions, [expected], "{query} finds passage {expected}");
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn embeddings_are_the_models_size_and_name_their_model() {
    let mut connection = indexed_message().await;

    let unnamed = diesel::sql_query(
        "UPDATE knowledge_passages SET embedding = $1::vector WHERE position = 0",
    )
    .bind::<Text, _>(embedding_of(0.1))
    .execute(&mut connection)
    .await;
    assert!(
        unnamed.is_err(),
        "an embedding names the model that made it"
    );

    let wrong_size = diesel::sql_query(
        "UPDATE knowledge_passages SET embedding = '[0.1, 0.2, 0.3]', embedding_model = 'test' \
         WHERE position = 0",
    )
    .execute(&mut connection)
    .await;
    assert!(
        wrong_size.is_err(),
        "an embedding has the model's 768 dimensions"
    );

    for (position, value) in [(0, 0.1_f32), (1, -0.1)] {
        diesel::sql_query(
            "UPDATE knowledge_passages SET embedding = $1::vector, embedding_model = 'test' \
             WHERE position = $2",
        )
        .bind::<Text, _>(embedding_of(value))
        .bind::<Integer, _>(position)
        .execute(&mut connection)
        .await
        .expect("an embedding of the right size is stored");
    }
    let unembedded = "SELECT 1 FROM knowledge_passages WHERE embedding IS NULL";
    assert_eq!(count(&mut connection, unembedded).await, 0);

    let nearest: Position = diesel::sql_query(
        "SELECT position FROM knowledge_passages ORDER BY embedding <=> $1::vector LIMIT 1",
    )
    .bind::<Text, _>(embedding_of(0.2))
    .get_result(&mut connection)
    .await
    .expect("the nearest neighbour is found");
    assert_eq!(
        nearest.position, 0,
        "cosine distance ranks the same direction first"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_learning_is_a_source_of_its_own_that_takes_its_document_with_it() {
    let mut connection = indexed_message().await;
    connection
        .batch_execute(
            "INSERT INTO learnings (id, title, body, category, evidence, created_by, created_at) \
             VALUES ('00000000-0000-7000-8000-0000000000f1', 'Pin every tool version', \
                     'Never use latest.', 'convention', 'CI drifted.', \
                     '0199a3c4-0000-7000-8000-00000000beef', now()); \
             INSERT INTO knowledge_documents (id, source, learning_id, occurred_at, \
                                              content_sha256, created_by) \
             VALUES (gen_random_uuid(), 'learning', '00000000-0000-7000-8000-0000000000f1', \
                     now(), repeat('a', 64), '0199a3c4-0000-7000-8000-00000000beef');",
        )
        .await
        .expect("a learning and its document are stored");

    let both = diesel::sql_query(
        "UPDATE knowledge_documents SET learning_id = '00000000-0000-7000-8000-0000000000f1' \
         WHERE id = $1",
    )
    .bind::<SqlUuid, _>(DOCUMENT)
    .execute(&mut connection)
    .await;
    assert!(both.is_err(), "a document has exactly one source");

    connection
        .batch_execute("DELETE FROM learnings WHERE id = '00000000-0000-7000-8000-0000000000f1'")
        .await
        .expect("the learning is deleted");
    let learning_documents = "SELECT 1 FROM knowledge_documents WHERE source = 'learning'";
    assert_eq!(count(&mut connection, learning_documents).await, 0);
    assert_eq!(
        count(&mut connection, "SELECT 1 FROM knowledge_documents").await,
        1,
        "the record's document stays"
    );
}
