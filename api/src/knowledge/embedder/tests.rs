// Copyright © 2026 Jalapeno Labs

//! The embedder against Postgres and a fake embedding service (#92): every waiting passage
//! embedded as a document and recorded as the model's, a passage rewritten mid-pass left for
//! its new text, another model's embeddings forgotten and remade, and a service that serves
//! another model or refuses recording nothing.
//!
//! `#[ignore]`d so `cargo test` stays hermetic; `api/scripts/verify-migrations.sh` runs them.

use chrono::Utc;
use diesel::QueryableByName;
use diesel::sql_types::{Integer, Nullable, Text, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use super::{Embedder, EmbedderError};
use crate::connections::connect_postgres;
use crate::embeddings::fake::FakeEmbeddings;
use crate::embeddings::{DIMENSIONS, EmbeddingsError, MODEL};
use crate::models::agent::Author;
use crate::models::knowledge_document::{self, IndexedDocument, KnowledgeSource};
use crate::models::learning::{self, LearningContent};
use crate::test_support::{TEST_PERSON_ID, migrated_database};

#[derive(QueryableByName, Debug)]
struct Passage {
    #[diesel(sql_type = Integer)]
    position: i32,
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = Nullable<Text>)]
    embedding: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    embedding_model: Option<String>,
}

/// A saved learning, indexed as `passages` in order, and its id.
async fn indexed(connection: &mut AsyncPgConnection, title: &str, passages: &[String]) -> Uuid {
    let saved = learning::save(
        connection,
        LearningContent {
            title: title.to_owned(),
            body: "Body.".to_owned(),
            category: "pitfall".to_owned(),
            evidence: "Seen in the tests of #92.".to_owned(),
        },
        Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: Utc::now(),
        },
    )
    .await
    .expect("saved");
    index_as(connection, saved.learning.id, passages).await;
    saved.learning.id
}

/// Indexes the learning `id` again as `passages`.
async fn index_as(connection: &mut AsyncPgConnection, id: Uuid, passages: &[String]) {
    knowledge_document::upsert(
        connection,
        &IndexedDocument {
            source: KnowledgeSource::Learning(id),
            title: "A learning".to_owned(),
            url: None,
            occurred_at: Utc::now(),
            created_by: TEST_PERSON_ID,
            passages: passages.to_vec(),
        },
    )
    .await
    .expect("indexed");
}

/// The learning's passages in order, each embedding in pgvector's text form.
async fn passages(connection: &mut AsyncPgConnection, id: Uuid) -> Vec<Passage> {
    diesel::sql_query(
        "SELECT position, content, embedding::text AS embedding, embedding_model \
         FROM knowledge_passages \
         WHERE document_id = (SELECT id FROM knowledge_documents WHERE learning_id = $1) \
         ORDER BY position",
    )
    .bind::<SqlUuid, _>(id)
    .load(connection)
    .await
    .expect("passages load")
}

/// An embedding as pgvector writes it, read back as numbers.
fn numbers(embedding: &str) -> Vec<f32> {
    embedding
        .trim_matches(['[', ']'])
        .split(',')
        .map(|value| value.parse().expect("a number"))
        .collect()
}

/// The embedder, over the database at `url` and the fake.
async fn embedder(url: &secrecy::SecretString, fake: &FakeEmbeddings) -> Embedder {
    let database = connect_postgres(url, 2).await.expect("the pool connects");
    Embedder::new(database, fake.embeddings.clone())
}

fn texts(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("Passage number {index}."))
        .collect()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_pass_embeds_every_waiting_passage_as_a_document_made_by_the_model() {
    let (url, mut connection) = migrated_database().await;
    let fake = FakeEmbeddings::start().await;
    // More than one batch's worth, so the pass takes batch after batch.
    let id = indexed(&mut connection, "Many passages", &texts(40)).await;

    let stored = embedder(&url, &fake).await.drain().await.expect("a pass");

    assert_eq!(stored, 40);
    for passage in passages(&mut connection, id).await {
        assert_eq!(passage.embedding_model.as_deref(), Some(MODEL));
        let expected = FakeEmbeddings::vector_for(
            &format!("search_document: {}", passage.content),
            DIMENSIONS,
        );
        assert_eq!(
            numbers(passage.embedding.as_deref().expect("embedded")),
            expected,
            "passage {} holds the vector the model gave its text, exactly",
            passage.position
        );
    }
    let received = fake.received();
    assert_eq!(received.len(), 40, "each passage embedded once");
    assert!(
        received
            .iter()
            .all(|input| input.starts_with("search_document: ")),
        "every passage is embedded as a document"
    );

    let again = embedder(&url, &fake).await.drain().await.expect("a pass");
    assert_eq!(again, 0, "nothing waits once every passage is embedded");
    assert_eq!(
        fake.received().len(),
        40,
        "and the service is not asked again"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_passage_rewritten_while_it_was_embedded_waits_for_its_new_text() {
    let (url, mut connection) = migrated_database().await;
    let fake = FakeEmbeddings::start().await;
    let id = indexed(
        &mut connection,
        "Rewritten",
        &["kept".to_owned(), "old".to_owned()],
    )
    .await;

    // The pass takes both passages, and while the model works the source is saved again with
    // its second passage changed.
    let taken = knowledge_document::unembedded(&mut connection, 32)
        .await
        .expect("taken");
    let contents: Vec<&str> = taken
        .iter()
        .map(|passage| passage.content.as_str())
        .collect();
    let embedded = fake
        .embeddings
        .embed_passages(&contents)
        .await
        .expect("embedded");
    index_as(&mut connection, id, &["kept".to_owned(), "new".to_owned()]).await;
    let pairs: Vec<_> = taken.into_iter().zip(embedded).collect();
    let stored = knowledge_document::store_embeddings(&mut connection, MODEL, &pairs)
        .await
        .expect("stored");

    assert_eq!(stored, 1, "only the passage whose text held is recorded");
    let found = passages(&mut connection, id).await;
    assert_eq!(found[0].embedding_model.as_deref(), Some(MODEL));
    assert_eq!(found[1].content, "new");
    assert_eq!(
        found[1].embedding, None,
        "the new text never gets the old text's embedding"
    );

    let stored = embedder(&url, &fake).await.drain().await.expect("a pass");
    assert_eq!(stored, 1, "the new text is embedded next");
    assert_eq!(
        fake.received().last().map(String::as_str),
        Some("search_document: new")
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn embeddings_of_another_model_are_forgotten_and_made_again() {
    let (url, mut connection) = migrated_database().await;
    let fake = FakeEmbeddings::start().await;
    let ours = indexed(&mut connection, "Ours", &texts(2)).await;
    let embedder = embedder(&url, &fake).await;
    embedder.drain().await.expect("a pass");
    let theirs = indexed(&mut connection, "Theirs", &texts(3)).await;
    let older = format!("[{}]", vec!["0.5"; DIMENSIONS].join(","));
    diesel::sql_query(
        "UPDATE knowledge_passages SET embedding = $1::vector, embedding_model = 'an-older-model' \
         WHERE document_id = (SELECT id FROM knowledge_documents WHERE learning_id = $2)",
    )
    .bind::<Text, _>(older)
    .bind::<SqlUuid, _>(theirs)
    .execute(&mut connection)
    .await
    .expect("embedded by another model");

    embedder.forget_other_models().await.expect("forgotten");

    assert!(
        passages(&mut connection, theirs)
            .await
            .iter()
            .all(|passage| passage.embedding.is_none() && passage.embedding_model.is_none()),
        "another model's embeddings are forgotten"
    );
    assert!(
        passages(&mut connection, ours)
            .await
            .iter()
            .all(|passage| passage.embedding_model.as_deref() == Some(MODEL)),
        "the model's own are kept"
    );
    let stored = embedder.drain().await.expect("a pass");
    assert_eq!(
        stored, 3,
        "the forgotten passages are embedded again by the model"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_service_serving_another_model_records_nothing() {
    let (url, mut connection) = migrated_database().await;
    let fake = FakeEmbeddings::start().await;
    fake.serve("nomic-ai/nomic-embed-text-v1.5@another-revision");
    let id = indexed(&mut connection, "Waiting", &texts(2)).await;

    let pass = embedder(&url, &fake).await.drain().await;

    assert!(
        matches!(
            pass,
            Err(EmbedderError::Embeddings(
                EmbeddingsError::WrongModel { .. }
            ))
        ),
        "{pass:?}"
    );
    assert!(fake.received().is_empty(), "nothing is sent to be embedded");
    assert!(
        passages(&mut connection, id)
            .await
            .iter()
            .all(|passage| passage.embedding.is_none())
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_refusing_service_ends_the_pass_and_the_next_one_embeds() {
    let (url, mut connection) = migrated_database().await;
    let fake = FakeEmbeddings::start().await;
    let id = indexed(&mut connection, "Waiting", &texts(2)).await;
    let embedder = embedder(&url, &fake).await;

    fake.refuse(true);
    let pass = embedder.drain().await;
    assert!(
        matches!(
            pass,
            Err(EmbedderError::Embeddings(EmbeddingsError::Refused { .. }))
        ),
        "{pass:?}"
    );
    assert!(
        passages(&mut connection, id)
            .await
            .iter()
            .all(|passage| passage.embedding.is_none())
    );

    fake.refuse(false);
    assert_eq!(embedder.drain().await.expect("a pass"), 2);
}
