// Copyright © 2026 Jalapeno Labs

//! The backfill: the engine loop that indexes the timeline records stored before they were
//! indexed as they are stored.
//!
//! # Why a loop, not a workflow
//!
//! Like the embedder, it has no source of its own: it is one walk over every record, started by
//! the engine rather than by anything a person or a reader did (`docs/engine.md`, Workflows and
//! steps). It runs beside the engine's other loops and never holds up its start.
//!
//! # The walk
//!
//! It takes the records in id order, [`BATCH_SIZE`] at a time, and indexes each as storing it
//! does (`timeline_record::index`), in one transaction with the row in `knowledge_backfills`
//! that records the batch's last id. So a batch is indexed and counted together, and an engine
//! that stops part way resumes after the last batch it committed. Indexing a record twice changes
//! nothing, so a batch cut short is simply taken again. A batch shorter than [`BATCH_SIZE`] is
//! the end: every record stored after it was indexed as it was stored.
//!
//! # Its pace
//!
//! It rests [`PAUSE`] between batches, about a hundred records a second, so a large timeline
//! does not crowd the readers' and the API's writes. The embedder takes the passages it writes
//! newest documents first, so what was just saved is still found by meaning within seconds.
//!
//! # Once per version
//!
//! It walks to [`records::INDEXED_AS`]. Once a walk reaches the end the engine never walks
//! again, until a release bumps the version because a record's text changed, and then every
//! record is walked once more.
//!
//! # Failures
//!
//! A database that is not answering ends the walk where it is, and it is tried again after
//! [`RETRY_INTERVAL`]. The log carries counts, ids, and durations, never a word of a record.

use std::time::{Duration, Instant};

use anyhow::Context;
use diesel_async::AsyncConnection;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use super::records;
use crate::database::Pool;
use crate::models::knowledge_backfill::{self, BackfillSource, Walk};
use crate::models::timeline_record;

/// How many records a batch indexes in one transaction: few enough that its locks are held for
/// a moment, many enough that the walk is not mostly round trips.
const BATCH_SIZE: i64 = 100;

/// How long the walk rests between batches. A batch takes a fraction of this, so the walk takes
/// a small, steady share of the database, about 360,000 records an hour.
const PAUSE: Duration = Duration::from_secs(1);

/// How long the walk waits after a batch failed, for a database that is restarting.
const RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// Indexes the timeline records stored before they were indexed on write. Cheap to clone.
#[derive(Clone)]
pub struct Backfill {
    database: Pool,
}

impl std::fmt::Debug for Backfill {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Backfill").finish_non_exhaustive()
    }
}

impl Backfill {
    pub const fn new(database: Pool) -> Self {
        Self { database }
    }

    /// Walks the timeline until every record is indexed at [`records::INDEXED_AS`], or until
    /// `shutdown`, then ends.
    pub fn start(self, shutdown: CancellationToken) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let result = tokio::select! {
                    () = shutdown.cancelled() => return,
                    result = self.walk(&shutdown) => result,
                };
                let Err(error) = result else {
                    return;
                };
                event!(
                    name: "knowledge.backfill.failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "the timeline's backfill stopped; trying again",
                );
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = tokio::time::sleep(RETRY_INTERVAL) => {}
                }
            }
        })
    }

    /// Walks from where the last walk to this version stopped to the last record, resting
    /// between batches. Ends early, with nothing lost, on `shutdown`.
    async fn walk(&self, shutdown: &CancellationToken) -> anyhow::Result<()> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection available")?;
        let walk = knowledge_backfill::begin(
            &mut connection,
            BackfillSource::TimelineRecord,
            records::INDEXED_AS,
        )
        .await
        .context("cannot read where the backfill stands")?;
        drop(connection);
        let Walk::After(mut through) = walk else {
            return Ok(());
        };

        event!(
            name: "knowledge.backfill.started",
            Level::INFO,
            knowledge.backfill.version = records::INDEXED_AS,
            knowledge.backfill.resumed = through.is_some(),
            "indexing the timeline's records",
        );
        let started = Instant::now();
        let mut indexed: usize = 0;
        loop {
            let batch = self.index_batch(through).await?;
            indexed += batch.count;
            if batch.count < usize::try_from(BATCH_SIZE).expect("a small batch size") {
                break;
            }
            through = batch.last;
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = tokio::time::sleep(PAUSE) => {}
            }
        }

        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection available")?;
        knowledge_backfill::finish(&mut connection, BackfillSource::TimelineRecord)
            .await
            .context("cannot record that the backfill finished")?;
        event!(
            name: "knowledge.backfill.finished",
            Level::INFO,
            knowledge.backfill.version = records::INDEXED_AS,
            knowledge.backfill.records = indexed,
            knowledge.backfill.duration_ms =
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "every timeline record is indexed",
        );
        Ok(())
    }

    /// Indexes the next batch of records after `through`, and records how far it got, in one
    /// transaction.
    async fn index_batch(&self, through: Option<Uuid>) -> anyhow::Result<Batch> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection available")?;
        let batch = connection
            .transaction(async |connection| {
                let batch = timeline_record::after_id(connection, through, BATCH_SIZE).await?;
                for record in &batch {
                    timeline_record::index(connection, record).await?;
                }
                let last = batch.last().map(|record| record.id);
                if let Some(last) = last {
                    knowledge_backfill::advance(connection, BackfillSource::TimelineRecord, last)
                        .await?;
                }
                Ok::<_, diesel::result::Error>(Batch {
                    count: batch.len(),
                    last: last.or(through),
                })
            })
            .await
            .context("cannot index a batch of records")?;
        event!(
            name: "knowledge.backfill.batch",
            Level::DEBUG,
            knowledge.backfill.records = batch.count,
            "indexed a batch of the timeline",
        );
        Ok(batch)
    }
}

/// What one batch did.
struct Batch {
    /// How many records it indexed.
    count: usize,
    /// The last record it indexed, or where the walk already was when it found none.
    last: Option<Uuid>,
}

#[cfg(test)]
mod tests;
