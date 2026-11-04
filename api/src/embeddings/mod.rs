// Copyright © 2026 Jalapeno Labs

//! The embedding model: turns passages and queries into the vectors meaning is searched by.
//!
//! Anthropic offers no embedding model, and no text may leave the host, so Elysium runs a small
//! open one itself: nomic-embed-text-v1.5, served by Hugging Face's text-embeddings-inference
//! from the image `embeddings/` builds, as the compose service `embeddings`. Its network has no
//! route out, and only the engine and the API join it. See `docs/knowledge.md`, Embeddings.
//!
//! [`Embeddings`] is the client both processes use: the engine's embedder
//! (`crate::knowledge::embedder`) embeds passages with [`Embeddings::embed_passages`], and a
//! search embeds its query with [`Embeddings::embed_query`]. The model is trained with a task
//! prefix on each side, so the two are never interchangeable: a passage is embedded as
//! `search_document: <text>` and a query as `search_query: <text>`.
//!
//! Every vector is checked as it arrives: [`DIMENSIONS`] finite numbers, the size of
//! `knowledge_passages.embedding`. [`Embeddings::check_model`] asks the service which model it
//! serves, so nothing is recorded as made by [`MODEL`] unless it was.

use std::fmt::Write as _;
use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use url::Url;

/// The model and revision every embedding is made by, as the service names it
/// (`--served-model-name` in `embeddings/Dockerfile`, which must change with it). It is what
/// `knowledge_passages.embedding_model` records, so changing it re-embeds every passage.
pub const MODEL: &str = "nomic-ai/nomic-embed-text-v1.5@e5cf08aadaa33385f5990def41f7a23405aec398";

/// The size of every embedding: nomic-embed-text-v1.5's, and the `vector(768)` column's. Another
/// size is a migration that re-embeds everything.
pub const DIMENSIONS: usize = 768;

/// The most texts one request carries: the service's `--max-client-batch-size`, left at its
/// default. It batches the model's work itself, eight texts at a time on its CPU backend.
pub const MAX_BATCH: usize = 32;

/// What the model is told a passage is, ahead of its text.
const DOCUMENT_PREFIX: &str = "search_document: ";

/// What the model is told a query is, ahead of its text.
const QUERY_PREFIX: &str = "search_query: ";

/// How long one request may take. A full batch took about seven seconds on 4 CPUs of a Ryzen 9
/// 7950X (`docs/knowledge.md`, Embeddings); this covers a much slower host and the model still
/// warming up, and still lets go of a service that hangs.
const REQUEST_TIMEOUT: Duration = Duration::from_mins(2);

/// How much of a refusal's body an error keeps. The service answers a short JSON error.
const ERROR_BODY_LIMIT: usize = 500;

/// One text's embedding: [`DIMENSIONS`] finite numbers, of unit length as the service
/// normalizes them.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding(Vec<f32>);

impl Embedding {
    /// Checks `values` as an embedding: exactly [`DIMENSIONS`] of them, every one finite.
    ///
    /// # Errors
    /// [`EmbeddingsError::Malformed`] naming what is wrong.
    pub fn new(values: Vec<f32>) -> Result<Self, EmbeddingsError> {
        if values.len() != DIMENSIONS {
            return Err(EmbeddingsError::Malformed(format!(
                "an embedding of {} dimensions, not {DIMENSIONS}",
                values.len()
            )));
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Err(EmbeddingsError::Malformed(
                "an embedding holding a number that is not finite".to_owned(),
            ));
        }
        Ok(Self(values))
    }

    /// The embedding in pgvector's text form, `[0.1,-0.2,...]`, to bind as text and cast to
    /// `vector`. Rust writes each `f32` as the shortest decimal that reads back as the same
    /// number, and pgvector reads it as an `f32`, so nothing is lost on the way.
    pub fn to_pgvector(&self) -> String {
        let mut text = String::with_capacity(self.0.len() * 12);
        text.push('[');
        for (index, value) in self.0.iter().enumerate() {
            if index > 0 {
                text.push(',');
            }
            write!(text, "{value}").expect("writing to a String cannot fail");
        }
        text.push(']');
        text
    }
}

/// Why the embedding model gave no embedding.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingsError {
    /// The service could not be reached, or did not answer in time: it is starting, stopped,
    /// or overloaded. Trying again later may succeed.
    #[error("the embedding service is not answering: {0}")]
    Unreachable(#[source] reqwest::Error),
    /// The service answered with an error.
    #[error("the embedding service refused the request with {status}: {body}")]
    Refused { status: StatusCode, body: String },
    /// The service serves a model other than [`MODEL`], so its embeddings would be recorded as
    /// made by a model that did not make them.
    #[error("the embedding service serves {serving}, not {MODEL}")]
    WrongModel { serving: String },
    /// The service answered with something that is not the embeddings asked for.
    #[error("the embedding service answered {0}")]
    Malformed(String),
}

/// The embedding service's client. Cheap to clone; clones share one HTTP client.
#[derive(Debug, Clone)]
pub struct Embeddings {
    http: reqwest::Client,
    /// The service's base URL, `EMBEDDINGS_URL`: `http://embeddings:8080/` in every stack.
    base_url: Url,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    inputs: &'a [String],
    // Unit length, so cosine distance is the dot product. The service's default, sent anyway,
    // since the index's cosine measure is chosen for it.
    normalize: bool,
    // A text past the model's 2,048 token context is cut there rather than refused, so one
    // long passage never stops the rest.
    truncate: bool,
}

#[derive(Deserialize)]
struct Info {
    served_model_name: String,
}

impl Embeddings {
    pub const fn new(http: reqwest::Client, base_url: Url) -> Self {
        Self { http, base_url }
    }

    /// Checks that the service serves [`MODEL`].
    ///
    /// # Errors
    /// [`EmbeddingsError::WrongModel`] when it serves another, and the errors of a request
    /// (see [`Embeddings::embed_passages`]).
    pub async fn check_model(&self) -> Result<(), EmbeddingsError> {
        let response = self
            .http
            .get(self.url("info"))
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(EmbeddingsError::Unreachable)?;
        let info: Info = read(response).await?;
        if info.served_model_name != MODEL {
            return Err(EmbeddingsError::WrongModel {
                serving: info.served_model_name,
            });
        }
        Ok(())
    }

    /// Embeds `passages` as documents to be found, in order: one embedding each.
    ///
    /// # Errors
    /// [`EmbeddingsError::Unreachable`] when the service does not answer,
    /// [`EmbeddingsError::Refused`] when it answers with an error, and
    /// [`EmbeddingsError::Malformed`] when its answer is not one embedding of [`DIMENSIONS`]
    /// finite numbers for each passage.
    ///
    /// # Panics
    /// Panics on more than [`MAX_BATCH`] passages, which the service would refuse: callers
    /// batch.
    pub async fn embed_passages(
        &self,
        passages: &[impl AsRef<str>],
    ) -> Result<Vec<Embedding>, EmbeddingsError> {
        assert!(
            passages.len() <= MAX_BATCH,
            "at most {MAX_BATCH} passages are embedded at once"
        );
        let inputs: Vec<String> = passages
            .iter()
            .map(|passage| format!("{DOCUMENT_PREFIX}{}", passage.as_ref()))
            .collect();
        self.embed(&inputs).await
    }

    /// Embeds `query` as a search for passages.
    ///
    /// # Errors
    /// As [`Embeddings::embed_passages`].
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "hybrid search embeds its query with it (#94), from the API"
        )
    )]
    pub async fn embed_query(&self, query: &str) -> Result<Embedding, EmbeddingsError> {
        let inputs = [format!("{QUERY_PREFIX}{query}")];
        let mut embeddings = self.embed(&inputs).await?;
        Ok(embeddings.remove(0))
    }

    /// One `/embed` request for `inputs`, each prefixed already, read back as one checked
    /// embedding per input.
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingsError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let response = self
            .http
            .post(self.url("embed"))
            .timeout(REQUEST_TIMEOUT)
            .json(&EmbedRequest {
                inputs,
                normalize: true,
                truncate: true,
            })
            .send()
            .await
            .map_err(EmbeddingsError::Unreachable)?;
        let vectors: Vec<Vec<f32>> = read(response).await?;
        if vectors.len() != inputs.len() {
            return Err(EmbeddingsError::Malformed(format!(
                "{} embeddings for {} texts",
                vectors.len(),
                inputs.len()
            )));
        }
        vectors.into_iter().map(Embedding::new).collect()
    }

    fn url(&self, path: &str) -> Url {
        self.base_url
            .join(path)
            .expect("a fixed path joins any base URL")
    }
}

/// Reads a successful answer as `Shape`, or the error the service answered.
async fn read<Shape: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<Shape, EmbeddingsError> {
    let status = response.status();
    if !status.is_success() {
        let mut body = response.text().await.unwrap_or_default();
        body.truncate(body.floor_char_boundary(ERROR_BODY_LIMIT));
        return Err(EmbeddingsError::Refused { status, body });
    }
    response
        .json()
        .await
        .map_err(|error| EmbeddingsError::Malformed(format!("what it cannot read: {error}")))
}

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;
