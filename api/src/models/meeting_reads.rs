// Copyright © 2026 Jalapeno Labs

//! What agents read of meetings, and who reads what they wrote from them. See
//! `docs/knowledge.md`, Who sees what, What agents write from a meeting.
//!
//! # The rule
//!
//! An agent that reads a meeting can say what it read in whatever it writes next, so what it
//! writes is read only by whoever may read every meeting it read, by the meeting's own rule
//! (`models::meeting::readable_by`). Every meeting counts whatever its visibility when it was read,
//! and the rule is asked on every request: making a meeting public opens what was written from it,
//! and making it private again closes it, with nothing written again. What names no meeting is every
//! member's.
//!
//! # What names its meetings
//!
//! - A timeline record names the meetings its words come from (`timeline_record_meetings`): an
//!   excerpt its own meeting, and an agent's record every meeting its writer had read
//!   ([`name_on_record`]).
//! - An agent's turn keeps the meetings that reached it ([`keep`]): a Project Manager's
//!   (`project_manager_turn_meetings`), Elysia's (`elysia_turn_meetings`), and a coding or Studio
//!   session's (`session_turn_meetings`), which also carries what its session's earlier turns read
//!   ([`carry_session`]), since each turn resumes the one before.
//!
//! # Where it is asked
//!
//! Inside each query that reads for someone, never as a list worked out before it:
//! [`readable_records`], [`readable_manager_turns`], [`readable_elysia_turns`], and
//! [`readable_summaries`] in Diesel, and [`readable_record_sql`] in the knowledge index's SQL, which
//! `tests::its_sql_reads_records_as_its_query_does` holds to the same answers.

use std::collections::{HashMap, HashSet};

use diesel::dsl::not;
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::sql_types::Bool;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use tracing::{Level, event};
use uuid::Uuid;

use crate::database::schema::{
    elysia_turn_meetings, elysia_turns, knowledge_documents, project_manager_turn_meetings,
    project_manager_turns, session_turn_meetings, timeline_record_meetings, timeline_records,
    turn_summaries,
};
use crate::meetings::SEGMENT_SOURCE_KIND;
use crate::models::meeting::{self, Reader};
use crate::models::timeline_record::TimelineRecord;
use crate::models::work_batch::WorkBatch;

/// An agent's turn, whose reading of meetings is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// A Project Manager's turn (`project_manager_turns`).
    ProjectManager(Uuid),
    /// One of Elysia's turns (`elysia_turns`).
    Elysia(Uuid),
    /// A coding or Studio session's turn (`session_turns`), whose reading is kept by its session as
    /// its summary is, so the two go together.
    Session { session_id: i64, turn_id: Uuid },
}

/// Keeps that `turn` read `meeting_ids`, besides what it read before. Call it in the transaction
/// that hands the meetings' words over, or before, so nothing reaches an agent unkept.
///
/// # Errors
/// Propagates any database error, including a foreign key violation for a turn or a meeting that
/// does not exist.
pub async fn keep(
    connection: &mut AsyncPgConnection,
    turn: Turn,
    meeting_ids: &[Uuid],
) -> QueryResult<()> {
    if meeting_ids.is_empty() {
        return Ok(());
    }
    // One insert per table: each kind of turn keeps its reading beside its own rows, which
    // cascade with them.
    match turn {
        Turn::ProjectManager(turn_id) => {
            let rows: Vec<_> = meeting_ids
                .iter()
                .map(|meeting_id| {
                    (
                        project_manager_turn_meetings::turn_id.eq(turn_id),
                        project_manager_turn_meetings::meeting_id.eq(*meeting_id),
                    )
                })
                .collect();
            diesel::insert_into(project_manager_turn_meetings::table)
                .values(rows)
                .on_conflict_do_nothing()
                .execute(connection)
                .await?;
        }
        Turn::Elysia(turn_id) => {
            let rows: Vec<_> = meeting_ids
                .iter()
                .map(|meeting_id| {
                    (
                        elysia_turn_meetings::turn_id.eq(turn_id),
                        elysia_turn_meetings::meeting_id.eq(*meeting_id),
                    )
                })
                .collect();
            diesel::insert_into(elysia_turn_meetings::table)
                .values(rows)
                .on_conflict_do_nothing()
                .execute(connection)
                .await?;
        }
        Turn::Session {
            session_id,
            turn_id,
        } => {
            let rows: Vec<_> = meeting_ids
                .iter()
                .map(|meeting_id| {
                    (
                        session_turn_meetings::session_id.eq(session_id),
                        session_turn_meetings::turn_id.eq(turn_id),
                        session_turn_meetings::meeting_id.eq(*meeting_id),
                    )
                })
                .collect();
            diesel::insert_into(session_turn_meetings::table)
                .values(rows)
                .on_conflict_do_nothing()
                .execute(connection)
                .await?;
        }
    }
    Ok(())
}

/// Every meeting `turn` has read so far, in no particular order.
///
/// # Errors
/// Propagates any database error.
pub async fn of_turn(connection: &mut AsyncPgConnection, turn: Turn) -> QueryResult<Vec<Uuid>> {
    match turn {
        Turn::ProjectManager(turn_id) => {
            project_manager_turn_meetings::table
                .filter(project_manager_turn_meetings::turn_id.eq(turn_id))
                .select(project_manager_turn_meetings::meeting_id)
                .load(connection)
                .await
        }
        Turn::Elysia(turn_id) => {
            elysia_turn_meetings::table
                .filter(elysia_turn_meetings::turn_id.eq(turn_id))
                .select(elysia_turn_meetings::meeting_id)
                .load(connection)
                .await
        }
        Turn::Session { turn_id, .. } => {
            session_turn_meetings::table
                .filter(session_turn_meetings::turn_id.eq(turn_id))
                .select(session_turn_meetings::meeting_id)
                .load(connection)
                .await
        }
    }
}

/// Gives session turn `turn_id` every meeting its session's turns read so far: each turn resumes
/// the one before, so it carries what they read, and a session is one person's one piece of work.
/// Call it as the turn starts, before anything it writes. A session runs one turn at a time, so
/// all it read so far came before this turn; reading it by the session rather than by its turns'
/// rows means a turn's row being gone never lets the next one start unheld.
///
/// # Errors
/// Propagates any database error.
pub async fn carry_session(connection: &mut AsyncPgConnection, turn_id: Uuid) -> QueryResult<()> {
    diesel::sql_query(
        "INSERT INTO session_turn_meetings (session_id, turn_id, meeting_id)
         SELECT DISTINCT this.session_id, this.id, session_read.meeting_id
         FROM session_turns this
         JOIN session_turn_meetings session_read ON session_read.session_id = this.session_id
         WHERE this.id = $1
         ON CONFLICT DO NOTHING",
    )
    .bind::<diesel::sql_types::Uuid, _>(turn_id)
    .execute(connection)
    .await
    .map(drop)
}

/// Names `meeting_ids` on record `record_id`, besides what it names already: the meetings its
/// words come from. Call it in the transaction that stores the record.
///
/// # Errors
/// Propagates any database error.
pub async fn name_on_record(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    meeting_ids: &[Uuid],
) -> QueryResult<()> {
    if meeting_ids.is_empty() {
        return Ok(());
    }
    let rows: Vec<_> = meeting_ids
        .iter()
        .map(|meeting_id| {
            (
                timeline_record_meetings::timeline_record_id.eq(record_id),
                timeline_record_meetings::meeting_id.eq(*meeting_id),
            )
        })
        .collect();
    diesel::insert_into(timeline_record_meetings::table)
        .values(rows)
        .on_conflict_do_nothing()
        .execute(connection)
        .await
        .map(drop)
}

/// The meetings each of `record_ids` names, read in one query. A record that names none is
/// absent.
///
/// # Errors
/// Propagates any database error.
pub async fn of_records(
    connection: &mut AsyncPgConnection,
    record_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Vec<Uuid>>> {
    let rows: Vec<(Uuid, Uuid)> = timeline_record_meetings::table
        .filter(timeline_record_meetings::timeline_record_id.eq_any(record_ids))
        .select((
            timeline_record_meetings::timeline_record_id,
            timeline_record_meetings::meeting_id,
        ))
        .load(connection)
        .await?;
    let mut named: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (record_id, meeting_id) in rows {
        named.entry(record_id).or_default().push(meeting_id);
    }
    Ok(named)
}

/// The items among `items` an agent may be handed, each with the meetings its record (`record_of`)
/// names: every one but an excerpt that names no meeting, which nobody could be told may read what
/// an agent writes from it, as it is indexed for nobody. Each one left out is logged.
///
/// # Errors
/// Propagates any database error.
pub async fn for_agent<Item>(
    connection: &mut AsyncPgConnection,
    items: Vec<Item>,
    record_of: impl Fn(&Item) -> &TimelineRecord,
) -> QueryResult<Vec<(Item, Vec<Uuid>)>> {
    let ids: Vec<Uuid> = items.iter().map(|item| record_of(item).id).collect();
    let named = of_records(connection, &ids).await?;
    let mut handed = Vec::with_capacity(items.len());
    for item in items {
        let record = record_of(&item);
        // Read, not taken: two items may carry one record, and each names its meetings.
        let meetings = named.get(&record.id).cloned().unwrap_or_default();
        if record.source_kind == SEGMENT_SOURCE_KIND && meetings.is_empty() {
            event!(
                name: "meeting_reads.excerpt.unplaced",
                Level::WARN,
                record.id = %record.id,
                "an excerpt names no meeting, so no agent is handed it",
            );
            continue;
        }
        handed.push((item, meetings));
    }
    Ok(handed)
}

/// The meetings the knowledge documents `document_ids` come from: each one's own meeting
/// (`access_meeting_id`) and those its record names, once each, in no particular order.
///
/// # Errors
/// Propagates any database error.
pub async fn of_documents(
    connection: &mut AsyncPgConnection,
    document_ids: &[Uuid],
) -> QueryResult<Vec<Uuid>> {
    if document_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut meetings: Vec<Uuid> = knowledge_documents::table
        .filter(knowledge_documents::id.eq_any(document_ids))
        .filter(knowledge_documents::access_meeting_id.is_not_null())
        .select(knowledge_documents::access_meeting_id.assume_not_null())
        .load(connection)
        .await?;
    let named: Vec<Uuid> = knowledge_documents::table
        .inner_join(
            timeline_record_meetings::table.on(timeline_record_meetings::timeline_record_id
                .nullable()
                .eq(knowledge_documents::timeline_record_id)),
        )
        .filter(knowledge_documents::id.eq_any(document_ids))
        .select(timeline_record_meetings::meeting_id)
        .load(connection)
        .await?;
    meetings.extend(named);
    meetings.sort_unstable();
    meetings.dedup();
    Ok(meetings)
}

/// Which of records `record_ids` `reader` may read: each naming no meeting they may not read, read
/// in two queries however many there are. For a list already read, such as a page's promises.
///
/// # Errors
/// Propagates any database error.
pub async fn readable_among(
    connection: &mut AsyncPgConnection,
    record_ids: &[Uuid],
    reader: Reader,
) -> QueryResult<HashSet<Uuid>> {
    let named = of_records(connection, record_ids).await?;
    let meeting_ids: Vec<Uuid> = named.values().flatten().copied().collect();
    let readable = meeting::readable_among(connection, &meeting_ids, reader).await?;
    let reads = |record_id: &Uuid| {
        named.get(record_id).is_none_or(|meetings| {
            meetings
                .iter()
                .all(|meeting_id| readable.contains(meeting_id))
        })
    };
    Ok(record_ids.iter().copied().filter(reads).collect())
}

/// Whether a record may be read by `reader`: it names no meeting `reader` may not read.
pub fn readable_records(
    reader: Reader,
) -> Box<dyn BoxableExpression<timeline_records::table, Pg, SqlType = Bool>> {
    let held_back = timeline_record_meetings::table
        .filter(not(
            timeline_record_meetings::meeting_id.eq_any(meeting::readable_ids(reader))
        ))
        .select(timeline_record_meetings::timeline_record_id);
    Box::new(timeline_records::id.ne_all(held_back))
}

/// Whether a Project Manager's turn may be read by `reader`: it read no meeting `reader` may not
/// read.
pub fn readable_manager_turns(
    reader: Reader,
) -> Box<dyn BoxableExpression<project_manager_turns::table, Pg, SqlType = Bool>> {
    let held_back = project_manager_turn_meetings::table
        .filter(not(
            project_manager_turn_meetings::meeting_id.eq_any(meeting::readable_ids(reader))
        ))
        .select(project_manager_turn_meetings::turn_id);
    Box::new(project_manager_turns::id.ne_all(held_back))
}

/// Whether one of Elysia's turns may be read by `reader`: it read no meeting `reader` may not read.
pub fn readable_elysia_turns(
    reader: Reader,
) -> Box<dyn BoxableExpression<elysia_turns::table, Pg, SqlType = Bool>> {
    let held_back = elysia_turn_meetings::table
        .filter(not(
            elysia_turn_meetings::meeting_id.eq_any(meeting::readable_ids(reader))
        ))
        .select(elysia_turn_meetings::turn_id);
    Box::new(elysia_turns::id.ne_all(held_back))
}

/// The meetings each of `batches` was written from: those the Project Manager's turn that
/// submitted it read, whose words its title and goal, and its worker's result after them, can
/// carry. A batch a person made by accepting a finding names no turn and is absent.
///
/// # Errors
/// Propagates any database error.
pub async fn of_batches(
    connection: &mut AsyncPgConnection,
    batches: &[&WorkBatch],
) -> QueryResult<HashMap<Uuid, Vec<Uuid>>> {
    let turn_ids: Vec<Uuid> = batches.iter().filter_map(|batch| batch.turn_id).collect();
    if turn_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, Uuid)> = project_manager_turn_meetings::table
        .filter(project_manager_turn_meetings::turn_id.eq_any(&turn_ids))
        .select((
            project_manager_turn_meetings::turn_id,
            project_manager_turn_meetings::meeting_id,
        ))
        .load(connection)
        .await?;
    let mut by_turn: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (turn_id, meeting_id) in rows {
        by_turn.entry(turn_id).or_default().push(meeting_id);
    }
    let mut by_batch = HashMap::new();
    for batch in batches {
        if let Some(meetings) = batch.turn_id.and_then(|turn_id| by_turn.get(&turn_id)) {
            by_batch.insert(batch.id, meetings.clone());
        }
    }
    Ok(by_batch)
}

/// Which of `batches` `reader` may read: each written from no meeting they may not read
/// ([`of_batches`]), read in a few queries however many there are.
///
/// # Errors
/// Propagates any database error.
pub async fn readable_batches(
    connection: &mut AsyncPgConnection,
    batches: &[&WorkBatch],
    reader: Reader,
) -> QueryResult<HashSet<Uuid>> {
    let named = of_batches(connection, batches).await?;
    let meeting_ids: Vec<Uuid> = named.values().flatten().copied().collect();
    let readable = meeting::readable_among(connection, &meeting_ids, reader).await?;
    let reads = |batch_id: &Uuid| {
        named.get(batch_id).is_none_or(|meetings| {
            meetings
                .iter()
                .all(|meeting_id| readable.contains(meeting_id))
        })
    };
    Ok(batches
        .iter()
        .map(|batch| batch.id)
        .filter(|batch_id| reads(batch_id))
        .collect())
}

/// The turns of session `session_id` that read a meeting `reader` may not read, whose events and
/// summaries are held back from them, read in one query.
///
/// # Errors
/// Propagates any database error.
pub async fn held_back_session_turns(
    connection: &mut AsyncPgConnection,
    session_id: i64,
    reader: Reader,
) -> QueryResult<HashSet<Uuid>> {
    let held_back: Vec<Uuid> = session_turn_meetings::table
        .filter(session_turn_meetings::session_id.eq(session_id))
        .filter(not(
            session_turn_meetings::meeting_id.eq_any(meeting::readable_ids(reader))
        ))
        .select(session_turn_meetings::turn_id)
        .distinct()
        .load(connection)
        .await?;
    Ok(held_back.into_iter().collect())
}

/// Whether a turn summary of session `session_id` may be read by `reader`: its turn read no meeting
/// `reader` may not read. Read by its session, whose index answers it, rather than across every
/// session's reading.
pub fn readable_summaries(
    session_id: i64,
    reader: Reader,
) -> Box<dyn BoxableExpression<turn_summaries::table, Pg, SqlType = Bool>> {
    let held_back = session_turn_meetings::table
        .filter(session_turn_meetings::session_id.eq(session_id))
        .filter(not(
            session_turn_meetings::meeting_id.eq_any(meeting::readable_ids(reader))
        ))
        .select(session_turn_meetings::turn_id);
    Box::new(turn_summaries::turn_id.ne_all(held_back))
}

/// [`readable_records`] in SQL, for the knowledge index's queries: whether the record whose id
/// `$record` names (a column, `NULL` for what is no record) names no meeting the reader whose id is
/// bound at `$reader` and whose reach is bound at `$reach` may not read
/// (`models::meeting::readable_meeting_sql`). The two state one rule, and
/// `tests::its_sql_reads_records_as_its_query_does` holds them to the same answers.
macro_rules! readable_record_sql {
    ($record:literal, $reader:literal, $reach:literal) => {
        concat!(
            "NOT EXISTS (SELECT 1 FROM timeline_record_meetings named WHERE named.timeline_record_id = ",
            $record,
            " AND NOT ",
            $crate::models::meeting::readable_meeting_sql!("named.meeting_id", $reader, $reach),
            ")"
        )
    };
}
pub(crate) use readable_record_sql;

#[cfg(test)]
mod tests;
