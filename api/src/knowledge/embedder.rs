// Copyright © 2026 Jalapeno Labs

//! The embedder: the engine loop that gives every passage its embedding as it arrives.
//!
//! # Why a loop, not a workflow step
//!
//! A workflow run is one workflow on one source (`docs/engine.md`, Runs), and embedding has
//! none: it is the same small job for every passage of every source, a learning saved in the
//! browser as much as a backfill of the timeline. A run per document would fill the Runs page
//! with thousands of one-step runs that say nothing. So, like the connectors' readers, it is a
//! loop of the engine's own, and its queue is the index itself: the partial index
//! `knowledge_passages_unembedded` holds exactly the passages that wait.
//!
//! # A pass
//!
//! Once the service answers that it serves [`MODEL`] ([`Embeddings::check_model`]), a pass
//! takes up to [`MAX_BATCH`] waiting passages, newest documents first, embeds them in one
//! request, and records each embedding as made by [`MODEL`], then takes the next batch, until
//! none waits ([`Embedder::drain`]). A passage is recorded only while its text is still what
//! was embedded (`knowledge_document::store_embeddings`): one rewritten since it was taken waits
//! for an embedding of its new text. Nothing is held between batches, so a pass cut short by a
//! shutdown loses at most the batch in flight, which the next engine takes again.
//!
//! Between passes it looks every [`IDLE_INTERVAL`], an index scan of an empty partial index
//! when nothing waits. A passage saved now is therefore found by meaning within seconds, and by
//! its words at once.
//!
//! # Another model
//!
//! When the engine starts, every embedding made by a model other than [`MODEL`] is forgotten
//! (`knowledge_document::forget_embeddings_not_made_by`), so a release that changes the model
//! re-embeds every passage through the same queue. Vectors of two models are not comparable,
//! so until then those passages are found by their words alone. A service serving another model
//! than the engine expects, such as a deployment whose images drifted apart, embeds nothing:
//! each pass logs it at error level and records nothing.
//!
//! # Failures
//!
//! A service that is starting, stopped, or refusing, and a database that is not answering, end
//! the pass; the next is tried after [`RETRY_INTERVAL`]. The service going away and coming back
//! is logged once each way, not every retry. The log carries counts and durations, never a
//! word of a passage.

use std::time::{Duration, Instant};

use anyhow::Context;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use crate::database::Pool;
use crate::embeddings::{Embeddings, EmbeddingsError, MAX_BATCH, MODEL};
use crate::models::knowledge_document;

/// How often the embedder looks for passages while none waits. Short, since a look costs an
/// index scan of an empty partial index, and what was just saved should be found by meaning
/// soon after it is found by its words.
const IDLE_INTERVAL: Duration = Duration::from_secs(10);

/// How long the embedder waits after a pass that failed: long enough not to hammer a service
/// that is warming up (about 15 seconds on 4 CPUs) or a database that is restarting.
const RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// Why a pass stopped short.
#[derive(Debug, thiserror::Error)]
pub enum EmbedderError {
    #[error(transparent)]
    Embeddings(#[from] EmbeddingsError),
    #[error("{0:#}")]
    Database(#[from] anyhow::Error),
}

/// Embeds the knowledge index's passages. Cheap to clone.
#[derive(Clone)]
pub struct Embedder {
    database: Pool,
    embeddings: Embeddings,
}

impl std::fmt::Debug for Embedder {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Embedder")
            .field("embeddings", &self.embeddings)
            .finish_non_exhaustive()
    }
}

impl Embedder {
    pub const fn new(database: Pool, embeddings: Embeddings) -> Self {
        Self {
            database,
            embeddings,
        }
    }

    /// Runs the embedder until `shutdown`: forgets what another model embedded, then embeds
    /// every passage that waits, pass after pass.
    pub fn start(self, shutdown: CancellationToken) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let wait = match self.forget_other_models().await {
                    Ok(()) => break,
                    Err(error) => {
                        event!(
                            name: "knowledge.embeddings.forget_failed",
                            Level::WARN,
                            error.message = %format!("{error:#}"),
                            "cannot forget what other models embedded; trying again",
                        );
                        RETRY_INTERVAL
                    }
                };
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = tokio::time::sleep(wait) => {}
                }
            }

            let mut failing = false;
            loop {
                let result = tokio::select! {
                    () = shutdown.cancelled() => return,
                    result = self.drain() => result,
                };
                let wait = match result {
                    Ok(_) => {
                        if failing {
                            event!(
                                name: "knowledge.embeddings.resumed",
                                Level::INFO,
                                "passages are being embedded again",
                            );
                        }
                        failing = false;
                        IDLE_INTERVAL
                    }
                    Err(error) => {
                        report(&error, failing);
                        failing = true;
                        RETRY_INTERVAL
                    }
                };
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = tokio::time::sleep(wait) => {}
                }
            }
        })
    }

    /// Forgets every embedding [`MODEL`] did not make, so those passages wait for it again.
    async fn forget_other_models(&self) -> anyhow::Result<()> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection available")?;
        let forgotten =
            knowledge_document::forget_embeddings_not_made_by(&mut connection, MODEL).await?;
        if forgotten > 0 {
            event!(
                name: "knowledge.embeddings.model_changed",
                Level::INFO,
                embeddings.model = MODEL,
                embeddings.forgotten = forgotten,
                "{{embeddings.forgotten}} passages embedded by another model wait for \
                 {{embeddings.model}}",
            );
        }
        Ok(())
    }

    /// Embeds batch after batch until no passage waits, and answers how many were recorded.
    ///
    /// # Errors
    /// The first failure ends the pass: the service not answering, refusing, or serving another
    /// model than [`MODEL`], or the database not answering. What earlier batches recorded
    /// stays recorded.
    pub async fn drain(&self) -> Result<usize, EmbedderError> {
        let started = Instant::now();
        let mut checked = false;
        let mut stored = 0;
        // As many as one request to the service carries.
        let batch_size = i64::try_from(MAX_BATCH).expect("a batch is a few dozen passages");
        loop {
            let mut connection = self
                .database
                .get()
                .await
                .context("no database connection available")?;
            let batch = knowledge_document::unembedded(&mut connection, batch_size)
                .await
                .context("cannot read the passages waiting to be embedded")?;
            if batch.is_empty() {
                break;
            }
            // A connection is not held while the model works: a batch takes seconds.
            drop(connection);

            // Once a pass, and only when there is work, so an idle look asks the service
            // nothing.
            if !checked {
                self.embeddings.check_model().await?;
                checked = true;
            }
            let contents: Vec<&str> = batch
                .iter()
                .map(|passage| passage.content.as_str())
                .collect();
            let embedded = self.embeddings.embed_passages(&contents).await?;

            let mut connection = self
                .database
                .get()
                .await
                .context("no database connection available")?;
            let pairs: Vec<_> = batch.into_iter().zip(embedded).collect();
            stored += knowledge_document::store_embeddings(&mut connection, MODEL, &pairs)
                .await
                .context("cannot record the passages' embeddings")?;
        }
        if stored > 0 {
            event!(
                name: "knowledge.embeddings.stored",
                Level::INFO,
                embeddings.stored = stored,
                embeddings.duration_ms = u64::try_from(started.elapsed().as_millis())
                    .unwrap_or(u64::MAX),
                "embedded {{embeddings.stored}} passages in {{embeddings.duration_ms}} ms",
            );
        }
        Ok(stored)
    }
}

/// Logs a pass that failed. A service that is not there is a warning once, when it goes away,
/// and quiet while it stays away; a service serving the wrong model, or anything else, is
/// logged every time, since only a person can mend it.
fn report(error: &EmbedderError, already_failing: bool) {
    match error {
        EmbedderError::Embeddings(EmbeddingsError::Unreachable(_)) if already_failing => {
            event!(
                name: "knowledge.embeddings.still_unavailable",
                Level::DEBUG,
                error.message = %error,
                "the embedding service is still not answering",
            );
        }
        EmbedderError::Embeddings(EmbeddingsError::WrongModel { .. }) => {
            event!(
                name: "knowledge.embeddings.wrong_model",
                Level::ERROR,
                error.message = %error,
                "nothing is embedded until the embedding service serves the engine's model",
            );
        }
        _ => {
            event!(
                name: "knowledge.embeddings.pass_failed",
                Level::WARN,
                error.message = %error,
                "passages could not be embedded; trying again shortly",
            );
        }
    }
}

#[cfg(test)]
mod tests;
