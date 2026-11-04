// Copyright © 2026 Jalapeno Labs

use reqwest::StatusCode;
use url::Url;

use super::fake::FakeEmbeddings;
use super::{DIMENSIONS, Embedding, Embeddings, EmbeddingsError, MODEL};

#[tokio::test]
async fn passages_are_embedded_as_documents_and_queries_as_queries() {
    let fake = FakeEmbeddings::start().await;

    let passages = fake
        .embeddings
        .embed_passages(&[
            "Backups run at 08:00 UTC.",
            "Restore replaces each database.",
        ])
        .await
        .expect("embedded");
    let query = fake
        .embeddings
        .embed_query("when do backups run?")
        .await
        .expect("embedded");

    assert_eq!(passages.len(), 2, "one embedding a passage");
    assert_eq!(
        fake.received(),
        [
            "search_document: Backups run at 08:00 UTC.",
            "search_document: Restore replaces each database.",
            "search_query: when do backups run?",
        ],
        "the model is told which side of a search each text is on"
    );
    let expected = FakeEmbeddings::vector_for("search_query: when do backups run?", DIMENSIONS);
    assert_eq!(query, Embedding::new(expected).expect("valid"));
}

#[tokio::test]
async fn nothing_to_embed_asks_nothing() {
    let fake = FakeEmbeddings::start().await;
    let none: [&str; 0] = [];
    let embeddings = fake.embeddings.embed_passages(&none).await.expect("empty");
    assert!(embeddings.is_empty());
    assert!(fake.received().is_empty(), "no request for no passages");
}

#[tokio::test]
async fn the_model_served_is_checked_by_name() {
    let fake = FakeEmbeddings::start().await;
    fake.embeddings
        .check_model()
        .await
        .expect("the expected model");

    fake.serve("nomic-ai/nomic-embed-text-v1.5@another-revision");
    let checked = fake.embeddings.check_model().await;
    assert!(
        matches!(&checked, Err(EmbeddingsError::WrongModel { serving })
            if serving == "nomic-ai/nomic-embed-text-v1.5@another-revision"),
        "{checked:?}"
    );
}

#[tokio::test]
async fn an_embedding_of_another_size_is_refused() {
    let fake = FakeEmbeddings::start().await;
    fake.answer_dimensions(384);
    let embedded = fake.embeddings.embed_passages(&["A passage."]).await;
    assert!(
        matches!(&embedded, Err(EmbeddingsError::Malformed(reason)) if reason.contains("384")),
        "{embedded:?}"
    );
}

#[tokio::test]
async fn a_refusal_carries_its_status_and_body() {
    let fake = FakeEmbeddings::start().await;
    fake.refuse(true);
    let embedded = fake.embeddings.embed_passages(&["A passage."]).await;
    assert!(
        matches!(&embedded, Err(EmbeddingsError::Refused { status, body })
            if *status == StatusCode::TOO_MANY_REQUESTS && body.contains("overloaded")),
        "{embedded:?}"
    );
}

#[tokio::test]
async fn a_service_that_is_not_there_is_unreachable() {
    // Port 9 is discard, which nothing here listens on.
    let embeddings = Embeddings::new(
        reqwest::Client::new(),
        Url::parse("http://127.0.0.1:9/").expect("a URL"),
    );
    let checked = embeddings.check_model().await;
    assert!(
        matches!(checked, Err(EmbeddingsError::Unreachable(_))),
        "{checked:?}"
    );
}

#[test]
fn an_embedding_is_written_as_pgvector_reads_it() {
    let mut values = vec![0.0_f32; DIMENSIONS];
    values[0] = 0.1;
    values[1] = -2.5e-7;
    let text = Embedding::new(values).expect("valid").to_pgvector();

    assert!(text.starts_with("[0.1,-0.00000025,0,"), "{}", &text[..40]);
    assert!(text.ends_with(",0]"));
    let read: Vec<f32> = text
        .trim_matches(['[', ']'])
        .split(',')
        .map(|value| value.parse().expect("a number"))
        .collect();
    assert_eq!(read.len(), DIMENSIONS);
    assert_eq!(
        read[1].to_bits(),
        (-2.5e-7_f32).to_bits(),
        "read back exactly"
    );
}

#[test]
fn an_embedding_holding_what_is_not_a_number_is_refused() {
    let mut values = vec![0.0_f32; DIMENSIONS];
    values[7] = f32::NAN;
    assert!(matches!(
        Embedding::new(values),
        Err(EmbeddingsError::Malformed(_))
    ));
}

#[test]
fn the_model_names_its_revision() {
    let (name, revision) = MODEL.split_once('@').expect("a model and a revision");
    assert_eq!(name, "nomic-ai/nomic-embed-text-v1.5");
    assert_eq!(revision.len(), 40, "a full commit, which never moves");
    assert!(
        include_str!("../../../embeddings/Dockerfile")
            .contains(&format!("ARG MODEL_REVISION={revision}")),
        "the image serves the revision the engine records"
    );
}

/// The cosine between two of the service's embeddings, which it normalizes: their dot product.
fn similarity(left: &Embedding, right: &Embedding) -> f32 {
    left.0.iter().zip(&right.0).map(|(a, b)| a * b).sum()
}

/// Against the real image, opt-in: `EMBEDDINGS_TEST_URL` names a running `embeddings` service,
/// such as `docker run --publish 127.0.0.1::8080 elysium-embeddings:local`.
#[tokio::test]
#[ignore = "needs EMBEDDINGS_TEST_URL, a running embedding service"]
async fn a_running_service_serves_the_model_and_ranks_by_meaning() {
    // `verify-migrations.sh` runs every ignored test; this one needs a service it may not have.
    let Ok(url) = std::env::var("EMBEDDINGS_TEST_URL") else {
        eprintln!("skipped: set EMBEDDINGS_TEST_URL to embed through a running service");
        return;
    };
    let embeddings = Embeddings::new(reqwest::Client::new(), Url::parse(&url).expect("a URL"));
    embeddings
        .check_model()
        .await
        .expect("the service serves MODEL");

    let passages = embeddings
        .embed_passages(&[
            "The restore replaces each database whole with its dump. Everything written since \
             the backup is lost.",
            "Colors come from the Matter VS Code theme and live in matter.css.",
        ])
        .await
        .expect("embedded");
    let query = embeddings
        .embed_query("how do I restore a database backup?")
        .await
        .expect("embedded");

    let length = query
        .0
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    assert!(
        (length - 1.0).abs() < 1e-4,
        "unit length, so cosine is the dot product"
    );
    let relevant = similarity(&query, &passages[0]);
    let unrelated = similarity(&query, &passages[1]);
    assert!(
        relevant > unrelated + 0.2,
        "the passage about restoring ranks first: {relevant} against {unrelated}"
    );
}
