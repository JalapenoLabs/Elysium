// Copyright © 2026 Jalapeno Labs

//! How far each backfill of the knowledge index has got (`knowledge_backfills`), so one an
//! engine stopped part way resumes where it left off. See `crate::knowledge::backfill`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::knowledge_backfills;

/// A source whose rows stored before they were indexed on write are walked by a backfill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillSource {
    /// Timeline records, each indexed as its document.
    TimelineRecord,
    /// Turn summaries, each put on its session's project's timeline as a record, which indexes it.
    TurnSummary,
}

impl BackfillSource {
    /// Its row's key in `knowledge_backfills`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TimelineRecord => "timeline_record",
            Self::TurnSummary => "turn_summary",
        }
    }
}

/// Where a backfill stands against the version the code indexes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Walk {
    /// Every row is indexed at that version, or a later one a newer release walked to.
    Finished,
    /// The walk goes on after this row, or from the first when `None`.
    After(Option<Uuid>),
}

/// Where `source`'s backfill to `version` stands, starting the walk over from the first row
/// when the last one walked to another version.
///
/// # Errors
/// Propagates any database error, including a missing row for `source`, which its migration
/// seeds.
pub async fn begin(
    connection: &mut AsyncPgConnection,
    source: BackfillSource,
    version: i32,
) -> QueryResult<Walk> {
    let (walked_version, through_id, finished_at) = knowledge_backfills::table
        .find(source.as_str())
        .select((
            knowledge_backfills::version,
            knowledge_backfills::through_id,
            knowledge_backfills::finished_at,
        ))
        .first::<(Option<i32>, Option<Uuid>, Option<DateTime<Utc>>)>(connection)
        .await?;

    // A release rolled back to an older version finds the newer walk's text in place, and
    // leaves it: walking down would only re-cut what the next upgrade cuts again.
    let finished = finished_at.is_some();
    match walked_version {
        Some(walked) if finished && walked >= version => return Ok(Walk::Finished),
        Some(walked) if !finished && walked == version => return Ok(Walk::After(through_id)),
        _ => {}
    }
    diesel::update(knowledge_backfills::table.find(source.as_str()))
        .set((
            knowledge_backfills::version.eq(version),
            knowledge_backfills::through_id.eq(None::<Uuid>),
            knowledge_backfills::finished_at.eq(None::<DateTime<Utc>>),
        ))
        .execute(connection)
        .await?;
    Ok(Walk::After(None))
}

/// Records that `source`'s walk indexed every row up to `through_id`.
///
/// # Errors
/// Propagates any database error.
pub async fn advance(
    connection: &mut AsyncPgConnection,
    source: BackfillSource,
    through_id: Uuid,
) -> QueryResult<()> {
    diesel::update(knowledge_backfills::table.find(source.as_str()))
        .set(knowledge_backfills::through_id.eq(through_id))
        .execute(connection)
        .await
        .map(drop)
}

/// Records that `source`'s walk reached its last row.
///
/// # Errors
/// Propagates any database error.
pub async fn finish(connection: &mut AsyncPgConnection, source: BackfillSource) -> QueryResult<()> {
    diesel::update(knowledge_backfills::table.find(source.as_str()))
        .set(knowledge_backfills::finished_at.eq(Utc::now()))
        .execute(connection)
        .await
        .map(drop)
}
