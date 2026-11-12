// Copyright © 2026 Jalapeno Labs

//! The timeline: everything Elysium ingests, as one kind of record, attributed to projects.
//! See `docs/connectors.md`.
//!
//! A record belongs to whoever it was ingested for, and only they list it: a person's mail is
//! theirs even when it is about a shared project.
//!
//! What Elysium's own agents put on a project's timeline is nobody's in particular: it belongs to
//! the workspace, and everyone who reads a timeline reads it ([`SHARED_OWNERS`]). That is a
//! Project Manager's turns and decisions (`crate::project_managers::timeline`), owned by
//! [`PROJECT_MANAGER_USER_ID`], and every coding session's turn summaries
//! (`crate::sessions::timeline`), owned by [`CODING_AGENT_USER_ID`], since every session is
//! listed to everyone. Only a record's own owner changes what it is attributed to, so nobody
//! moves those.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::dsl::jsonb_set;
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::sql_types::{Array, Jsonb, Nullable, Text};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::{meeting_marks, timeline_record_projects, timeline_records};
use crate::knowledge::records;
use crate::meetings::SEGMENT_SOURCE_KIND;
use crate::models::knowledge_document::{self, KnowledgeSource};
use crate::models::meeting_comment;
use crate::models::meeting_mark::MarkKind;
use crate::models::user::{CODING_AGENT_USER_ID, PROJECT_MANAGER_USER_ID};

/// The owners whose records every reader of a timeline reads besides their own: Elysium's own
/// agents, whose records are about a project rather than any one person's feed.
pub const SHARED_OWNERS: [Uuid; 2] = [PROJECT_MANAGER_USER_ID, CODING_AGENT_USER_ID];

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = timeline_records, check_for_backend(diesel::pg::Pg))]
pub struct TimelineRecord {
    pub id: Uuid,
    pub source_kind: String,
    pub connector_id: Option<Uuid>,
    pub external_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub actor: String,
    pub title: String,
    pub body: String,
    pub url: Option<String>,
    pub media: Option<Value>,
    pub metadata: Value,
    pub action_item_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    /// The excerpt of a meeting this record is (`models::meeting_mark`, kind `excerpt`), whose
    /// words it reads; `None` for every other record, and for a meeting's topic recorded before
    /// excerpts that no word of its meeting could anchor.
    pub excerpt_mark_id: Option<Uuid>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = timeline_records)]
pub struct NewTimelineRecord {
    pub id: Uuid,
    pub source_kind: String,
    pub connector_id: Option<Uuid>,
    pub external_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub actor: String,
    pub title: String,
    pub body: String,
    pub url: Option<String>,
    pub media: Option<Value>,
    pub metadata: Value,
    pub created_by: Uuid,
}

/// A turn summary's record: the record, and the summary it is (`turn_summaries`), which keeps it
/// one record per turn and deletes it with the summary.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = timeline_records)]
struct NewTurnSummaryRecord {
    #[diesel(embed)]
    record: NewTimelineRecord,
    turn_summary_session_id: i64,
    turn_summary_turn_id: Uuid,
}

/// An excerpt's record: the record, and the mark it is, which keeps it one record per excerpt.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = timeline_records)]
struct NewExcerptRecord {
    #[diesel(embed)]
    record: NewTimelineRecord,
    excerpt_mark_id: Uuid,
}

/// What an excerpt's record reads of its words, which an edit, a black-out, or lining the
/// transcript up again changes in place (`crate::meetings::excerpts`).
#[derive(Debug, Clone, PartialEq, AsChangeset)]
#[diesel(table_name = timeline_records)]
pub struct ExcerptText {
    pub occurred_at: DateTime<Utc>,
    pub actor: String,
    pub body: String,
    pub media: Option<Value>,
}

/// Which projects a record is about, with how sure the attribution is.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = timeline_record_projects, check_for_backend(diesel::pg::Pg))]
pub struct Attribution {
    pub timeline_record_id: Uuid,
    pub project_id: Uuid,
    pub confidence: f32,
    pub attributed_by: String,
    pub created_at: DateTime<Utc>,
}

/// Stores `record` unless its account already recorded the same external id, and returns it
/// when it is new. Reading the same message twice therefore records it once.
///
/// A new record enters the knowledge index ([`index`]) in the same transaction, so every
/// record, whoever stored it, is searchable the moment it commits, and a record that fails to
/// store leaves nothing indexed.
///
/// # Errors
/// Propagates any database error.
pub async fn insert_new(
    connection: &mut AsyncPgConnection,
    record: NewTimelineRecord,
) -> QueryResult<Option<TimelineRecord>> {
    // A savepoint when the caller's transaction is already open, as every caller's is.
    connection
        .transaction(async |connection| {
            let stored = diesel::insert_into(timeline_records::table)
                .values(record)
                .on_conflict((
                    timeline_records::connector_id,
                    timeline_records::external_id,
                ))
                .do_nothing()
                .returning(TimelineRecord::as_returning())
                .get_result(connection)
                .await
                .optional()?;
            if let Some(stored) = &stored {
                index(connection, stored).await?;
            }
            Ok(stored)
        })
        .await
}

/// Stores `record` as the record of excerpt `mark_id`, and indexes it ([`index`]) in the same
/// transaction. The mark is new, so the record is too.
///
/// # Errors
/// Propagates any database error, including a unique violation for a mark that has a record.
pub async fn insert_excerpt(
    connection: &mut AsyncPgConnection,
    record: NewTimelineRecord,
    mark_id: Uuid,
) -> QueryResult<TimelineRecord> {
    connection
        .transaction(async |connection| {
            let stored = diesel::insert_into(timeline_records::table)
                .values(NewExcerptRecord {
                    record,
                    excerpt_mark_id: mark_id,
                })
                .returning(TimelineRecord::as_returning())
                .get_result(connection)
                .await?;
            index(connection, &stored).await?;
            Ok(stored)
        })
        .await
}

/// Gives excerpt record `id` what its words read now, and answers it as it then reads. The caller
/// indexes it again.
///
/// # Errors
/// Propagates any database error.
pub async fn set_excerpt_text(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    text: &ExcerptText,
) -> QueryResult<TimelineRecord> {
    diesel::update(timeline_records::table.find(id))
        .set(text)
        .returning(TimelineRecord::as_returning())
        .get_result(connection)
        .await
}

/// Gives excerpt record `id` its `clip` once it is cut, written onto its media's `clip` alone:
/// cutting takes a while, and a refresh of the excerpt's span landing meanwhile keeps what it
/// wrote. Media is never indexed, so nothing is indexed again.
///
/// # Errors
/// Propagates any database error.
pub async fn set_clip(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    clip: &Value,
) -> QueryResult<()> {
    let media = jsonb_set::<Nullable<Jsonb>, Array<Text>, _, _, _>(
        timeline_records::media,
        vec!["clip"],
        Some(clip.clone()),
    );
    diesel::update(timeline_records::table.find(id))
        .set(timeline_records::media.eq(media))
        .execute(connection)
        .await
        .map(drop)
}

/// Gives record `id` its `title`, and `metadata`, as segmenting reads an excerpt again, and answers
/// it as it then reads. The caller indexes it again.
///
/// # Errors
/// Propagates any database error.
pub async fn set_heading(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    title: &str,
    metadata: &Value,
) -> QueryResult<TimelineRecord> {
    diesel::update(timeline_records::table.find(id))
        .set((
            timeline_records::title.eq(title),
            timeline_records::metadata.eq(metadata),
        ))
        .returning(TimelineRecord::as_returning())
        .get_result(connection)
        .await
}

/// Stores `record` as the record of turn `turn_id`'s summary in session `session_id`, or, when
/// that summary already has one, gives it `record`'s text, time, and metadata, keeping its id.
/// Either way it is indexed ([`index`]) in the same transaction, so writing a summary's record
/// again, as the backfill may, leaves one record that reads as the summary does now.
///
/// # Errors
/// Propagates any database error, including a foreign key violation for a summary that does not
/// exist.
pub async fn upsert_turn_summary(
    connection: &mut AsyncPgConnection,
    session_id: i64,
    turn_id: Uuid,
    record: NewTimelineRecord,
) -> QueryResult<TimelineRecord> {
    use diesel::upsert::excluded;

    connection
        .transaction(async |connection| {
            let stored = diesel::insert_into(timeline_records::table)
                .values(NewTurnSummaryRecord {
                    record,
                    turn_summary_session_id: session_id,
                    turn_summary_turn_id: turn_id,
                })
                .on_conflict((
                    timeline_records::turn_summary_session_id,
                    timeline_records::turn_summary_turn_id,
                ))
                .do_update()
                .set((
                    timeline_records::occurred_at.eq(excluded(timeline_records::occurred_at)),
                    timeline_records::actor.eq(excluded(timeline_records::actor)),
                    timeline_records::title.eq(excluded(timeline_records::title)),
                    timeline_records::body.eq(excluded(timeline_records::body)),
                    timeline_records::metadata.eq(excluded(timeline_records::metadata)),
                ))
                .returning(TimelineRecord::as_returning())
                .get_result(connection)
                .await?;
            index(connection, &stored).await?;
            Ok(stored)
        })
        .await
}

/// Puts `record` in the knowledge index as it reads now (`crate::knowledge::records`), or
/// takes it out when it says nothing. Indexing it again changes nothing unless its text did.
///
/// A record that is one version of a page (`records::revision`) is indexed only while it is
/// the newest version its connector recorded, and indexing it takes every older version out,
/// so the index holds what the page says now. Two transactions indexing versions of one page
/// wait for each other, so whichever commits last still leaves the newest one alone indexed.
///
/// A meeting's excerpt is indexed with the comment threads over its span, read as they stand
/// (`models::meeting_comment::indexed_over`), so a thread written before its meeting was
/// segmented is indexed as its excerpts are made.
///
/// Call it inside the transaction that stores or changes the record, or one of its threads.
///
/// # Errors
/// Propagates any database error.
pub async fn index(connection: &mut AsyncPgConnection, record: &TimelineRecord) -> QueryResult<()> {
    let source = KnowledgeSource::TimelineRecord {
        id: record.id,
        kind: record.source_kind.clone(),
    };
    if let Some(revision) = records::revision(record) {
        let newest = keep_newest_version(connection, &revision).await?;
        if newest != Some(record.id) {
            return knowledge_document::remove(connection, &source).await;
        }
    }
    let threads = match segment_span(record) {
        Some((meeting_id, start_ms, end_ms)) => {
            meeting_comment::indexed_over(connection, meeting_id, start_ms, end_ms).await?
        }
        None => Vec::new(),
    };
    let Some(document) = records::document(record, &threads) else {
        return knowledge_document::remove(connection, &source).await;
    };
    knowledge_document::upsert(connection, &document)
        .await
        .map(drop)
}

/// The meeting an excerpt's record is of, and its span on the meeting's clock (`media`'s
/// `meetingId`, `startMs`, and `endMs`); `None` for any other record.
pub(crate) fn segment_span(record: &TimelineRecord) -> Option<(Uuid, i64, i64)> {
    if record.source_kind != SEGMENT_SOURCE_KIND {
        return None;
    }
    let media = record.media.as_ref()?;
    let span = media["meetingId"]
        .as_str()
        .and_then(|meeting_id| meeting_id.parse::<Uuid>().ok())
        .zip(media["startMs"].as_i64())
        .zip(media["endMs"].as_i64())
        .map(|((meeting_id, start_ms), end_ms)| (meeting_id, start_ms, end_ms));
    if span.is_none() {
        tracing::event!(
            name: "knowledge.segment.unplaced",
            tracing::Level::WARN,
            record.id = %record.id,
            "a meeting segment's media names no meeting or span; indexing it without its comments",
        );
    }
    span
}

/// Takes every version of `revision`'s page out of the index but its newest, and answers the
/// newest one's id.
async fn keep_newest_version(
    connection: &mut AsyncPgConnection,
    revision: &records::Revision<'_>,
) -> QueryResult<Option<Uuid>> {
    // Held until the transaction ends, and taken before the versions are read, so a version
    // another transaction records meanwhile is seen once it commits rather than missed.
    let key = format!(
        "knowledge.revisions:{}:{}",
        revision.connector_id, revision.prefix
    );
    diesel::sql_query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind::<diesel::sql_types::Text, _>(key)
        .execute(connection)
        .await?;

    let versions: Vec<(Uuid, String, Option<String>)> = timeline_records::table
        .filter(timeline_records::connector_id.eq(revision.connector_id))
        .filter(
            timeline_records::external_id
                .like(starting_with(revision.prefix))
                .escape('\\'),
        )
        .select((
            timeline_records::id,
            timeline_records::source_kind,
            timeline_records::external_id,
        ))
        .load(connection)
        .await?;
    let newest = versions
        .iter()
        .filter_map(|(id, _, external_id)| {
            let (_, number) = crate::confluence::reader::version_of(external_id.as_deref()?)?;
            Some((number, *id))
        })
        .max()
        .map(|(_, id)| id);
    for (id, kind, _) in versions {
        if Some(id) != newest {
            knowledge_document::remove(connection, &KnowledgeSource::TimelineRecord { id, kind })
                .await?;
        }
    }
    Ok(newest)
}

/// A `LIKE` pattern matching every text that starts with `prefix`, escaped with `\`.
fn starting_with(prefix: &str) -> String {
    let escaped = prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("{escaped}%")
}

/// Up to `limit` records of every owner, in id order, after `after` when it is given: the walk
/// the knowledge index's backfill takes (`crate::knowledge::backfill`).
///
/// # Errors
/// Propagates any database error.
pub async fn after_id(
    connection: &mut AsyncPgConnection,
    after: Option<Uuid>,
    limit: i64,
) -> QueryResult<Vec<TimelineRecord>> {
    let mut query = timeline_records::table
        .order(timeline_records::id.asc())
        .limit(limit)
        .select(TimelineRecord::as_select())
        .into_boxed();
    if let Some(after) = after {
        query = query.filter(timeline_records::id.gt(after));
    }
    query.load(connection).await
}

/// The record `connector_id` keeps under `external_id`, if it has one.
///
/// # Errors
/// Propagates any database error.
pub async fn find_by_external_id(
    connection: &mut AsyncPgConnection,
    connector_id: Uuid,
    external_id: &str,
) -> QueryResult<Option<TimelineRecord>> {
    timeline_records::table
        .filter(timeline_records::connector_id.eq(connector_id))
        .filter(timeline_records::external_id.eq(external_id))
        .select(TimelineRecord::as_select())
        .first(connection)
        .await
        .optional()
}

/// Up to `limit` records of every owner, in the order they were ingested: after position
/// `after` (an ingestion instant and id, from an earlier read), ingested no earlier than
/// `since` and no later than `through`. For a reader of everything ingested, such as the
/// company doc's review, which reads for the whole company (`docs/knowledge.md`, Who sees what).
///
/// # Errors
/// Propagates any database error.
pub async fn ingested_between(
    connection: &mut AsyncPgConnection,
    after: Option<(DateTime<Utc>, Uuid)>,
    since: DateTime<Utc>,
    through: DateTime<Utc>,
    limit: i64,
) -> QueryResult<Vec<TimelineRecord>> {
    let mut query = timeline_records::table
        .filter(timeline_records::created_at.ge(since))
        .filter(timeline_records::created_at.le(through))
        .order((
            timeline_records::created_at.asc(),
            timeline_records::id.asc(),
        ))
        .limit(limit)
        .select(TimelineRecord::as_select())
        .into_boxed();
    if let Some((at, id)) = after {
        query = query.filter(
            timeline_records::created_at
                .gt(at)
                .or(timeline_records::created_at
                    .eq(at)
                    .and(timeline_records::id.gt(id))),
        );
    }
    query.load(connection).await
}

/// The record `id`, when `owner` owns it, without its attributions.
///
/// # Errors
/// Propagates any database error.
pub async fn find_owned_record(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<Option<TimelineRecord>> {
    timeline_records::table
        .filter(timeline_records::id.eq(id))
        .filter(timeline_records::created_by.eq(owner))
        .select(TimelineRecord::as_select())
        .first(connection)
        .await
        .optional()
}

/// Whether any record of `connector_id`'s whose external id starts with `prefix` raised an
/// action item: for a kind that records each version of a thing, whether that thing already
/// raised one, so editing it again does not raise another.
///
/// # Errors
/// Propagates any database error.
pub async fn has_raised_item(
    connection: &mut AsyncPgConnection,
    connector_id: Uuid,
    prefix: &str,
) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        timeline_records::table
            .filter(timeline_records::connector_id.eq(connector_id))
            .filter(
                timeline_records::external_id
                    .like(starting_with(prefix))
                    .escape('\\'),
            )
            .filter(timeline_records::action_item_id.is_not_null()),
    ))
    .get_result(connection)
    .await
}

/// Records the action item a record raised.
///
/// # Errors
/// Propagates any database error.
pub async fn set_action_item(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    action_item_id: Uuid,
) -> QueryResult<TimelineRecord> {
    diesel::update(timeline_records::table.find(id))
        .set(timeline_records::action_item_id.eq(action_item_id))
        .returning(TimelineRecord::as_returning())
        .get_result(connection)
        .await
}

/// What to list from the timeline.
#[derive(Debug, Clone, Copy)]
pub struct TimelineQuery {
    pub owner: Uuid,
    /// Whether the workspace's records ([`SHARED_OWNERS`]) are read too: for the company's
    /// members, never a guest, since they quote what was ingested.
    pub shared: bool,
    /// Only records attributed to this project.
    pub project_id: Option<Uuid>,
    /// Only the record that raised this action item.
    pub action_item_id: Option<Uuid>,
    /// Only this meeting's standing excerpts.
    pub meeting_id: Option<Uuid>,
    /// Only records that happened before this, for the next page.
    pub before: Option<DateTime<Utc>>,
    pub limit: i64,
}

/// `owner`'s records, and the workspace's when `query.shared`, that `query` narrows to, on
/// every page: its project and action item, but not its paging. A page and the total it is
/// counted against share this one filter, so they never disagree about what the timeline holds.
fn matching(query: &TimelineQuery) -> timeline_records::BoxedQuery<'static, Pg> {
    let mut readable = vec![query.owner];
    if query.shared {
        readable.extend(SHARED_OWNERS);
    }
    let mut statement = timeline_records::table
        .filter(timeline_records::created_by.eq_any(readable))
        .into_boxed();
    if let Some(project_id) = query.project_id {
        statement = statement.filter(
            timeline_records::id.eq_any(
                timeline_record_projects::table
                    .filter(timeline_record_projects::project_id.eq(project_id))
                    .select(timeline_record_projects::timeline_record_id),
            ),
        );
    }
    if let Some(action_item_id) = query.action_item_id {
        statement = statement.filter(timeline_records::action_item_id.eq(action_item_id));
    }
    if let Some(meeting_id) = query.meeting_id {
        statement = statement.filter(
            timeline_records::excerpt_mark_id
                .eq_any(standing_excerpts_of(meeting_id).select(meeting_marks::id.nullable())),
        );
    }
    statement
}

/// The records among `ids` that exist, in no particular order.
///
/// # Errors
/// Propagates any database error.
pub async fn find_many(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
) -> QueryResult<Vec<TimelineRecord>> {
    timeline_records::table
        .filter(timeline_records::id.eq_any(ids))
        .select(TimelineRecord::as_select())
        .load(connection)
        .await
}

/// Meeting `meeting_id`'s standing excerpts, the marks of kind `excerpt` no change removed.
fn standing_excerpts_of(meeting_id: Uuid) -> meeting_marks::BoxedQuery<'static, Pg> {
    meeting_marks::table
        .filter(meeting_marks::meeting_id.eq(meeting_id))
        .filter(meeting_marks::kind.eq(MarkKind::Excerpt.as_str()))
        .filter(meeting_marks::removed_by.is_null())
        .into_boxed()
}

/// Meeting `meeting_id`'s standing excerpts' records, each with its attributions, in no
/// particular order. An excerpt's record is its meeting's owner's. There is no cap: a person may
/// make any number beside segmenting's own, and each excerpt covers words of the meeting, so the
/// meeting's length bounds them.
///
/// # Errors
/// Propagates any database error.
pub async fn of_meeting(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Vec<(TimelineRecord, Vec<Attribution>)>> {
    let records: Vec<TimelineRecord> = timeline_records::table
        .filter(
            timeline_records::excerpt_mark_id
                .eq_any(standing_excerpts_of(meeting_id).select(meeting_marks::id.nullable())),
        )
        .select(TimelineRecord::as_select())
        .load(connection)
        .await?;
    let ids: Vec<Uuid> = records.iter().map(|record| record.id).collect();
    let mut by_record: HashMap<Uuid, Vec<Attribution>> = HashMap::new();
    for attribution in attributions_of(connection, &ids).await? {
        by_record
            .entry(attribution.timeline_record_id)
            .or_default()
            .push(attribution);
    }
    Ok(records
        .into_iter()
        .map(|record| {
            let attributions = by_record.remove(&record.id).unwrap_or_default();
            (record, attributions)
        })
        .collect())
}

/// The record of excerpt `mark_id`, standing or removed; `None` for a mark that is no excerpt.
///
/// # Errors
/// Propagates any database error.
pub async fn of_excerpt(
    connection: &mut AsyncPgConnection,
    mark_id: Uuid,
) -> QueryResult<Option<TimelineRecord>> {
    timeline_records::table
        .filter(timeline_records::excerpt_mark_id.eq(mark_id))
        .select(TimelineRecord::as_select())
        .first(connection)
        .await
        .optional()
}

/// The records of excerpts `mark_ids`, standing or removed, in no particular order.
///
/// # Errors
/// Propagates any database error.
pub async fn of_excerpts(
    connection: &mut AsyncPgConnection,
    mark_ids: &[Uuid],
) -> QueryResult<Vec<TimelineRecord>> {
    timeline_records::table
        .filter(timeline_records::excerpt_mark_id.eq_any(mark_ids))
        .select(TimelineRecord::as_select())
        .load(connection)
        .await
}

/// The projects each of `meeting_ids` has an excerpt on: every project one of its standing
/// excerpts' records is attributed to, once each, in the order of their ids. A meeting with no
/// excerpt on a project has no entry. An excerpt's record is its meeting's owner's, so this reads
/// nobody else's.
///
/// # Errors
/// Propagates any database error.
pub async fn projects_of_meetings(
    connection: &mut AsyncPgConnection,
    meeting_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Vec<Uuid>>> {
    let rows: Vec<(Uuid, Uuid)> = meeting_marks::table
        .inner_join(
            timeline_records::table
                .on(timeline_records::excerpt_mark_id.eq(meeting_marks::id.nullable())),
        )
        .inner_join(
            timeline_record_projects::table
                .on(timeline_record_projects::timeline_record_id.eq(timeline_records::id)),
        )
        .filter(meeting_marks::meeting_id.eq_any(meeting_ids))
        .filter(meeting_marks::kind.eq(MarkKind::Excerpt.as_str()))
        .filter(meeting_marks::removed_by.is_null())
        .select((
            meeting_marks::meeting_id,
            timeline_record_projects::project_id,
        ))
        .distinct()
        .order_by(timeline_record_projects::project_id.asc())
        .load(connection)
        .await?;

    let mut by_meeting: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (meeting_id, project_id) in rows {
        by_meeting.entry(meeting_id).or_default().push(project_id);
    }
    Ok(by_meeting)
}

/// How many records `query` matches across every page, ignoring `before` and `limit`.
///
/// # Errors
/// Propagates any database error.
pub async fn count(connection: &mut AsyncPgConnection, query: &TimelineQuery) -> QueryResult<i64> {
    matching(query).count().get_result(connection).await
}

/// `owner`'s records and the workspace's, newest first, with each one's attributions.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    query: TimelineQuery,
) -> QueryResult<Vec<(TimelineRecord, Vec<Attribution>)>> {
    let mut statement = matching(&query)
        .select(TimelineRecord::as_select())
        .order((
            timeline_records::occurred_at.desc(),
            timeline_records::id.desc(),
        ))
        .limit(query.limit);
    if let Some(before) = query.before {
        statement = statement.filter(timeline_records::occurred_at.lt(before));
    }
    let records: Vec<TimelineRecord> = statement.load(connection).await?;

    let ids: Vec<Uuid> = records.iter().map(|record| record.id).collect();
    let mut by_record: HashMap<Uuid, Vec<Attribution>> = HashMap::new();
    for attribution in attributions_of(connection, &ids).await? {
        by_record
            .entry(attribution.timeline_record_id)
            .or_default()
            .push(attribution);
    }

    Ok(records
        .into_iter()
        .map(|record| {
            let attributions = by_record.remove(&record.id).unwrap_or_default();
            (record, attributions)
        })
        .collect())
}

/// Every attribution of the records `ids`, in no particular order.
///
/// # Errors
/// Propagates any database error.
pub async fn attributions_of(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
) -> QueryResult<Vec<Attribution>> {
    timeline_record_projects::table
        .filter(timeline_record_projects::timeline_record_id.eq_any(ids))
        .select(Attribution::as_select())
        .load(connection)
        .await
}

/// The record `id` with its attributions, when `owner` owns it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist or belongs to someone else.
pub async fn find_owned(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<(TimelineRecord, Vec<Attribution>)> {
    let record = timeline_records::table
        .filter(timeline_records::id.eq(id))
        .filter(timeline_records::created_by.eq(owner))
        .select(TimelineRecord::as_select())
        .first(connection)
        .await?;
    let attributions = timeline_record_projects::table
        .filter(timeline_record_projects::timeline_record_id.eq(id))
        .select(Attribution::as_select())
        .load(connection)
        .await?;
    Ok((record, attributions))
}

/// Up to `limit` records attributed to `project_id`, every owner's, newest first and before
/// `before` when it is given, each with its attribution to the project. For the project's
/// Project Manager, which works for the project rather than for one person: the text it reads
/// is the workspace's to search anyway (`docs/knowledge.md`, Who sees what).
///
/// # Errors
/// Propagates any database error.
pub async fn attributed_to(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> QueryResult<Vec<(TimelineRecord, Attribution)>> {
    let mut statement = timeline_records::table
        .inner_join(timeline_record_projects::table)
        .filter(timeline_record_projects::project_id.eq(project_id))
        .select((TimelineRecord::as_select(), Attribution::as_select()))
        .order((
            timeline_records::occurred_at.desc(),
            timeline_records::id.desc(),
        ))
        .limit(limit)
        .into_boxed();
    if let Some(before) = before {
        statement = statement.filter(timeline_records::occurred_at.lt(before));
    }
    statement.load(connection).await
}

/// The records among `ids` attributed to `project_id`, every owner's, each with its attribution
/// to the project, in no particular order.
///
/// # Errors
/// Propagates any database error.
pub async fn attributed_among(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    ids: &[Uuid],
) -> QueryResult<Vec<(TimelineRecord, Attribution)>> {
    timeline_records::table
        .inner_join(timeline_record_projects::table)
        .filter(timeline_record_projects::project_id.eq(project_id))
        .filter(timeline_records::id.eq_any(ids))
        .select((TimelineRecord::as_select(), Attribution::as_select()))
        .load(connection)
        .await
}

/// The record `id` with its attributions, when `reader` may read it: their own, or, when
/// `shared`, the workspace's ([`SHARED_OWNERS`]).
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist or `reader` may not read it.
pub async fn find_readable(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    reader: Uuid,
    shared: bool,
) -> QueryResult<(TimelineRecord, Vec<Attribution>)> {
    let owner = timeline_records::table
        .find(id)
        .select(timeline_records::created_by)
        .first::<Uuid>(connection)
        .await?;
    let is_shared = shared && SHARED_OWNERS.contains(&owner);
    if owner != reader && !is_shared {
        return Err(diesel::result::Error::NotFound);
    }
    find_owned(connection, id, owner).await
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::json;

    use super::*;
    use crate::models::attribution;
    use crate::models::project::{self, NewProject};
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    /// A record of the test person's that happened `minutes` after a fixed moment.
    fn record_at(minutes: i64) -> NewTimelineRecord {
        let base = DateTime::parse_from_rfc3339("2026-11-04T12:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc);
        NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: "gmail".to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: base + TimeDelta::minutes(minutes),
            actor: "Sam".to_owned(),
            title: "Launch plan".to_owned(),
            body: String::new(),
            url: None,
            media: None,
            metadata: json!({}),
            created_by: TEST_PERSON_ID,
        }
    }

    /// The record's document, as `(source, title, created_by)`, and its passages in order.
    async fn indexed(
        connection: &mut AsyncPgConnection,
        record_id: Uuid,
    ) -> Option<((String, String, Uuid), Vec<String>)> {
        use crate::database::schema::{knowledge_documents, knowledge_passages};

        let (document_id, document) = knowledge_documents::table
            .filter(knowledge_documents::timeline_record_id.eq(record_id))
            .select((
                knowledge_documents::id,
                (
                    knowledge_documents::source,
                    knowledge_documents::title,
                    knowledge_documents::created_by,
                ),
            ))
            .first::<(Uuid, (String, String, Uuid))>(connection)
            .await
            .optional()
            .expect("the index is read")?;
        let passages = knowledge_passages::table
            .filter(knowledge_passages::document_id.eq(document_id))
            .order(knowledge_passages::position.asc())
            .select(knowledge_passages::content)
            .load(connection)
            .await
            .expect("the passages are read");
        Some((document, passages))
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_new_record_is_searchable_the_moment_it_is_stored() {
        let (_url, mut connection) = migrated_database().await;
        let mut mail = record_at(0);
        mail.body = "WAY-123 ships Friday.".to_owned();

        let stored = insert_new(&mut connection, mail)
            .await
            .expect("stored")
            .expect("new");

        assert_eq!(
            indexed(&mut connection, stored.id).await,
            Some((
                ("gmail".to_owned(), "Launch plan".to_owned(), TEST_PERSON_ID),
                vec!["Launch plan\n\nFrom Sam\n\nWAY-123 ships Friday.".to_owned()],
            ))
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_record_that_says_nothing_is_stored_but_not_indexed() {
        let (_url, mut connection) = migrated_database().await;
        let mut empty = record_at(0);
        empty.title = String::new();
        empty.actor = String::new();

        let stored = insert_new(&mut connection, empty)
            .await
            .expect("stored")
            .expect("new");
        assert_eq!(indexed(&mut connection, stored.id).await, None);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn counts_every_page_a_project_timeline_holds() {
        let (_url, mut connection) = migrated_database().await;
        let launch = project::create(
            &mut connection,
            &NewProject {
                created_by: TEST_PERSON_ID,
                name: "Launch".to_owned(),
                description: String::new(),
            },
        )
        .await
        .expect("project");

        for minutes in [0, 10, 20] {
            let record = insert_new(&mut connection, record_at(minutes))
                .await
                .expect("stored")
                .expect("new");
            attribution::attribute(
                &mut connection,
                record.id,
                launch.id,
                1.0,
                attribution::Attributor::Elysia,
            )
            .await
            .expect("attributed");
        }
        // About nothing in particular, so only the whole timeline counts it.
        insert_new(&mut connection, record_at(30))
            .await
            .expect("stored")
            .expect("new");

        let first_page = TimelineQuery {
            owner: TEST_PERSON_ID,
            shared: false,
            project_id: Some(launch.id),
            action_item_id: None,
            meeting_id: None,
            before: None,
            limit: 2,
        };
        let page = list(&mut connection, first_page).await.expect("listed");
        assert_eq!(page.len(), 2, "a page holds no more than its limit");
        assert_eq!(
            count(&mut connection, &first_page).await.expect("counted"),
            3
        );

        // An older page counts the same total: paging narrows the page, never the total.
        let older_page = TimelineQuery {
            before: Some(page[1].0.occurred_at),
            ..first_page
        };
        let older = list(&mut connection, older_page).await.expect("listed");
        assert_eq!(older.len(), 1);
        assert_eq!(
            count(&mut connection, &older_page).await.expect("counted"),
            3
        );

        let everything = TimelineQuery {
            project_id: None,
            ..first_page
        };
        assert_eq!(
            count(&mut connection, &everything).await.expect("counted"),
            4
        );
    }
}
