// Copyright © 2026 Jalapeno Labs

//! The backfill: the engine loop that brings into the knowledge index what was stored before it
//! was indexed as it is stored.
//!
//! # Why a loop, not a workflow
//!
//! Like the embedder, it has no source of its own: it is one walk over every row of each of its
//! sources, started by the engine rather than by anything a person or a reader did
//! (`docs/engine.md`, Workflows and steps). It runs beside the engine's other loops and never
//! holds up its start.
//!
//! # Its sources
//!
//! It walks each of [`SOURCES`] in turn, each with a row of its own in `knowledge_backfills`:
//!
//! - **Timeline records**, each indexed as storing it does (`timeline_record::index`).
//! - **Turn summaries**, each put on its session's project's timeline as writing it does
//!   (`crate::sessions::timeline::record`), which indexes the record it writes. A summary of a
//!   session without a project is walked past.
//! - **Meetings**, each segmented one's whole transcript indexed as segmenting it does
//!   (`crate::meetings::excerpts::index_transcript`); one never segmented is walked past, and
//!   indexed as it is segmented.
//!
//! # The walk
//!
//! It takes a source's rows in id order (a summary's by its turn), [`BATCH_SIZE`] at a time, and
//! brings each in, in one transaction with the source's row in `knowledge_backfills` that records
//! the batch's last id. So a batch is written and counted together, and an engine that stops part
//! way resumes after the last batch it committed. Bringing a row in twice changes nothing, so a
//! batch cut short is simply taken again. A batch shorter than [`BATCH_SIZE`] is the end: every
//! row stored after it was brought in as it was stored.
//!
//! # Its pace
//!
//! It rests [`PAUSE`] between batches, about a hundred rows a second, so a large timeline does not
//! crowd the readers' and the API's writes. The embedder takes the passages it writes newest
//! documents first, so what was just saved is still found by meaning within seconds.
//!
//! # Once per version
//!
//! Each source walks to its own version ([`version`]): timeline records to
//! [`records::INDEXED_AS`], turn summaries to [`timeline::RECORDED_AS`], and meetings to
//! [`excerpts::TRANSCRIPT_INDEXED_AS`]. Once a walk reaches the
//! end the engine never walks that source again, until a release bumps its version because what it
//! writes changed, and then every row of it is walked once more.
//!
//! # Failures
//!
//! A database that is not answering ends the walk where it is, and it is tried again after
//! [`RETRY_INTERVAL`], from the first source not yet walked. The log carries counts, ids, and
//! durations, never a word of a record.

use std::time::{Duration, Instant};

use anyhow::Context;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use super::records;
use crate::database::Pool;
use crate::meetings::excerpts::{self, MeetingText};
use crate::models::knowledge_backfill::{self, BackfillSource, Walk};
use crate::models::{meeting, timeline_record, turn_summary};
use crate::sessions::timeline;

/// Every source the backfill walks, in the order it walks them.
const SOURCES: [BackfillSource; 3] = [
    BackfillSource::TimelineRecord,
    BackfillSource::TurnSummary,
    BackfillSource::Meeting,
];

/// The version `source`'s walk goes to: what its code writes today.
const fn version(source: BackfillSource) -> i32 {
    match source {
        BackfillSource::TimelineRecord => records::INDEXED_AS,
        BackfillSource::TurnSummary => timeline::RECORDED_AS,
        BackfillSource::Meeting => excerpts::TRANSCRIPT_INDEXED_AS,
    }
}

/// How many rows a batch brings in in one transaction: few enough that its locks are held for a
/// moment, many enough that the walk is not mostly round trips.
const BATCH_SIZE: i64 = 100;

/// How long the walk rests between batches. A batch takes a fraction of this, so the walk takes
/// a small, steady share of the database, about 360,000 rows an hour.
const PAUSE: Duration = Duration::from_secs(1);

/// How long the walk waits after a batch failed, for a database that is restarting.
const RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// Brings into the index what was stored before it was indexed on write. Cheap to clone.
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

    /// Walks every source until each is brought in at its [`version`], or until `shutdown`,
    /// then ends.
    pub fn start(self, shutdown: CancellationToken) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let result = tokio::select! {
                    () = shutdown.cancelled() => return,
                    result = self.walk_every_source(&shutdown) => result,
                };
                let Err(error) = result else {
                    return;
                };
                event!(
                    name: "knowledge.backfill.failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "the knowledge index's backfill stopped; trying again",
                );
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = tokio::time::sleep(RETRY_INTERVAL) => {}
                }
            }
        })
    }

    /// Walks each of [`SOURCES`] in turn; one already walked to its version is passed at once.
    /// Ends early, with nothing lost, on `shutdown`.
    async fn walk_every_source(&self, shutdown: &CancellationToken) -> anyhow::Result<()> {
        for source in SOURCES {
            if shutdown.is_cancelled() {
                return Ok(());
            }
            self.walk(source, shutdown).await?;
        }
        Ok(())
    }

    /// Walks `source` from where the last walk to its version stopped to its last row, resting
    /// between batches. Ends early, with nothing lost, on `shutdown`.
    async fn walk(
        &self,
        source: BackfillSource,
        shutdown: &CancellationToken,
    ) -> anyhow::Result<()> {
        let version = version(source);
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection available")?;
        let walk = knowledge_backfill::begin(&mut connection, source, version)
            .await
            .context("cannot read where the backfill stands")?;
        drop(connection);
        let Walk::After(mut through) = walk else {
            return Ok(());
        };

        event!(
            name: "knowledge.backfill.started",
            Level::INFO,
            knowledge.backfill.source = source.as_str(),
            knowledge.backfill.version = version,
            knowledge.backfill.resumed = through.is_some(),
            "bringing a source into the knowledge index",
        );
        let started = Instant::now();
        let mut walked: usize = 0;
        loop {
            let batch = self.bring_in_batch(source, through).await?;
            walked += batch.count;
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
        knowledge_backfill::finish(&mut connection, source)
            .await
            .context("cannot record that the backfill finished")?;
        event!(
            name: "knowledge.backfill.finished",
            Level::INFO,
            knowledge.backfill.source = source.as_str(),
            knowledge.backfill.version = version,
            knowledge.backfill.rows = walked,
            knowledge.backfill.duration_ms =
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "a source is in the knowledge index",
        );
        Ok(())
    }

    /// Brings in the next batch of `source`'s rows after `through`, and records how far it got,
    /// in one transaction.
    async fn bring_in_batch(
        &self,
        source: BackfillSource,
        through: Option<Uuid>,
    ) -> anyhow::Result<Batch> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection available")?;
        let batch = connection
            .transaction(async |connection| {
                let walked = match source {
                    BackfillSource::TimelineRecord => index_records(connection, through).await?,
                    BackfillSource::TurnSummary => record_summaries(connection, through).await?,
                    BackfillSource::Meeting => index_meetings(connection, through).await?,
                };
                let last = walked.last().copied();
                if let Some(last) = last {
                    knowledge_backfill::advance(connection, source, last).await?;
                }
                Ok::<_, diesel::result::Error>(Batch {
                    count: walked.len(),
                    last: last.or(through),
                })
            })
            .await
            .context("cannot bring in a batch")?;
        event!(
            name: "knowledge.backfill.batch",
            Level::DEBUG,
            knowledge.backfill.source = source.as_str(),
            knowledge.backfill.rows = batch.count,
            "brought a batch into the knowledge index",
        );
        Ok(batch)
    }
}

/// Indexes the next batch of timeline records after `through`, answering their ids in order.
async fn index_records(
    connection: &mut AsyncPgConnection,
    through: Option<Uuid>,
) -> diesel::QueryResult<Vec<Uuid>> {
    let batch = timeline_record::after_id(connection, through, BATCH_SIZE).await?;
    for record in &batch {
        timeline_record::index(connection, record).await?;
    }
    Ok(batch.iter().map(|record| record.id).collect())
}

/// Puts the next batch of turn summaries after turn `through` on their sessions' projects'
/// timelines, answering their turns' ids in order.
async fn record_summaries(
    connection: &mut AsyncPgConnection,
    through: Option<Uuid>,
) -> diesel::QueryResult<Vec<Uuid>> {
    let batch = turn_summary::after_turn(connection, through, BATCH_SIZE).await?;
    for (summary, session) in &batch {
        timeline::record(connection, summary, session).await?;
    }
    Ok(batch.iter().map(|(summary, _)| summary.turn_id).collect())
}

/// Indexes the whole transcript of each of the next batch of meetings after `through` that was
/// segmented, answering every meeting's id in order.
async fn index_meetings(
    connection: &mut AsyncPgConnection,
    through: Option<Uuid>,
) -> diesel::QueryResult<Vec<Uuid>> {
    let batch = meeting::after_id(connection, through, BATCH_SIZE).await?;
    for walked in &batch {
        let text = MeetingText::load(connection, walked.id).await?;
        excerpts::index_transcript(connection, walked, &text).await?;
    }
    Ok(batch.iter().map(|walked| walked.id).collect())
}

/// What one batch did.
struct Batch {
    /// How many rows it walked.
    count: usize,
    /// The last row it walked, or where the walk already was when it found none.
    last: Option<Uuid>,
}

#[cfg(test)]
mod tests;
