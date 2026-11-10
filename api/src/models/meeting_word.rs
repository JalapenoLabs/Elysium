// Copyright © 2026 Jalapeno Labs

//! A meeting's final transcript: a stream of words, each with a stable id. See
//! `docs/meetings.md`, The final transcript.
//!
//! Every word has a speaker, a start and end on the meeting's clock, and where it came from: the
//! source transcript of the capture it was heard in, that capture's Whisper transcript, a
//! correction of what the capture heard, or an edit. Marks (`models::meeting_mark`) are anchored
//! to a range of word ids, so a mark is one word or the whole meeting.
//!
//! # Every word it ever held
//!
//! A word is never deleted or rewritten. An edit ([`edit`]) marks the words of its range as
//! replaced (`superseded_by`) and writes new ones in their place, so an undo ([`undo`]) puts the
//! old words back and every mark and history entry naming a word still resolves. The transcript is
//! the words that stand, in order of `position`.
//!
//! `position` orders every word the meeting ever held, standing or replaced, in one sequence with
//! gaps of [`POSITION_GAP`]. An edit writes its new words just after the last word it replaces and
//! before whatever came next, so the words it replaced stay exactly where they were, and an undo
//! that stands them up again finds them between the same neighbours. When no gap is left there,
//! the meeting's words are renumbered in one statement, keeping their order.
//!
//! # Marks over an edit
//!
//! Marks over the words an edit replaces cover the words that replace them: a mark starting in
//! the range starts at the first new word, and one ending in it ends at the last. The edit also
//! makes a mark of its own, of kind `edit`, over its new words. Its history entry records every
//! mark it moved, so its undo moves each back exactly where it was, unless a later change moved
//! it since; marks made over the new words afterwards move onto the words put back the same way.
//!
//! Every change is made under the meeting's lock and recorded in its history
//! (`models::meeting_event`) in the same transaction.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use crate::database::schema::{meeting_marks, meeting_words};
use crate::models::agent::Author;
use crate::models::meeting;
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::meeting_mark::{self, Mark, MarkKind, NewMark, Range};

/// The gap between two neighbouring words' positions as a transcript is built or renumbered:
/// room for an edit to write a thousand words between any two before the meeting's words need
/// renumbering. `crate::meetings::final_transcript` and the migration that split meetings into
/// captures number with it too.
pub const POSITION_GAP: i64 = 1024;

/// The most characters one edit writes: a long paragraph rewritten by hand, and far less than a
/// request body allows.
pub const EDIT_TEXT_MAX_CHARACTERS: usize = 20_000;

/// How many words one insert writes. Each word binds ten parameters, and Postgres takes at most
/// 65,535 in one statement.
const INSERT_CHUNK: usize = 1_000;

/// Where a word came from. Stored as `meeting_words.origin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Its capture's source transcript, as the source sent it.
    Source,
    /// Its capture's Whisper transcript.
    Whisper,
    /// A correction of what its capture heard.
    Correction,
    /// An edit, by a person or an agent; no capture said it.
    Edit,
}

impl Origin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Whisper => "whisper",
            Self::Correction => "correction",
            Self::Edit => "edit",
        }
    }
}

/// What a meeting's final transcript was built from. Stored in the `built` entry of its history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltFrom {
    /// Its corrected copy.
    Correction,
    /// Its capture's source transcript, uncorrected.
    Source,
    /// Its capture's Whisper transcript, uncorrected, for a source that sent no lines.
    Whisper,
}

/// One word of a meeting's final transcript, standing or replaced.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = meeting_words, check_for_backend(diesel::pg::Pg))]
pub struct Word {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub position: i64,
    pub text: String,
    pub speaker: String,
    /// On the meeting's clock.
    pub start_ms: i64,
    pub end_ms: i64,
    /// An [`Origin`] name.
    pub origin: String,
    /// The capture it was heard in; `None` for an edit's words.
    pub capture_id: Option<Uuid>,
    /// The change that wrote it.
    pub introduced_by: Uuid,
    /// The change that replaced it; `None` while it stands.
    pub superseded_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_words)]
struct NewWord {
    id: Uuid,
    meeting_id: Uuid,
    position: i64,
    text: String,
    speaker: String,
    start_ms: i64,
    end_ms: i64,
    origin: String,
    capture_id: Option<Uuid>,
    introduced_by: Uuid,
}

/// One word of a transcript about to be built, already on the meeting's clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltWord {
    pub text: String,
    pub speaker: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub origin: Origin,
}

/// A meeting's final transcript as it is first built, from one capture.
#[derive(Debug, Clone)]
pub struct Build {
    pub meeting_id: Uuid,
    pub capture_id: Uuid,
    pub built_from: BuiltFrom,
    /// In order.
    pub words: Vec<BuiltWord>,
}

/// Stores meeting `build.meeting_id`'s final transcript, `build`, as `author`, and records it in
/// its history, unless the meeting has one already. Answers the history entry, or `None`, writing
/// nothing, when the meeting has words already or `build` has none: a transcript is built once,
/// and every change after is an edit.
///
/// # Errors
/// Propagates any database error, such as a meeting that does not exist.
pub async fn insert_built(
    connection: &mut AsyncPgConnection,
    build: Build,
    author: Author<'_>,
) -> QueryResult<Option<MeetingEvent>> {
    if build.words.is_empty() {
        return Ok(None);
    }
    connection
        .transaction(async |connection| {
            meeting::lock(connection, build.meeting_id).await?;
            if has_words(connection, build.meeting_id).await? {
                return Ok(None);
            }
            let entry = meeting_event::record(
                connection,
                Change {
                    id: Uuid::now_v7(),
                    meeting_id: build.meeting_id,
                    kind: EventKind::Built,
                    author,
                    data: json!({
                        "captureId": build.capture_id,
                        "builtFrom": build.built_from,
                        "words": build.words.len(),
                    }),
                    undid: None,
                },
            )
            .await?;
            let rows: Vec<NewWord> = build
                .words
                .into_iter()
                .zip(1_i64..)
                .map(|(word, number)| NewWord {
                    id: Uuid::now_v7(),
                    meeting_id: build.meeting_id,
                    position: number * POSITION_GAP,
                    text: word.text,
                    speaker: word.speaker,
                    start_ms: word.start_ms,
                    end_ms: word.end_ms,
                    origin: word.origin.as_str().to_owned(),
                    capture_id: Some(build.capture_id),
                    introduced_by: entry.id,
                })
                .collect();
            insert_words(connection, rows).await?;
            Ok(Some(entry))
        })
        .await
}

/// Whether meeting `meeting_id` has a final transcript.
///
/// # Errors
/// Propagates any database error.
pub async fn has_words(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        meeting_words::table.filter(meeting_words::meeting_id.eq(meeting_id)),
    ))
    .get_result(connection)
    .await
}

/// Meeting `meeting_id`'s final transcript as it reads: its standing words, in order.
///
/// # Errors
/// Propagates any database error.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the meeting's page reads it (#234)")
)]
pub async fn standing(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Vec<Word>> {
    meeting_words::table
        .filter(meeting_words::meeting_id.eq(meeting_id))
        .filter(meeting_words::superseded_by.is_null())
        .order(meeting_words::position.asc())
        .select(Word::as_select())
        .load(connection)
        .await
}

/// The words of meeting `meeting_id` among `ids`, standing or replaced, in order.
///
/// # Errors
/// Propagates any database error.
pub async fn find_many(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    ids: &[Uuid],
) -> QueryResult<Vec<Word>> {
    meeting_words::table
        .filter(meeting_words::meeting_id.eq(meeting_id))
        .filter(meeting_words::id.eq_any(ids))
        .order(meeting_words::position.asc())
        .select(Word::as_select())
        .load(connection)
        .await
}

/// A transcript's words read as text, a space between each.
pub fn text_of(words: &[Word]) -> String {
    words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What one change did to a meeting's words, as its history entry (`edited`) records it, so it can
/// be undone exactly. An undo is a change of the same shape, its inverse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordChange {
    /// The words it replaced, in order, which stand no longer.
    pub replaced: Vec<Uuid>,
    /// The words it stood in their place, in order: written by it, or put back by an undo.
    pub words: Vec<Uuid>,
    /// Every mark it moved, with its range before and after.
    pub marks: Vec<MovedMark>,
    /// The marks it made or put back: an edit's own mark.
    pub marks_added: Vec<Uuid>,
    /// The marks it removed: an undone edit's own mark.
    pub marks_removed: Vec<Uuid>,
    /// What the replaced words said, and what their replacement says.
    pub before_text: String,
    pub after_text: String,
}

/// One mark a change moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovedMark {
    pub id: Uuid,
    pub before: Range,
    pub after: Range,
}

/// An edit: the range of standing words it replaces, and what it writes in their place.
#[derive(Debug, Clone, Copy)]
pub struct Replacement<'text> {
    pub range: Range,
    /// Split at whitespace into the new words.
    pub text: &'text str,
}

/// Why an edit could not be made.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// The meeting, or a word of the range, is not the meeting's.
    #[error("not found")]
    NotFound,
    /// A word of the range was replaced already.
    #[error("a word of the range was replaced")]
    NotStanding,
    /// The range ends before it starts.
    #[error("the range ends before it starts")]
    OutOfOrder,
    /// The edit writes no words. Leaving words out of what matters is a black-out's work.
    #[error("an edit writes at least one word")]
    Empty,
    /// The edit writes more than [`EDIT_TEXT_MAX_CHARACTERS`].
    #[error("an edit writes at most {EDIT_TEXT_MAX_CHARACTERS} characters")]
    TooLong,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Replaces the standing words of `replacement.range` in meeting `meeting_id` with
/// `replacement.text`'s words, as `author`, and records it. The new words take the replaced
/// words' span of time, spread evenly, and the first replaced word's speaker. Every mark over the
/// replaced words moves onto the new ones, and the edit makes a mark of its own over them (the
/// module's docs). Answers the history entry.
///
/// # Errors
/// [`EditError::NotFound`], [`EditError::NotStanding`], or [`EditError::OutOfOrder`] for a range
/// that is not one of the transcript as it reads, [`EditError::Empty`] or
/// [`EditError::TooLong`] for the text; any database error.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "people edit words from the page (#234)")
)]
pub async fn edit(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    replacement: Replacement<'_>,
    author: Author<'_>,
) -> Result<MeetingEvent, EditError> {
    if replacement.text.chars().count() > EDIT_TEXT_MAX_CHARACTERS {
        return Err(EditError::TooLong);
    }
    let texts: Vec<&str> = replacement.text.split_whitespace().collect();
    if texts.is_empty() {
        return Err(EditError::Empty);
    }
    connection
        .transaction(async |connection| {
            match meeting::lock(connection, meeting_id).await {
                Ok(_) => {}
                Err(diesel::result::Error::NotFound) => return Err(EditError::NotFound),
                Err(error) => return Err(EditError::Database(error)),
            }
            let range = replacement.range;
            let (first, last) = standing_ends(connection, meeting_id, range).await?;
            let replaced = standing_between(connection, meeting_id, &first, &last).await?;
            let positions = room_after(connection, meeting_id, last.id, texts.len()).await?;

            let event_id = Uuid::now_v7();
            let start_ms = first.start_ms;
            let end_ms = last.end_ms.max(start_ms);
            let count = i64::try_from(texts.len()).expect("an edit's words fit a 64-bit count");
            let new_words: Vec<NewWord> = texts
                .iter()
                .zip(positions)
                .zip(0_i64..)
                .map(|((text, position), index)| NewWord {
                    id: Uuid::now_v7(),
                    meeting_id,
                    position,
                    text: (*text).to_owned(),
                    speaker: first.speaker.clone(),
                    start_ms: start_ms + (end_ms - start_ms) * index / count,
                    end_ms: start_ms + (end_ms - start_ms) * (index + 1) / count,
                    origin: Origin::Edit.as_str().to_owned(),
                    capture_id: None,
                    introduced_by: event_id,
                })
                .collect();
            let replaced_ids: Vec<Uuid> = replaced.iter().map(|word| word.id).collect();
            let new_ids: Vec<Uuid> = new_words.iter().map(|word| word.id).collect();
            let onto = Range {
                start_word_id: new_ids[0],
                end_word_id: new_ids[new_ids.len() - 1],
            };
            let anchored = meeting_mark::anchored_in(connection, meeting_id, &replaced_ids).await?;
            let moves = moves_onto(&anchored, &replaced_ids, onto, &HashMap::new());
            let edit_mark = NewMark::new(meeting_id, MarkKind::Edit, onto, json!({}), author);
            let change = WordChange {
                replaced: replaced_ids.clone(),
                words: new_ids,
                marks: moves.clone(),
                marks_added: vec![edit_mark.id],
                marks_removed: Vec::new(),
                before_text: text_of(&replaced),
                after_text: texts.join(" "),
            };

            let entry =
                record_change(connection, meeting_id, event_id, &change, author, None).await?;
            supersede(connection, &replaced_ids, event_id).await?;
            insert_words(connection, new_words).await?;
            move_marks(connection, &moves).await?;
            meeting_mark::insert(connection, edit_mark).await?;
            Ok(entry)
        })
        .await
}

/// Undoes `undone`, an `edited` entry, as `author`: the words it replaced stand again, its words
/// are replaced, the marks it moved go back where they were, and its own mark goes. The undo is
/// an entry of the same shape, so it can be undone in turn. Answers `None`, changing nothing, when
/// its words moved on since, another change having replaced one of them or put back what it
/// replaced, so undoing it would overwrite that. The caller holds the meeting's lock.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn undo(
    connection: &mut AsyncPgConnection,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> QueryResult<Option<MeetingEvent>> {
    let change: WordChange = match serde_json::from_value(undone.data.clone()) {
        Ok(change) => change,
        Err(error) => {
            event!(
                name: "meetings.history.unreadable",
                Level::WARN,
                meeting_event.id = %undone.id,
                error.message = %error,
                "an edit's history entry is not in its shape; it cannot be undone",
            );
            return Ok(None);
        }
    };
    let meeting_id = undone.meeting_id;
    if !as_it_left_them(connection, undone, &change).await? {
        return Ok(None);
    }
    let (Some(first), Some(last)) = (change.replaced.as_slice().first(), change.replaced.last())
    else {
        // Only an entry written by hand replaces nothing.
        return Ok(None);
    };
    let onto = Range {
        start_word_id: *first,
        end_word_id: *last,
    };

    let event_id = Uuid::now_v7();
    let anchored = meeting_mark::anchored_in(connection, meeting_id, &change.words).await?;
    let exact: HashMap<Uuid, MovedMark> = change
        .marks
        .iter()
        .map(|moved| (moved.id, *moved))
        .collect();
    let moves = moves_onto(&anchored, &change.words, onto, &exact);
    let (to_remove, to_restore) = its_own_marks(connection, undone, &change).await?;
    let inverse = WordChange {
        replaced: change.words.clone(),
        words: change.replaced.clone(),
        marks: moves.clone(),
        marks_added: to_restore.clone(),
        marks_removed: to_remove.clone(),
        before_text: change.after_text,
        after_text: change.before_text,
    };

    let entry = record_change(
        connection,
        meeting_id,
        event_id,
        &inverse,
        author,
        Some(undone.id),
    )
    .await?;
    supersede(connection, &change.words, event_id).await?;
    diesel::update(
        meeting_words::table
            .filter(meeting_words::id.eq_any(&change.replaced))
            .filter(meeting_words::superseded_by.eq(undone.id)),
    )
    .set(meeting_words::superseded_by.eq(None::<Uuid>))
    .execute(connection)
    .await?;
    move_marks(connection, &moves).await?;
    meeting_mark::remove_all(connection, &to_remove, event_id).await?;
    meeting_mark::restore_all(connection, &to_restore, undone.id).await?;
    Ok(Some(entry))
}

/// Whether `change`, which `undone` recorded, still holds: every word it stood stands, and every
/// word it replaced is still replaced by it.
async fn as_it_left_them(
    connection: &mut AsyncPgConnection,
    undone: &MeetingEvent,
    change: &WordChange,
) -> QueryResult<bool> {
    let named: Vec<Uuid> = change
        .words
        .iter()
        .chain(&change.replaced)
        .copied()
        .collect();
    let words: HashMap<Uuid, Word> = find_many(connection, undone.meeting_id, &named)
        .await?
        .into_iter()
        .map(|word| (word.id, word))
        .collect();
    let its_words_stand = change.words.iter().all(|id| {
        words
            .get(id)
            .is_some_and(|word| word.superseded_by.is_none())
    });
    let what_it_replaced_is_still_replaced_by_it = change.replaced.iter().all(|id| {
        words
            .get(id)
            .is_some_and(|word| word.superseded_by == Some(undone.id))
    });
    Ok(its_words_stand && what_it_replaced_is_still_replaced_by_it)
}

/// The marks `change`, which `undone` recorded, made or put back that still stand, which its undo
/// removes, and the marks it removed that are still removed by it, which its undo puts back.
async fn its_own_marks(
    connection: &mut AsyncPgConnection,
    undone: &MeetingEvent,
    change: &WordChange,
) -> QueryResult<(Vec<Uuid>, Vec<Uuid>)> {
    let named: Vec<Uuid> = change
        .marks_added
        .iter()
        .chain(&change.marks_removed)
        .copied()
        .collect();
    let marks: HashMap<Uuid, Mark> = meeting_marks::table
        .filter(meeting_marks::meeting_id.eq(undone.meeting_id))
        .filter(meeting_marks::id.eq_any(&named))
        .select(Mark::as_select())
        .load(connection)
        .await?
        .into_iter()
        .map(|mark| (mark.id, mark))
        .collect();
    let to_remove = change
        .marks_added
        .iter()
        .filter(|id| marks.get(id).is_some_and(|mark| mark.removed_by.is_none()))
        .copied()
        .collect();
    let to_restore = change
        .marks_removed
        .iter()
        .filter(|id| {
            marks
                .get(id)
                .is_some_and(|mark| mark.removed_by == Some(undone.id))
        })
        .copied()
        .collect();
    Ok((to_remove, to_restore))
}

/// Where each mark of `anchored` goes when the words `replaced` give way to `onto`: a mark the
/// change being undone moved, and that nothing moved since, goes back exactly where it was
/// (`exact`, by mark); any other end among `replaced` moves to the matching end of `onto`. Marks
/// left where they are are not listed.
fn moves_onto(
    anchored: &[Mark],
    replaced: &[Uuid],
    onto: Range,
    exact: &HashMap<Uuid, MovedMark>,
) -> Vec<MovedMark> {
    let replaced: HashSet<&Uuid> = replaced.iter().collect();
    let mut moves = Vec::new();
    for mark in anchored {
        let current = mark.range();
        let moved_to = match exact.get(&mark.id) {
            Some(recorded) if recorded.after == current => recorded.before,
            _ => Range {
                start_word_id: if replaced.contains(&current.start_word_id) {
                    onto.start_word_id
                } else {
                    current.start_word_id
                },
                end_word_id: if replaced.contains(&current.end_word_id) {
                    onto.end_word_id
                } else {
                    current.end_word_id
                },
            },
        };
        if moved_to != current {
            moves.push(MovedMark {
                id: mark.id,
                before: current,
                after: moved_to,
            });
        }
    }
    moves
}

/// The first and last word of `range`, both standing words of the meeting, in order.
async fn standing_ends(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    range: Range,
) -> Result<(Word, Word), EditError> {
    let ends = find_many(
        connection,
        meeting_id,
        &[range.start_word_id, range.end_word_id],
    )
    .await?;
    let first = ends.iter().find(|word| word.id == range.start_word_id);
    let last = ends.iter().find(|word| word.id == range.end_word_id);
    let (Some(first), Some(last)) = (first, last) else {
        return Err(EditError::NotFound);
    };
    if first.superseded_by.is_some() || last.superseded_by.is_some() {
        return Err(EditError::NotStanding);
    }
    if last.position < first.position {
        return Err(EditError::OutOfOrder);
    }
    Ok((first.clone(), last.clone()))
}

/// The standing words from `first` to `last`, in order.
async fn standing_between(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    first: &Word,
    last: &Word,
) -> QueryResult<Vec<Word>> {
    meeting_words::table
        .filter(meeting_words::meeting_id.eq(meeting_id))
        .filter(meeting_words::superseded_by.is_null())
        .filter(meeting_words::position.between(first.position, last.position))
        .order(meeting_words::position.asc())
        .select(Word::as_select())
        .load(connection)
        .await
}

/// `count` free positions just after word `after_id` and before whatever word comes next,
/// standing or replaced, renumbering the meeting's words first when the gap there is too small.
async fn room_after(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    after_id: Uuid,
    count: usize,
) -> QueryResult<Vec<i64>> {
    let wanted = i64::try_from(count).expect("an edit's words fit a 64-bit count");
    let after = position_of(connection, after_id).await?;
    let next: Option<i64> = meeting_words::table
        .filter(meeting_words::meeting_id.eq(meeting_id))
        .filter(meeting_words::position.gt(after))
        .select(diesel::dsl::min(meeting_words::position))
        .first(connection)
        .await?;
    let (after, step) = match next {
        None => (after, POSITION_GAP),
        Some(next) if next - after > wanted => (after, (next - after) / (wanted + 1)),
        Some(_) => {
            renumber(connection, meeting_id, after, wanted).await?;
            (position_of(connection, after_id).await?, POSITION_GAP)
        }
    };
    Ok((1..=wanted).map(|index| after + step * index).collect())
}

async fn position_of(connection: &mut AsyncPgConnection, word_id: Uuid) -> QueryResult<i64> {
    meeting_words::table
        .find(word_id)
        .select(meeting_words::position)
        .first(connection)
        .await
}

/// Renumbers every word of meeting `meeting_id`, standing or replaced, [`POSITION_GAP`] apart in
/// the order they hold, leaving `wanted` gaps more after the word at position `after`, so an edit
/// of that many words fits there whatever its length. One statement, which
/// `meeting_words_order`, deferrable, checks once at its end rather than row by row.
async fn renumber(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    after: i64,
    wanted: i64,
) -> QueryResult<()> {
    diesel::sql_query(
        "UPDATE meeting_words \
         SET position = (ranked.number + CASE WHEN ranked.position > $2 THEN $3 ELSE 0 END) * $4 \
         FROM (SELECT id, position, row_number() OVER (ORDER BY position) AS number \
               FROM meeting_words WHERE meeting_id = $1) AS ranked \
         WHERE meeting_words.id = ranked.id",
    )
    .bind::<SqlUuid, _>(meeting_id)
    .bind::<BigInt, _>(after)
    .bind::<BigInt, _>(wanted)
    .bind::<BigInt, _>(POSITION_GAP)
    .execute(connection)
    .await?;
    event!(
        name: "meetings.transcript.renumbered",
        Level::DEBUG,
        meeting.id = %meeting_id,
        "an edit found no room between two words, so the meeting's words were renumbered",
    );
    Ok(())
}

/// Marks the standing words among `ids` as replaced by change `event_id`.
async fn supersede(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
    event_id: Uuid,
) -> QueryResult<()> {
    diesel::update(
        meeting_words::table
            .filter(meeting_words::id.eq_any(ids))
            .filter(meeting_words::superseded_by.is_null()),
    )
    .set(meeting_words::superseded_by.eq(event_id))
    .execute(connection)
    .await
    .map(drop)
}

async fn move_marks(connection: &mut AsyncPgConnection, moves: &[MovedMark]) -> QueryResult<()> {
    for moved in moves {
        meeting_mark::set_range(connection, moved.id, moved.after).await?;
    }
    Ok(())
}

async fn insert_words(connection: &mut AsyncPgConnection, rows: Vec<NewWord>) -> QueryResult<()> {
    for chunk in rows.chunks(INSERT_CHUNK) {
        diesel::insert_into(meeting_words::table)
            .values(chunk)
            .execute(connection)
            .await?;
    }
    Ok(())
}

/// Records `change` as entry `event_id` of meeting `meeting_id`'s history.
async fn record_change(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    event_id: Uuid,
    change: &WordChange,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> QueryResult<MeetingEvent> {
    let data: Value = serde_json::to_value(change).expect("a word change is plain fields");
    meeting_event::record(
        connection,
        Change {
            id: event_id,
            meeting_id,
            kind: EventKind::Edited,
            author,
            data,
            undid,
        },
    )
    .await
}

#[cfg(test)]
pub(crate) mod tests {
    use diesel_async::SimpleAsyncConnection;
    use serde_json::json;

    use super::*;
    use crate::meetings::correction::Base;
    use crate::meetings::final_transcript;
    use crate::meetings::history::{self, UndoError};
    use crate::meetings::source::TranscriptLine;
    use crate::models::meeting::tests::fetched_with;
    use crate::models::meeting_capture::MeetingCapture;
    use crate::models::meeting_correction::BasedOn;
    use crate::models::meeting_mark::MarkError;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    pub(crate) fn person() -> Author<'static> {
        Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: Utc::now(),
        }
    }

    /// A meeting whose final transcript was built, as its source heard it, from `lines`, each
    /// `(speaker, text)` two seconds long, one after another.
    pub(crate) async fn built_meeting(
        connection: &mut AsyncPgConnection,
        lines: &[(&str, &str)],
    ) -> (MeetingCapture, Vec<Word>) {
        let heard: Vec<TranscriptLine> = lines
            .iter()
            .zip(0_i64..)
            .map(|((speaker, text), number)| TranscriptLine {
                start_ms: number * 2_000,
                end_ms: number * 2_000 + 2_000,
                speaker: (*speaker).to_owned(),
                text: (*text).to_owned(),
            })
            .collect();
        let (meeting, capture) = fetched_with(
            connection,
            serde_json::to_value(&heard).expect("serializes"),
        )
        .await;
        let base = Base {
            based_on: BasedOn::Source,
            lines: heard,
        };
        final_transcript::ensure_built(connection, &capture, &base, None)
            .await
            .expect("builds")
            .expect("a transcript");
        let words = standing(connection, meeting.id).await.expect("reads");
        (capture, words)
    }

    /// The word of `words` that reads `text`.
    pub(crate) fn word<'words>(words: &'words [Word], text: &str) -> &'words Word {
        words
            .iter()
            .find(|word| word.text == text)
            .unwrap_or_else(|| panic!("no word {text:?} in {words:?}"))
    }

    pub(crate) fn range(first: &Word, last: &Word) -> Range {
        Range {
            start_word_id: first.id,
            end_word_id: last.id,
        }
    }

    async fn text(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> String {
        text_of(&standing(connection, meeting_id).await.expect("reads"))
    }

    async fn mark(connection: &mut AsyncPgConnection, meeting_id: Uuid, id: Uuid) -> Mark {
        meeting_mark::find(connection, meeting_id, id)
            .await
            .expect("the mark")
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_transcript_is_built_once_with_every_word_naming_its_build() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(
            &mut connection,
            &[("Ada", "Launch is Friday."), ("Grace", "Agreed.")],
        )
        .await;
        assert_eq!(text_of(&words), "Launch is Friday. Agreed.");
        assert_eq!(
            words
                .iter()
                .map(|word| (word.speaker.as_str(), word.position))
                .collect::<Vec<_>>(),
            vec![("Ada", 1024), ("Ada", 2048), ("Ada", 3072), ("Grace", 4096)]
        );
        let history = meeting_event::history(&mut connection, capture.meeting_id)
            .await
            .expect("history");
        let [built] = history.as_slice() else {
            panic!("one build, not {history:?}");
        };
        assert_eq!(built.kind(), Some(EventKind::Built));
        assert!(words.iter().all(|word| word.introduced_by == built.id));
        assert!(
            words
                .iter()
                .all(|word| word.capture_id == Some(capture.id) && word.origin == "source")
        );

        // Building again changes nothing: a transcript is built once, and edited after.
        let other = Base {
            based_on: BasedOn::Source,
            lines: vec![TranscriptLine {
                start_ms: 0,
                end_ms: 1,
                speaker: String::new(),
                text: "Different".to_owned(),
            }],
        };
        assert!(
            final_transcript::ensure_built(&mut connection, &capture, &other, None)
                .await
                .expect("answers")
                .is_none()
        );
        assert_eq!(
            text(&mut connection, capture.meeting_id).await,
            "Launch is Friday. Agreed."
        );
        assert!(matches!(
            history::undo(&mut connection, capture.meeting_id, built.id, person()).await,
            Err(UndoError::NotUndoable)
        ));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    #[expect(
        clippy::too_many_lines,
        reason = "two edits over three marks, undone and redone, with what each left behind"
    )]
    async fn an_edit_replaces_its_range_and_marks_over_it_cover_its_words_until_undone() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(
            &mut connection,
            &[("Ada", "Jon ships Friday"), ("Grace", "Agreed")],
        )
        .await;
        let meeting_id = capture.meeting_id;
        let (jon, ships, friday, agreed) = (
            word(&words, "Jon").clone(),
            word(&words, "ships").clone(),
            word(&words, "Friday").clone(),
            word(&words, "Agreed").clone(),
        );
        let (excerpt, _) = meeting_mark::add(
            &mut connection,
            meeting_id,
            MarkKind::Excerpt,
            range(&jon, &friday),
            json!({}),
            person(),
        )
        .await
        .expect("an excerpt");
        let (comment, _) = meeting_mark::add(
            &mut connection,
            meeting_id,
            MarkKind::Comment,
            range(&ships, &ships),
            json!({}),
            person(),
        )
        .await
        .expect("a comment");
        let (black_out, _) = meeting_mark::add(
            &mut connection,
            meeting_id,
            MarkKind::BlackOut,
            range(&friday, &agreed),
            json!({}),
            person(),
        )
        .await
        .expect("a black-out");

        // A name heard wrong: one word becomes two, in the span the old word took.
        let renamed = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&jon, &jon),
                text: " John  Smith ",
            },
            person(),
        )
        .await
        .expect("edits");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "John Smith ships Friday Agreed"
        );
        let standing_now = standing(&mut connection, meeting_id).await.expect("reads");
        let (first_name, surname) = (word(&standing_now, "John"), word(&standing_now, "Smith"));
        assert_eq!(
            (
                first_name.start_ms,
                first_name.end_ms,
                surname.start_ms,
                surname.end_ms
            ),
            (jon.start_ms, 333, 333, jon.end_ms),
            "the new words share the span the old word took"
        );
        assert_eq!(
            (
                first_name.origin.as_str(),
                first_name.capture_id,
                first_name.speaker.as_str()
            ),
            ("edit", None, "Ada")
        );
        assert_eq!(first_name.introduced_by, renamed.id);
        // The excerpt started on the word replaced, so it starts on the first new word now.
        assert_eq!(
            mark(&mut connection, meeting_id, excerpt.id).await.range(),
            range(first_name, &friday)
        );
        let change: WordChange = serde_json::from_value(renamed.data.clone()).expect("reads");
        assert_eq!(change.before_text, "Jon");
        assert_eq!(change.after_text, "John Smith");
        let [edit_mark] = change.marks_added.as_slice() else {
            panic!("an edit makes one mark of its own");
        };
        let own = mark(&mut connection, meeting_id, *edit_mark).await;
        assert_eq!(
            (own.kind(), own.range(), own.created_by),
            (
                Some(MarkKind::Edit),
                range(first_name, surname),
                TEST_PERSON_ID
            )
        );

        // A second edit across the comment's word and the black-out's start.
        let reworded = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&ships, &friday),
                text: "is shipping Monday",
            },
            person(),
        )
        .await
        .expect("edits");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "John Smith is shipping Monday Agreed"
        );
        let standing_now = standing(&mut connection, meeting_id).await.expect("reads");
        let (is, monday) = (word(&standing_now, "is"), word(&standing_now, "Monday"));
        assert_eq!(
            mark(&mut connection, meeting_id, comment.id).await.range(),
            range(is, monday),
            "a mark inside the range covers all of its new words"
        );
        assert_eq!(
            mark(&mut connection, meeting_id, black_out.id)
                .await
                .range(),
            range(is, &agreed)
        );
        assert_eq!(
            mark(&mut connection, meeting_id, excerpt.id).await.range(),
            range(word(&standing_now, "John"), monday)
        );

        // Undoing the second puts its words and marks back exactly.
        let undone = history::undo(&mut connection, meeting_id, reworded.id, person())
            .await
            .expect("undoes");
        assert_eq!(undone.kind(), Some(EventKind::Edited));
        assert_eq!(undone.undid(), Some(reworded.id));
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "John Smith ships Friday Agreed"
        );
        assert_eq!(
            mark(&mut connection, meeting_id, comment.id).await.range(),
            range(&ships, &ships)
        );
        assert_eq!(
            mark(&mut connection, meeting_id, black_out.id)
                .await
                .range(),
            range(&friday, &agreed)
        );
        let second: WordChange = serde_json::from_value(reworded.data.clone()).expect("reads");
        let second_mark = mark(&mut connection, meeting_id, second.marks_added[0]).await;
        assert_eq!(
            second_mark.removed_by,
            Some(undone.id),
            "its own mark goes with it"
        );
        // A change is undone once; again, its words are gone.
        assert!(matches!(
            history::undo(&mut connection, meeting_id, reworded.id, person()).await,
            Err(UndoError::ChangedSince)
        ));

        // Undoing the undo does the edit again, its mark back.
        history::undo(&mut connection, meeting_id, undone.id, person())
            .await
            .expect("redoes");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "John Smith is shipping Monday Agreed"
        );
        assert_eq!(
            mark(&mut connection, meeting_id, second_mark.id)
                .await
                .removed_by,
            None
        );

        // The first edit's words were not replaced since, so it undoes too, under the second.
        history::undo(&mut connection, meeting_id, renamed.id, person())
            .await
            .expect("undoes");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "Jon is shipping Monday Agreed"
        );
        let standing_now = standing(&mut connection, meeting_id).await.expect("reads");
        assert_eq!(
            mark(&mut connection, meeting_id, excerpt.id).await.range(),
            range(&jon, word(&standing_now, "Monday"))
        );
        // Every mark stands on standing words, removed ones too.
        let anchored: Vec<Uuid> = standing_now.iter().map(|word| word.id).collect();
        for id in [
            excerpt.id,
            comment.id,
            black_out.id,
            *edit_mark,
            second_mark.id,
        ] {
            let anchors = mark(&mut connection, meeting_id, id).await.range();
            assert!(
                anchored.contains(&anchors.start_word_id)
                    && anchored.contains(&anchors.end_word_id),
                "mark {id} stands on standing words"
            );
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_edit_whose_words_were_edited_since_cannot_be_undone() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(&mut connection, &[("Ada", "Jon ships")]).await;
        let meeting_id = capture.meeting_id;
        let jon = word(&words, "Jon").clone();
        let first = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&jon, &jon),
                text: "John",
            },
            person(),
        )
        .await
        .expect("edits");
        let standing_now = standing(&mut connection, meeting_id).await.expect("reads");
        let corrected_name = word(&standing_now, "John").clone();
        edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&corrected_name, &corrected_name),
                text: "Johnny",
            },
            person(),
        )
        .await
        .expect("edits again");

        assert!(matches!(
            history::undo(&mut connection, meeting_id, first.id, person()).await,
            Err(UndoError::ChangedSince)
        ));
        assert_eq!(text(&mut connection, meeting_id).await, "Johnny ships");
        // A replaced word is no end of a range, for an edit or a mark.
        assert!(matches!(
            edit(
                &mut connection,
                meeting_id,
                Replacement {
                    range: range(&jon, &jon),
                    text: "Jo",
                },
                person(),
            )
            .await,
            Err(EditError::NotStanding)
        ));
        assert!(matches!(
            meeting_mark::add(
                &mut connection,
                meeting_id,
                MarkKind::Comment,
                range(&corrected_name, &corrected_name),
                json!({}),
                person(),
            )
            .await,
            Err(MarkError::NotStanding)
        ));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_edit_is_refused_unless_it_is_a_range_of_the_transcript_and_writes_words() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(&mut connection, &[("Ada", "Jon ships Friday")]).await;
        let (_other, other_words) = built_meeting(&mut connection, &[("Grace", "Elsewhere")]).await;
        let meeting_id = capture.meeting_id;
        let (jon, friday) = (word(&words, "Jon"), word(&words, "Friday"));

        let empty = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(jon, friday),
                text: " \t ",
            },
            person(),
        )
        .await;
        assert!(matches!(empty, Err(EditError::Empty)), "{empty:?}");
        let backwards = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(friday, jon),
                text: "x",
            },
            person(),
        )
        .await;
        assert!(
            matches!(backwards, Err(EditError::OutOfOrder)),
            "{backwards:?}"
        );
        let elsewhere = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(jon, word(&other_words, "Elsewhere")),
                text: "x",
            },
            person(),
        )
        .await;
        assert!(
            matches!(elsewhere, Err(EditError::NotFound)),
            "{elsewhere:?}"
        );
        let long = "word ".repeat(EDIT_TEXT_MAX_CHARACTERS / 5 + 1);
        let too_long = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(jon, jon),
                text: &long,
            },
            person(),
        )
        .await;
        assert!(matches!(too_long, Err(EditError::TooLong)), "{too_long:?}");
        assert_eq!(text(&mut connection, meeting_id).await, "Jon ships Friday");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_edit_with_no_room_renumbers_the_words_and_keeps_every_one_in_its_place() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) =
            built_meeting(&mut connection, &[("Ada", "one two three four")]).await;
        let meeting_id = capture.meeting_id;
        // Packed tight, as a meeting edited many times over in one place leaves its words.
        connection
            .batch_execute(&format!(
                "UPDATE meeting_words SET position = position / 1024 \
                 WHERE meeting_id = '{meeting_id}'"
            ))
            .await
            .expect("packs");
        let two = word(&words, "two").clone();
        let edited = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&two, &two),
                text: "2 and a half",
            },
            person(),
        )
        .await
        .expect("edits");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "one 2 and a half three four"
        );
        let positions: Vec<i64> = standing(&mut connection, meeting_id)
            .await
            .expect("reads")
            .iter()
            .map(|word| word.position)
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));

        history::undo(&mut connection, meeting_id, edited.id, person())
            .await
            .expect("undoes");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "one two three four"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_removed_takes_its_words_marks_and_history_with_it() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(&mut connection, &[("Ada", "Jon ships")]).await;
        let meeting_id = capture.meeting_id;
        let jon = word(&words, "Jon").clone();
        meeting_mark::add(
            &mut connection,
            meeting_id,
            MarkKind::Comment,
            range(&jon, &jon),
            json!({}),
            person(),
        )
        .await
        .expect("a comment");
        edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&jon, &jon),
                text: "John",
            },
            person(),
        )
        .await
        .expect("edits");

        diesel::delete(crate::database::schema::meetings::table.find(meeting_id))
            .execute(&mut connection)
            .await
            .expect("the meeting goes, with everything that hangs from it");
        assert!(!has_words(&mut connection, meeting_id).await.expect("reads"));
        assert!(
            meeting_event::history(&mut connection, meeting_id)
                .await
                .expect("reads")
                .is_empty()
        );
    }
}
