// Copyright © 2026 Jalapeno Labs

//! Marks on a meeting's final transcript: everything anchored to a range of its words. See
//! `docs/meetings.md`, The final transcript.
//!
//! A mark is one row: its kind, the first and last word of its range (`start_word_id`,
//! `end_word_id`), a JSON object shaped by its kind, and who made it. A range runs from one word
//! of the transcript to another, so it is one word or the whole meeting. Every mark is anchored to
//! words that stand: an edit that replaces words moves every mark over them onto the words that
//! replace them (`models::meeting_word`), and its undo moves them back.
//!
//! A removed mark is kept, marked by the change that removed it (`removed_by`), so an undo puts it
//! back with whatever hangs from it. Every change is in the meeting's history
//! (`models::meeting_event`), made in the same transaction, under the meeting's lock.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::database::schema::meeting_marks;
use crate::models::agent::Author;
use crate::models::meeting;
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::meeting_word;

/// How many marks one insert writes. Each mark binds ten parameters, and Postgres takes at most
/// 65,535 in one statement.
const INSERT_CHUNK: usize = 1_000;

/// What a mark is. Stored as text, so a later kind needs no migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    /// How sure the transcript is of its words: a flag of the correcting agent's, or a range that
    /// changed.
    Confidence,
    /// Words an edit wrote, which no capture said. Made with its edit, never on its own.
    Edit,
    /// A range taken into projects.
    Excerpt,
    /// A range that does not matter, left out of search and of what agents read.
    BlackOut,
    /// A thread on a range.
    Comment,
    /// A frame pinned to a range.
    Pin,
    /// A task heard in a range.
    Task,
}

impl MarkKind {
    const ALL: [Self; 7] = [
        Self::Confidence,
        Self::Edit,
        Self::Excerpt,
        Self::BlackOut,
        Self::Comment,
        Self::Pin,
        Self::Task,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confidence => "confidence",
            Self::Edit => "edit",
            Self::Excerpt => "excerpt",
            Self::BlackOut => "black_out",
            Self::Comment => "comment",
            Self::Pin => "pin",
            Self::Task => "task",
        }
    }

    /// The kind stored as `text`, if this build knows it.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// One mark.
#[derive(Debug, Clone, PartialEq, Queryable, Selectable)]
#[diesel(table_name = meeting_marks, check_for_backend(diesel::pg::Pg))]
pub struct Mark {
    pub id: Uuid,
    pub meeting_id: Uuid,
    /// A [`MarkKind`] name, kept as text so marks of a later version still load.
    pub kind: String,
    pub start_word_id: Uuid,
    pub end_word_id: Uuid,
    pub data: Value,
    pub created_by: Uuid,
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
    /// The change that removed it; `None` while it stands.
    pub removed_by: Option<Uuid>,
}

impl Mark {
    /// Its kind, when this build knows it.
    pub fn kind(&self) -> Option<MarkKind> {
        MarkKind::parse(&self.kind)
    }

    /// Its range.
    pub const fn range(&self) -> Range {
        Range {
            start_word_id: self.start_word_id,
            end_word_id: self.end_word_id,
        }
    }
}

/// A range of the final transcript, from its first word to its last, both standing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub start_word_id: Uuid,
    pub end_word_id: Uuid,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_marks)]
pub(crate) struct NewMark {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub kind: String,
    pub start_word_id: Uuid,
    pub end_word_id: Uuid,
    pub data: Value,
    pub created_by: Uuid,
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl NewMark {
    /// A mark of `kind` over `range` of meeting `meeting_id`, made by `author`.
    pub(crate) fn new(
        meeting_id: Uuid,
        kind: MarkKind,
        range: Range,
        data: Value,
        author: Author<'_>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            meeting_id,
            kind: kind.as_str().to_owned(),
            start_word_id: range.start_word_id,
            end_word_id: range.end_word_id,
            data,
            created_by: author.user_id,
            agent: author.agent.map(|agent| agent.id().to_owned()),
            agent_name: author
                .agent
                .and_then(|agent| agent.name().map(ToOwned::to_owned)),
            created_at: author.at,
        }
    }
}

/// Why a mark could not be made or removed.
#[derive(Debug, thiserror::Error)]
pub enum MarkError {
    /// The meeting, the mark, or a word of its range is not the meeting's.
    #[error("not found")]
    NotFound,
    /// A word of its range was replaced; ranges run between standing words.
    #[error("a word of the range was replaced")]
    NotStanding,
    /// Its last word comes before its first.
    #[error("the range ends before it starts")]
    OutOfOrder,
    /// An edit's mark is made and removed with its edit, never on its own.
    #[error("an edit's mark goes with its edit")]
    EditsOwn,
    /// The mark was removed already.
    #[error("the mark was removed already")]
    Removed,
    /// Its data is not a JSON object.
    #[error("a mark's data is a JSON object")]
    NotAnObject,
    /// The meeting was merged into another, which is where it changes now.
    #[error("the meeting was merged into another")]
    Merged,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Meeting `meeting_id`'s standing marks, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn standing(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Vec<Mark>> {
    meeting_marks::table
        .filter(meeting_marks::meeting_id.eq(meeting_id))
        .filter(meeting_marks::removed_by.is_null())
        .order(meeting_marks::id.asc())
        .select(Mark::as_select())
        .load(connection)
        .await
}

/// Mark `id` of meeting `meeting_id`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the meeting has no such mark.
pub async fn find(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    id: Uuid,
) -> QueryResult<Mark> {
    meeting_marks::table
        .find(id)
        .filter(meeting_marks::meeting_id.eq(meeting_id))
        .select(Mark::as_select())
        .first(connection)
        .await
}

/// Makes a mark of `kind` over `range` of meeting `meeting_id` as `author`, and records it.
///
/// # Errors
/// [`MarkError::NotFound`] when the meeting or a word of the range is not the meeting's,
/// [`MarkError::NotStanding`] or [`MarkError::OutOfOrder`] for a range that is not one of the
/// transcript as it reads, [`MarkError::EditsOwn`] for an edit's mark, and
/// [`MarkError::NotAnObject`] for data that is not an object; any database error.
pub async fn add(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    kind: MarkKind,
    range: Range,
    data: Value,
    author: Author<'_>,
) -> Result<(Mark, MeetingEvent), MarkError> {
    if kind == MarkKind::Edit {
        return Err(MarkError::EditsOwn);
    }
    if !data.is_object() {
        return Err(MarkError::NotAnObject);
    }
    connection
        .transaction(async |connection| {
            lock_meeting(connection, meeting_id).await?;
            check_range(connection, meeting_id, range).await?;
            let new = NewMark::new(meeting_id, kind, range, data, author);
            let mark = insert(connection, new).await?;
            let event = record_marked(connection, &mark, author, None).await?;
            Ok((mark, event))
        })
        .await
}

/// Removes mark `id` of meeting `meeting_id` as `author`, and records it.
///
/// # Errors
/// [`MarkError::NotFound`] when the meeting has no such mark, [`MarkError::Removed`] when it was
/// removed already, and [`MarkError::EditsOwn`] for an edit's mark; any database error.
pub async fn remove(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    id: Uuid,
    author: Author<'_>,
) -> Result<MeetingEvent, MarkError> {
    connection
        .transaction(async |connection| {
            lock_meeting(connection, meeting_id).await?;
            let mark = found(find(connection, meeting_id, id).await)?;
            if mark.kind() == Some(MarkKind::Edit) {
                return Err(MarkError::EditsOwn);
            }
            if mark.removed_by.is_some() {
                return Err(MarkError::Removed);
            }
            Ok(record_unmarked(connection, &mark, author, None).await?)
        })
        .await
}

/// Undoes `undone`, a `marked` or `unmarked` entry, as `author`: a mark made is removed, and a
/// mark removed is put back. Answers `None` when the mark moved on since, having been removed or
/// put back by another change, so undoing it would overwrite that. The caller holds the meeting's
/// lock.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn undo(
    connection: &mut AsyncPgConnection,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> QueryResult<Option<MeetingEvent>> {
    let Some(mark_id) = undone
        .data
        .get("markId")
        .and_then(Value::as_str)
        .and_then(|mark_id| mark_id.parse::<Uuid>().ok())
    else {
        // Only a history entry written by hand names no mark.
        tracing::event!(
            name: "meetings.history.unreadable",
            tracing::Level::WARN,
            meeting_event.id = %undone.id,
            "a mark's history entry names no mark; it cannot be undone",
        );
        return Ok(None);
    };
    let mark = find(connection, undone.meeting_id, mark_id).await?;
    match undone.kind() {
        Some(EventKind::Marked) if mark.removed_by.is_none() => Ok(Some(
            record_unmarked(connection, &mark, author, Some(undone.id)).await?,
        )),
        Some(EventKind::Unmarked) if mark.removed_by == Some(undone.id) => {
            diesel::update(meeting_marks::table.find(mark.id))
                .set(meeting_marks::removed_by.eq(None::<Uuid>))
                .execute(connection)
                .await?;
            Ok(Some(
                record_marked(connection, &mark, author, Some(undone.id)).await?,
            ))
        }
        _ => Ok(None),
    }
}

/// Every mark of meeting `meeting_id`, standing or removed, with an end among `word_ids`: the
/// marks a change replacing those words moves.
pub(crate) async fn anchored_in(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    word_ids: &[Uuid],
) -> QueryResult<Vec<Mark>> {
    meeting_marks::table
        .filter(meeting_marks::meeting_id.eq(meeting_id))
        .filter(
            meeting_marks::start_word_id
                .eq_any(word_ids)
                .or(meeting_marks::end_word_id.eq_any(word_ids)),
        )
        .order(meeting_marks::id.asc())
        .select(Mark::as_select())
        .load(connection)
        .await
}

/// Replaces mark `id`'s data with `data`, for a kind whose state changes in place, such as a
/// comment thread being resolved. The caller holds the meeting's lock and records the change.
pub(crate) async fn set_data(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    data: &Value,
) -> QueryResult<Mark> {
    diesel::update(meeting_marks::table.find(id))
        .set(meeting_marks::data.eq(data))
        .returning(Mark::as_returning())
        .get_result(connection)
        .await
}

/// Moves mark `id` onto `range`.
pub(crate) async fn set_range(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    range: Range,
) -> QueryResult<()> {
    diesel::update(meeting_marks::table.find(id))
        .set((
            meeting_marks::start_word_id.eq(range.start_word_id),
            meeting_marks::end_word_id.eq(range.end_word_id),
        ))
        .execute(connection)
        .await
        .map(drop)
}

/// Removes every standing mark among `ids` as change `event_id`, and answers the ones it removed.
pub(crate) async fn remove_all(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
    event_id: Uuid,
) -> QueryResult<Vec<Uuid>> {
    diesel::update(
        meeting_marks::table
            .filter(meeting_marks::id.eq_any(ids))
            .filter(meeting_marks::removed_by.is_null()),
    )
    .set(meeting_marks::removed_by.eq(event_id))
    .returning(meeting_marks::id)
    .get_results(connection)
    .await
}

/// Puts back every mark among `ids` that change `removed_by` removed, and answers the ones it put
/// back.
pub(crate) async fn restore_all(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
    removed_by: Uuid,
) -> QueryResult<Vec<Uuid>> {
    diesel::update(
        meeting_marks::table
            .filter(meeting_marks::id.eq_any(ids))
            .filter(meeting_marks::removed_by.eq(removed_by)),
    )
    .set(meeting_marks::removed_by.eq(None::<Uuid>))
    .returning(meeting_marks::id)
    .get_results(connection)
    .await
}

pub(crate) async fn insert(connection: &mut AsyncPgConnection, new: NewMark) -> QueryResult<Mark> {
    diesel::insert_into(meeting_marks::table)
        .values(new)
        .returning(Mark::as_returning())
        .get_result(connection)
        .await
}

/// Inserts `marks`, made together by one change, in as few statements as Postgres takes.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn insert_all(
    connection: &mut AsyncPgConnection,
    marks: Vec<NewMark>,
) -> QueryResult<()> {
    for chunk in marks.chunks(INSERT_CHUNK) {
        diesel::insert_into(meeting_marks::table)
            .values(chunk)
            .execute(connection)
            .await?;
    }
    Ok(())
}

/// The meeting's lock, or [`MarkError::NotFound`] when there is no such meeting and
/// [`MarkError::Merged`] when it was merged into another, read under the lock a merge takes.
async fn lock_meeting(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> Result<(), MarkError> {
    let locked = found(meeting::lock(connection, meeting_id).await)?;
    if locked.merged_into.is_some() {
        return Err(MarkError::Merged);
    }
    Ok(())
}

/// That `range` runs between two standing words of the meeting, in order.
async fn check_range(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    range: Range,
) -> Result<(), MarkError> {
    let ends = meeting_word::find_many(
        connection,
        meeting_id,
        &[range.start_word_id, range.end_word_id],
    )
    .await?;
    let start = ends.iter().find(|word| word.id == range.start_word_id);
    let end = ends.iter().find(|word| word.id == range.end_word_id);
    let (Some(start), Some(end)) = (start, end) else {
        return Err(MarkError::NotFound);
    };
    if start.superseded_by.is_some() || end.superseded_by.is_some() {
        return Err(MarkError::NotStanding);
    }
    if end.position < start.position {
        return Err(MarkError::OutOfOrder);
    }
    Ok(())
}

/// Records `mark` as made (or put back, undoing `undid`).
async fn record_marked(
    connection: &mut AsyncPgConnection,
    mark: &Mark,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> QueryResult<MeetingEvent> {
    meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id: mark.meeting_id,
            kind: EventKind::Marked,
            author,
            data: json!({
                "markId": mark.id,
                "kind": mark.kind,
                "startWordId": mark.start_word_id,
                "endWordId": mark.end_word_id,
                "data": mark.data,
            }),
            undid,
        },
    )
    .await
}

/// Removes `mark` and records it (undoing `undid`, when it is an undo).
async fn record_unmarked(
    connection: &mut AsyncPgConnection,
    mark: &Mark,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> QueryResult<MeetingEvent> {
    let event = meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id: mark.meeting_id,
            kind: EventKind::Unmarked,
            author,
            data: json!({ "markId": mark.id, "kind": mark.kind }),
            undid,
        },
    )
    .await?;
    remove_all(connection, &[mark.id], event.id).await?;
    Ok(event)
}

/// `result`, with a missing row as [`MarkError::NotFound`].
fn found<Found>(result: QueryResult<Found>) -> Result<Found, MarkError> {
    match result {
        Ok(found) => Ok(found),
        Err(diesel::result::Error::NotFound) => Err(MarkError::NotFound),
        Err(error) => Err(MarkError::Database(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::history::{self, UndoError};
    use crate::models::meeting_word::tests::{built_meeting, person, range, word};
    use crate::models::meeting_word::{self, Replacement};
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    #[test]
    fn every_kind_is_stored_as_text_it_parses() {
        for kind in MarkKind::ALL {
            assert_eq!(MarkKind::parse(kind.as_str()), Some(kind));
            assert!(
                kind.as_str()
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character == '_'),
                "{} fits meeting_marks_kind_check",
                kind.as_str()
            );
        }
        assert_eq!(MarkKind::parse("sideways"), None);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_mark_is_made_removed_and_put_back_each_in_the_meetings_history() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(&mut connection, &[("Ada", "Jon ships Friday")]).await;
        let meeting_id = capture.meeting_id;
        let (jon, friday) = (word(&words, "Jon"), word(&words, "Friday"));

        let (flag, marked) = add(
            &mut connection,
            meeting_id,
            MarkKind::Confidence,
            range(jon, jon),
            json!({ "confidence": 40, "alternatives": ["John"] }),
            person(),
        )
        .await
        .expect("marks");
        assert_eq!(
            (
                flag.kind(),
                flag.created_by,
                flag.data["confidence"].clone()
            ),
            (Some(MarkKind::Confidence), TEST_PERSON_ID, json!(40))
        );
        assert_eq!(marked.kind(), Some(EventKind::Marked));
        assert_eq!(marked.data["markId"], json!(flag.id));
        assert_eq!(
            standing(&mut connection, meeting_id).await.expect("reads"),
            vec![flag.clone()]
        );

        let unmarked = remove(&mut connection, meeting_id, flag.id, person())
            .await
            .expect("removes");
        assert!(
            standing(&mut connection, meeting_id)
                .await
                .expect("reads")
                .is_empty()
        );
        assert!(matches!(
            remove(&mut connection, meeting_id, flag.id, person()).await,
            Err(MarkError::Removed)
        ));
        // Undoing the mark itself would remove it, and it is removed already.
        assert!(matches!(
            history::undo(&mut connection, meeting_id, marked.id, person()).await,
            Err(UndoError::ChangedSince)
        ));

        // Undoing the removal puts it back, and undoing that removes it again.
        let put_back = history::undo(&mut connection, meeting_id, unmarked.id, person())
            .await
            .expect("puts back")
            .entry;
        assert_eq!(put_back.kind(), Some(EventKind::Marked));
        assert_eq!(put_back.undid(), Some(unmarked.id));
        assert_eq!(
            find(&mut connection, meeting_id, flag.id)
                .await
                .expect("the mark")
                .removed_by,
            None
        );
        history::undo(&mut connection, meeting_id, put_back.id, person())
            .await
            .expect("removes again");
        assert!(
            standing(&mut connection, meeting_id)
                .await
                .expect("reads")
                .is_empty()
        );

        // A mark over the whole meeting, from its first word to its last.
        let (whole, _) = add(
            &mut connection,
            meeting_id,
            MarkKind::BlackOut,
            range(jon, friday),
            json!({}),
            person(),
        )
        .await
        .expect("marks");
        assert_eq!(whole.range(), range(jon, friday));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_mark_is_refused_off_the_transcript_and_an_edits_own_goes_with_its_edit() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(&mut connection, &[("Ada", "Jon ships Friday")]).await;
        let (_other, other_words) = built_meeting(&mut connection, &[("Grace", "Elsewhere")]).await;
        let meeting_id = capture.meeting_id;
        let (jon, friday) = (word(&words, "Jon"), word(&words, "Friday"));

        let backwards = add(
            &mut connection,
            meeting_id,
            MarkKind::Comment,
            range(friday, jon),
            json!({}),
            person(),
        )
        .await;
        assert!(
            matches!(backwards, Err(MarkError::OutOfOrder)),
            "{backwards:?}"
        );
        let elsewhere = add(
            &mut connection,
            meeting_id,
            MarkKind::Comment,
            range(jon, word(&other_words, "Elsewhere")),
            json!({}),
            person(),
        )
        .await;
        assert!(
            matches!(elsewhere, Err(MarkError::NotFound)),
            "{elsewhere:?}"
        );
        let not_an_object = add(
            &mut connection,
            meeting_id,
            MarkKind::Comment,
            range(jon, jon),
            json!(["text"]),
            person(),
        )
        .await;
        assert!(
            matches!(not_an_object, Err(MarkError::NotAnObject)),
            "{not_an_object:?}"
        );
        let by_hand = add(
            &mut connection,
            meeting_id,
            MarkKind::Edit,
            range(jon, jon),
            json!({}),
            person(),
        )
        .await;
        assert!(matches!(by_hand, Err(MarkError::EditsOwn)), "{by_hand:?}");

        let edited = meeting_word::edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(jon, jon),
                text: "John",
            },
            person(),
        )
        .await
        .expect("edits");
        let own: Uuid =
            serde_json::from_value(edited.data["marksAdded"][0].clone()).expect("the edit's mark");
        assert!(matches!(
            remove(&mut connection, meeting_id, own, person()).await,
            Err(MarkError::EditsOwn)
        ));
    }
}
