// Copyright © 2026 Jalapeno Labs

//! Comment threads on ranges of a meeting: starting one, replying, resolving and reopening it,
//! removing it, and what each sets off. See `docs/meetings.md`, Comments.
//!
//! A thread is a mark of kind `comment` over its range (`models::meeting_mark`), its data
//! `{ resolved, resolution }`: whether it is resolved, and the history entry that last resolved or
//! reopened it, which is the one an undo may reverse. Its comments are `models::meeting_comment`'s.
//! Every change takes the meeting's lock and writes, in one transaction, what it raises, queues,
//! and indexes; the caller announces what it set off once that commits
//! ([`Announcement::publish`]).
//!
//! # Who is told
//!
//! Each change raises an inbox item for each person it concerns, never for whoever made it: a
//! comment for everyone it mentions, and a reply, a resolve, or a reopen, an undo of either
//! included, for everyone else taking part in the thread, who wrote in it or was mentioned in it.
//! Machines are never told. Items are the whole workspace's until each person has a Next of their
//! own (`docs/action-items.md`, Ownership), so each names who it is for, and carries the meeting's
//! title and nothing written in the thread, as every item a meeting raises does.
//! `meeting_thread_items` keeps the thread that raised it, so the item's page opens the thread.
//!
//! # Elysia
//!
//! A person's comment that mentions Elysia queues a turn of hers (`elysia::trigger::MEETING_COMMENT`)
//! naming it. She answers in the thread, as Elysia, and may edit the meeting's words or put the
//! topics the range lies in on a project (`crate::elysia::dispatcher`, `tools::elysia`).
//!
//! # The index
//!
//! A meeting is in the knowledge index as its segments (`docs/meetings.md`, Segment records), so
//! a thread is indexed with each segment whose span its range overlaps (`knowledge::records`),
//! and goes wherever that segment goes: its projects' searches, and the agents reading it. Every
//! change here indexes again the segments the thread's range overlaps ([`reindex_over`]), and an
//! edit of words a thread quotes the segments holding that thread ([`reindex_quoting`]), in the
//! change's own transaction; a segment's text that came out
//! the same writes nothing.

use chrono::Utc;
use diesel::QueryResult;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::action_items::{Actor, WorkError};
use crate::elysia::trigger;
use crate::models::action_item::{
    self, ActionItem, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::agent::Author;
use crate::models::elysia::{self, NewTurn, Turn};
use crate::models::meeting::{self, Meeting};
use crate::models::meeting_comment::{self, BODY_MAX_CHARACTERS, WrittenComment};
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::meeting_mark::{self, Mark, MarkError, MarkKind, Range};
use crate::models::meeting_word::{self, EditError, Replacement};
use crate::models::timeline_record::{self, Attribution, TimelineRecord};
use crate::models::user::{self, ELYSIA_USER_ID, User};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_writes;
use crate::routes::v1::elysia::{TurnResponse, publish_snapshot};

/// The longest title of an inbox item, as items allow.
const ITEM_TITLE_MAX_CHARACTERS: usize = 200;

/// What a person writes: the comment, and whom it mentions by user id. Mentions are chosen, never
/// read out of the text, so a name written twice or a name two people share is never a guess.
#[derive(Debug, Clone, Copy)]
pub struct Writing<'text> {
    pub body: &'text str,
    pub mentions: &'text [Uuid],
}

/// Why a thread could not be written to or changed.
#[derive(Debug, thiserror::Error)]
pub enum CommentError {
    /// The meeting has no such thread.
    #[error("not found")]
    NotFound,
    /// The thread was removed.
    #[error("the thread was removed")]
    Removed,
    /// The comment says nothing.
    #[error("a comment says something")]
    Empty,
    /// The comment is longer than [`BODY_MAX_CHARACTERS`].
    #[error("a comment is at most {BODY_MAX_CHARACTERS} characters")]
    TooLong,
    /// A mention names nobody who can be mentioned: an active person, or Elysia.
    #[error("a mention names nobody who can be mentioned")]
    Unmentionable,
    /// The thread is resolved or open already.
    #[error("the thread is already so")]
    Unchanged,
    /// The range cannot be marked (`MarkError`).
    #[error(transparent)]
    Mark(MarkError),
    /// An inbox item it raises was refused, which an item in no project never is.
    #[error(transparent)]
    Item(#[from] WorkError),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// What a change wrote, and what it set off for its caller to announce once it commits.
#[derive(Debug)]
pub struct Outcome {
    /// The history entry it recorded: a thread made, removed, resolved, or reopened.
    pub entry: Option<MeetingEvent>,
    /// The comment it wrote.
    pub comment: Option<WrittenComment>,
    pub announcement: Announcement,
}

/// What a change set off that clients hear of ([`Announcement::publish`]).
#[derive(Debug, Default)]
pub struct Announcement {
    /// The inbox items it raised.
    raised: Vec<Recorded<ActionItem>>,
    /// The turn of Elysia's it queued.
    turn: Option<Turn>,
}

/// Why a person is told of a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Telling {
    Mentioned,
    Replied,
    Resolved,
    Reopened,
}

/// Starts a thread on `range` of `meeting` with `writing`, as `author`.
///
/// # Errors
/// [`CommentError::Empty`], [`CommentError::TooLong`], or [`CommentError::Unmentionable`] for
/// what was written, [`CommentError::Mark`] for a range that is not one of the transcript as it
/// reads; any database error.
pub async fn start(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    range: Range,
    writing: Writing<'_>,
    author: Author<'_>,
) -> Result<Outcome, CommentError> {
    let body = checked_body(writing.body)?;
    connection
        .transaction(async |connection| {
            let named = mentionable(connection, writing.mentions, author.user_id).await?;
            // The mark takes the meeting's lock, within this transaction.
            let (mark, entry) = meeting_mark::add(
                connection,
                meeting.id,
                MarkKind::Comment,
                range,
                json!({ "resolved": false }),
                author,
            )
            .await
            .map_err(|error| match error {
                MarkError::Database(error) => CommentError::Database(error),
                error => CommentError::Mark(error),
            })?;
            let written =
                meeting_comment::insert(connection, mark.id, body, &named.all, author).await?;
            let told: Vec<(Uuid, Telling)> = named
                .people
                .iter()
                .map(|person| (*person, Telling::Mentioned))
                .collect();
            let raised = tell(connection, meeting, mark.id, author.user_id, &told).await?;
            let turn = queue_elysia(connection, meeting, &written, named.author_is_person).await?;
            let span = span_of(connection, &mark).await?;
            reindex_over(connection, meeting, span).await?;
            Ok(Outcome {
                entry: Some(entry),
                comment: Some(written),
                announcement: Announcement { raised, turn },
            })
        })
        .await
}

/// Replies in thread `mark_id` of `meeting` with `writing`, as `author`. A resolved thread takes
/// replies too, so an answer under way when it was resolved still lands.
///
/// # Errors
/// [`CommentError::NotFound`] or [`CommentError::Removed`] for a thread that is not the
/// meeting's or stands no longer, and [`CommentError::Empty`], [`CommentError::TooLong`], or
/// [`CommentError::Unmentionable`] for what was written; any database error.
pub async fn reply(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    mark_id: Uuid,
    writing: Writing<'_>,
    author: Author<'_>,
) -> Result<Outcome, CommentError> {
    let body = checked_body(writing.body)?;
    connection
        .transaction(async |connection| {
            let mark = standing_thread(connection, meeting.id, mark_id).await?;
            let named = mentionable(connection, writing.mentions, author.user_id).await?;
            let taking_part = meeting_comment::participants(connection, mark.id).await?;
            let written =
                meeting_comment::insert(connection, mark.id, body, &named.all, author).await?;
            // Someone mentioned here hears of it once, as a mention.
            let mut told: Vec<(Uuid, Telling)> = named
                .people
                .iter()
                .map(|person| (*person, Telling::Mentioned))
                .collect();
            for participant in taking_part {
                if !named.all.contains(&participant) {
                    told.push((participant, Telling::Replied));
                }
            }
            let raised = tell(connection, meeting, mark.id, author.user_id, &told).await?;
            let turn = queue_elysia(connection, meeting, &written, named.author_is_person).await?;
            let span = span_of(connection, &mark).await?;
            reindex_over(connection, meeting, span).await?;
            Ok(Outcome {
                entry: None,
                comment: Some(written),
                announcement: Announcement { raised, turn },
            })
        })
        .await
}

/// Resolves thread `mark_id` of `meeting`, or opens it again when `resolved` is false, as
/// `author`, and records it.
///
/// # Errors
/// [`CommentError::NotFound`] or [`CommentError::Removed`] for a thread that is not the
/// meeting's or stands no longer, [`CommentError::Unchanged`] when it is so already; any database
/// error.
pub async fn set_resolved(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    mark_id: Uuid,
    resolved: bool,
    author: Author<'_>,
) -> Result<Outcome, CommentError> {
    connection
        .transaction(async |connection| {
            let mark = standing_thread(connection, meeting.id, mark_id).await?;
            if is_resolved(&mark) == resolved {
                return Err(CommentError::Unchanged);
            }
            let (entry, announcement) =
                resolve_and_tell(connection, meeting, &mark, resolved, author, None).await?;
            Ok(Outcome {
                entry: Some(entry),
                comment: None,
                announcement,
            })
        })
        .await
}

/// Removes thread `mark_id` of `meeting` as `author`, comments and all, taking it out of the
/// index; undoing the removal puts it back whole.
///
/// # Errors
/// [`CommentError::NotFound`] or [`CommentError::Removed`] for a thread that is not the
/// meeting's or stands no longer; any database error.
pub async fn remove(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    mark_id: Uuid,
    author: Author<'_>,
) -> Result<Outcome, CommentError> {
    connection
        .transaction(async |connection| {
            let mark = standing_thread(connection, meeting.id, mark_id).await?;
            let span = span_of(connection, &mark).await?;
            let entry = meeting_mark::remove(connection, meeting.id, mark_id, author)
                .await
                .map_err(|error| match error {
                    MarkError::Database(error) => CommentError::Database(error),
                    MarkError::Removed => CommentError::Removed,
                    _ => CommentError::NotFound,
                })?;
            reindex_over(connection, meeting, span).await?;
            Ok(Outcome {
                entry: Some(entry),
                comment: None,
                announcement: Announcement::default(),
            })
        })
        .await
}

/// Undoes `undone`, a `thread_resolved` or `thread_reopened` entry of `meeting`, as `author`: the
/// thread goes back to how it was, and everyone else taking part is told, as a resolve or a reopen
/// tells them, so nobody is left believing a change they heard of still stands. Answers `None`
/// when its thread was resolved or reopened again since, or removed, so undoing it would overwrite
/// that. The caller holds the meeting's lock.
///
/// # Errors
/// Propagates any database error, and an inbox item refused.
pub(crate) async fn undo_resolution(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> Result<Option<(MeetingEvent, Announcement)>, CommentError> {
    let Some(mark_id) = undone
        .data
        .get("markId")
        .and_then(Value::as_str)
        .and_then(|mark_id| mark_id.parse::<Uuid>().ok())
    else {
        // Only a history entry written by hand names no thread.
        tracing::event!(
            name: "meetings.history.unreadable",
            tracing::Level::WARN,
            meeting_event.id = %undone.id,
            "a thread's history entry names no thread; it cannot be undone",
        );
        return Ok(None);
    };
    let mark = match meeting_mark::find(connection, undone.meeting_id, mark_id).await {
        Ok(mark) => mark,
        Err(diesel::result::Error::NotFound) => {
            // Only a history entry written by hand names another meeting's mark.
            tracing::event!(
                name: "meetings.history.unreadable",
                tracing::Level::WARN,
                meeting_event.id = %undone.id,
                "a thread's history entry names no thread of its meeting; it cannot be undone",
            );
            return Ok(None);
        }
        Err(error) => return Err(CommentError::Database(error)),
    };
    if mark.removed_by.is_some() || resolution_of(&mark) != Some(undone.id) {
        return Ok(None);
    }
    let resolved = undone.kind() != Some(EventKind::ThreadResolved);
    let undo = resolve_and_tell(
        connection,
        meeting,
        &mark,
        resolved,
        author,
        Some(undone.id),
    )
    .await?;
    Ok(Some(undo))
}

/// Replaces a range of `meeting`'s words as `author` (`meeting_word::edit`), and indexes again,
/// in the same transaction, the segments holding a thread that quotes them.
///
/// # Errors
/// As `meeting_word::edit`; any database error.
pub async fn edit_words(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    replacement: Replacement<'_>,
    author: Author<'_>,
) -> Result<MeetingEvent, EditError> {
    connection
        .transaction(async |connection| {
            let entry = meeting_word::edit(connection, meeting.id, replacement, author).await?;
            // The replaced words stand no more but keep their times, which the new ones share.
            let ends = [
                replacement.range.start_word_id,
                replacement.range.end_word_id,
            ];
            if let Some(span) = words_span(connection, meeting.id, &ends).await? {
                reindex_quoting(connection, meeting, span).await?;
            }
            Ok(entry)
        })
        .await
}

/// Indexes again `meeting`'s segments over `span` of its clock, so each holds the threads over it
/// as they stand. Call it in the transaction that changed a thread there. A segment whose text
/// came out the same writes nothing.
///
/// # Errors
/// Propagates any database error.
pub async fn reindex_over(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    span: (i64, i64),
) -> QueryResult<()> {
    for (record, _) in segments_over(connection, meeting, span).await? {
        timeline_record::index(connection, &record).await?;
    }
    Ok(())
}

/// Indexes again the segments holding a thread that overlaps `span` of `meeting`'s clock, whose
/// words changed: a thread quotes them in every segment it lies in, which can reach past the span.
/// With no thread there, nothing is indexed, since a segment's own text never changes with an
/// edit.
///
/// # Errors
/// Propagates any database error.
pub async fn reindex_quoting(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    (start_ms, end_ms): (i64, i64),
) -> QueryResult<()> {
    let spans = meeting_comment::spans_over(connection, meeting.id, start_ms, end_ms).await?;
    let start = spans.iter().map(|(start, _)| *start).min();
    let end = spans.iter().map(|(_, end)| *end).max();
    let Some(covered) = start.zip(end) else {
        return Ok(());
    };
    reindex_over(connection, meeting, covered).await
}

/// Where words `word_ids` of meeting `meeting_id` lie on its clock, from the earliest start to the
/// latest end, standing or replaced; `None` when none of them is the meeting's.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn words_span(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    word_ids: &[Uuid],
) -> QueryResult<Option<(i64, i64)>> {
    let words = meeting_word::find_many(connection, meeting_id, word_ids).await?;
    let start = words.iter().map(|word| word.start_ms).min();
    let end = words.iter().map(|word| word.end_ms).max();
    Ok(start.zip(end).map(|(start, end)| (start, end.max(start))))
}

impl Announcement {
    /// Tells every client what the change set off, once its transaction committed: the meeting,
    /// whose page reads its transcript, history, and threads again, the items it raised, and
    /// Elysia's queue when it queued her a turn, which the engine is signalled to start.
    /// Everything was stored already, so a failure here is logged, and a client that missed it
    /// reads it on its next load.
    ///
    /// `entry_id` is the history entry a person's change from the page recorded, which that page
    /// applied already; `None` for a reply, which records none, and for anything an agent did,
    /// so every page reads it as news.
    pub async fn publish(
        self,
        events: &EventBus,
        connection: &mut AsyncPgConnection,
        meeting: &Meeting,
        entry_id: Option<Uuid>,
    ) {
        events.publish(&ServerEvent::MeetingChanged {
            id: meeting.id,
            state: meeting.state.clone(),
            created_by: meeting.created_by,
            entry_id,
        });
        if !self.raised.is_empty()
            && let Err(error) =
                publish_item_writes(events, connection, self.raised, &[], Utc::now()).await
        {
            tracing::event!(
                name: "meetings.comments.unpublished",
                tracing::Level::WARN,
                meeting.id = %meeting.id,
                error.message = %error,
                "a thread's inbox items were raised but could not be announced",
            );
        }
        let Some(turn) = self.turn else {
            return;
        };
        events.signal(&Signal::Elysia);
        events.publish(&ServerEvent::ElysiaTurnUpserted(TurnResponse::from(turn)));
        if let Err(error) = publish_snapshot(events, connection).await {
            tracing::event!(
                name: "meetings.comments.unpublished",
                tracing::Level::WARN,
                meeting.id = %meeting.id,
                error.message = %error,
                "Elysia's turn for a thread was queued but her queue could not be announced",
            );
        }
    }
}

/// Where thread `mark`'s range lies on its meeting's clock: from its first word's start to its
/// last word's end, in milliseconds.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when a word of its range is missing; any database error.
pub async fn span_of(connection: &mut AsyncPgConnection, mark: &Mark) -> QueryResult<(i64, i64)> {
    let ends = meeting_word::find_many(
        connection,
        mark.meeting_id,
        &[mark.start_word_id, mark.end_word_id],
    )
    .await?;
    let start = ends.iter().find(|word| word.id == mark.start_word_id);
    let end = ends.iter().find(|word| word.id == mark.end_word_id);
    match (start, end) {
        (Some(start), Some(end)) => Ok((start.start_ms, end.end_ms.max(start.start_ms))),
        _ => Err(diesel::result::Error::NotFound),
    }
}

/// `meeting`'s segments whose span overlaps `start_ms` to `end_ms`, each with what it is
/// attributed to, in the order they were said: the topics a thread there lies in.
///
/// # Errors
/// Propagates any database error.
pub async fn segments_over(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    (start_ms, end_ms): (i64, i64),
) -> QueryResult<Vec<(TimelineRecord, Vec<Attribution>)>> {
    let mut over: Vec<(TimelineRecord, Vec<Attribution>)> =
        timeline_record::of_meeting(connection, meeting.created_by, meeting.id)
            .await?
            .into_iter()
            // A range the time of one instant, a word with no time of its own, still lies in the
            // segment around it, as `meeting_comment::indexed_over` reads it.
            .filter(|(record, _)| {
                timeline_record::segment_span(record).is_some_and(
                    |(_, segment_start, segment_end)| {
                        segment_start < end_ms.max(start_ms + 1) && segment_end > start_ms
                    },
                )
            })
            .collect();
    over.sort_by_key(|(record, _)| record.occurred_at);
    Ok(over)
}

/// Whether thread `mark` is resolved.
pub fn is_resolved(mark: &Mark) -> bool {
    mark.data
        .get("resolved")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The history entry that last resolved or reopened thread `mark`, if one did.
fn resolution_of(mark: &Mark) -> Option<Uuid> {
    mark.data
        .get("resolution")
        .and_then(Value::as_str)
        .and_then(|entry_id| entry_id.parse().ok())
}

/// `body`, trimmed, when it says something and fits.
fn checked_body(body: &str) -> Result<&str, CommentError> {
    let body = body.trim();
    if body.is_empty() {
        return Err(CommentError::Empty);
    }
    if body.chars().count() > BODY_MAX_CHARACTERS {
        return Err(CommentError::TooLong);
    }
    Ok(body)
}

/// Meeting `meeting_id`'s thread `mark_id`, standing, under the meeting's lock.
async fn standing_thread(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    mark_id: Uuid,
) -> Result<Mark, CommentError> {
    match meeting::lock(connection, meeting_id).await {
        Ok(_) => {}
        Err(diesel::result::Error::NotFound) => return Err(CommentError::NotFound),
        Err(error) => return Err(CommentError::Database(error)),
    }
    let mark = match meeting_mark::find(connection, meeting_id, mark_id).await {
        Ok(mark) => mark,
        Err(diesel::result::Error::NotFound) => return Err(CommentError::NotFound),
        Err(error) => return Err(CommentError::Database(error)),
    };
    if mark.kind() != Some(MarkKind::Comment) {
        return Err(CommentError::NotFound);
    }
    if mark.removed_by.is_some() {
        return Err(CommentError::Removed);
    }
    Ok(mark)
}

/// Resolves thread `mark` of `meeting`, or opens it again, as `author` (undoing `undid`, when it is
/// an undo), records it, and tells everyone else taking part in the thread.
async fn resolve_and_tell(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    mark: &Mark,
    resolved: bool,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> Result<(MeetingEvent, Announcement), CommentError> {
    let entry = record_resolution(connection, mark, resolved, author, undid).await?;
    let telling = if resolved {
        Telling::Resolved
    } else {
        Telling::Reopened
    };
    let told: Vec<(Uuid, Telling)> = meeting_comment::participants(connection, mark.id)
        .await?
        .into_iter()
        .map(|person| (person, telling))
        .collect();
    let raised = tell(connection, meeting, mark.id, author.user_id, &told).await?;
    Ok((entry, Announcement { raised, turn: None }))
}

/// Records thread `mark` resolved, or opened again, as `author` (undoing `undid`, when it is an
/// undo), and keeps the entry on the thread as the one that last changed it.
async fn record_resolution(
    connection: &mut AsyncPgConnection,
    mark: &Mark,
    resolved: bool,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> QueryResult<MeetingEvent> {
    let kind = if resolved {
        EventKind::ThreadResolved
    } else {
        EventKind::ThreadReopened
    };
    let entry = meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id: mark.meeting_id,
            kind,
            author,
            data: json!({ "markId": mark.id }),
            undid,
        },
    )
    .await?;
    let mut data = mark.data.clone();
    if let Some(object) = data.as_object_mut() {
        object.insert("resolved".to_owned(), Value::Bool(resolved));
        object.insert("resolution".to_owned(), Value::String(entry.id.to_string()));
    }
    meeting_mark::set_data(connection, mark.id, &data).await?;
    Ok(entry)
}

/// Whom a comment may mention, and whom it tells.
struct Named {
    /// Every mention, each once, in the order written.
    all: Vec<Uuid>,
    /// The people among them other than the author, each told with an inbox item.
    people: Vec<Uuid>,
    /// Whether the author is a person, whose mention of Elysia she answers. A machine's never
    /// starts her, so she cannot set herself going.
    author_is_person: bool,
}

/// Checks that every one of `mentions` is an active person or Elysia.
async fn mentionable(
    connection: &mut AsyncPgConnection,
    mentions: &[Uuid],
    author_id: Uuid,
) -> Result<Named, CommentError> {
    let mut unique: Vec<Uuid> = Vec::with_capacity(mentions.len());
    for mention in mentions {
        if !unique.contains(mention) {
            unique.push(*mention);
        }
    }
    let mut looked_up = unique.clone();
    looked_up.push(author_id);
    let users: Vec<User> = user::find_many(connection, &looked_up).await?;
    let mut people = Vec::new();
    for mention in &unique {
        let found = users.iter().find(|candidate| candidate.id == *mention);
        match found {
            Some(person) if !person.is_machine() && person.is_active() => {
                if person.id != author_id {
                    people.push(person.id);
                }
            }
            Some(elysia) if elysia.id == ELYSIA_USER_ID => {}
            _ => return Err(CommentError::Unmentionable),
        }
    }
    let author_is_person = users
        .iter()
        .any(|candidate| candidate.id == author_id && !candidate.is_machine());
    Ok(Named {
        all: unique,
        people,
        author_is_person,
    })
}

/// Raises an inbox item for each person in `told` but `author_id`, saying why, for thread
/// `mark_id` of `meeting`. Answers the items raised.
async fn tell(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    mark_id: Uuid,
    author_id: Uuid,
    told: &[(Uuid, Telling)],
) -> Result<Vec<Recorded<ActionItem>>, CommentError> {
    let candidates: Vec<Uuid> = told.iter().map(|(person, _)| *person).collect();
    let people = user::find_many(connection, &candidates).await?;
    let author_name = user::find(connection, author_id).await?.name;
    let mut raised = Vec::new();
    for (person_id, telling) in told {
        if *person_id == author_id {
            continue;
        }
        let Some(person) = people
            .iter()
            .find(|candidate| candidate.id == *person_id && !candidate.is_machine())
        else {
            // Elysia and the other machines take part in threads without an inbox.
            continue;
        };
        let item = thread_item(meeting, &author_name, &person.name, *telling);
        let written = action_item::create(connection, item, Actor::System, Utc::now()).await?;
        meeting_comment::record_item(connection, written.record.id, mark_id, person.id, author_id)
            .await?;
        raised.push(written);
    }
    Ok(raised)
}

/// The inbox item telling `person` that `author` did something in a thread on `meeting`. It names
/// the meeting and nothing written in the thread, since items are the whole workspace's.
fn thread_item(meeting: &Meeting, author: &str, person: &str, telling: Telling) -> NewActionItem {
    let meeting_title = meeting.title.trim();
    let meeting_title = if meeting_title.is_empty() {
        "a meeting"
    } else {
        meeting_title
    };
    let title = match telling {
        Telling::Mentioned => format!("{author} mentioned {person} on {meeting_title}"),
        Telling::Replied => format!("{author} replied in a thread on {meeting_title}"),
        Telling::Resolved => format!("{author} resolved a thread on {meeting_title}"),
        Telling::Reopened => format!("{author} reopened a thread on {meeting_title}"),
    };
    NewActionItem {
        title: title.chars().take(ITEM_TITLE_MAX_CHARACTERS).collect(),
        notes: format!("For {person}."),
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    }
}

/// Queues Elysia's answer to `written` when a person wrote it and it mentions her.
async fn queue_elysia(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    written: &WrittenComment,
    author_is_person: bool,
) -> QueryResult<Option<Turn>> {
    if !author_is_person || !written.mentions.contains(&ELYSIA_USER_ID) {
        return Ok(None);
    }
    let turn = elysia::insert_turn(
        connection,
        NewTurn {
            id: Uuid::now_v7(),
            trigger_kind: trigger::MEETING_COMMENT.to_owned(),
            trigger_detail: json!({
                "meetingId": meeting.id,
                "markId": written.comment.mark_id,
                "commentId": written.comment.id,
            }),
            created_by: written.comment.created_by,
        },
    )
    .await?;
    Ok(Some(turn))
}

#[cfg(test)]
mod tests {
    use diesel::prelude::*;
    use diesel_async::RunQueryDsl;

    use super::*;
    use crate::database::schema::{knowledge_documents, knowledge_passages};
    use crate::meetings::SEGMENT_SOURCE_KIND;
    use crate::meetings::history::{self, UndoError};
    use crate::models::meeting_capture::MeetingCapture;
    use crate::models::meeting_word::Word;
    use crate::models::meeting_word::tests::{built_meeting, person, range, word};
    use crate::models::timeline_record::NewTimelineRecord;
    use crate::models::user::SYSTEM_USER_ID;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    const GRACE_ID: Uuid = uuid::uuid!("0199a3c4-0000-7000-8000-00000000a11c");

    /// Grace Hopper, an active member, and the meeting `lines` build, its owner the test person.
    async fn meeting_with_grace(
        connection: &mut AsyncPgConnection,
        lines: &[(&str, &str)],
    ) -> (Meeting, MeetingCapture, Vec<Word>) {
        diesel::sql_query(
            "INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) \
             VALUES ($1, gen_random_uuid(), 'grace@example.com', 'Grace Hopper', 'member', \
             'active', now())",
        )
        .bind::<diesel::sql_types::Uuid, _>(GRACE_ID)
        .execute(connection)
        .await
        .expect("Grace is created");
        let (capture, words) = built_meeting(connection, lines).await;
        let found = meeting::find(connection, capture.meeting_id)
            .await
            .expect("the meeting");
        (found, capture, words)
    }

    fn grace() -> Author<'static> {
        Author {
            user_id: GRACE_ID,
            agent: None,
            at: Utc::now(),
        }
    }

    fn writing<'text>(body: &'text str, mentions: &'text [Uuid]) -> Writing<'text> {
        Writing { body, mentions }
    }

    /// Who each item `announcement` raised is for, and its title.
    async fn told(
        connection: &mut AsyncPgConnection,
        announcement: &Announcement,
    ) -> Vec<(Uuid, String)> {
        let mut told = Vec::new();
        for raised in &announcement.raised {
            let person: Uuid = crate::database::schema::meeting_thread_items::table
                .find(raised.record.id)
                .select(crate::database::schema::meeting_thread_items::person_id)
                .first(connection)
                .await
                .expect("the item names its person");
            told.push((person, raised.record.title.clone()));
        }
        told
    }

    /// The text the knowledge index holds for timeline record `record_id`.
    async fn indexed_text(connection: &mut AsyncPgConnection, record_id: Uuid) -> String {
        let passages: Vec<String> = knowledge_passages::table
            .inner_join(knowledge_documents::table)
            .filter(knowledge_documents::timeline_record_id.eq(record_id))
            .order(knowledge_passages::position.asc())
            .select(knowledge_passages::content)
            .load(connection)
            .await
            .expect("reads the index");
        passages.join("\n")
    }

    /// A segment record of `meeting` from `start_ms` to `end_ms`, stored and indexed.
    async fn segment(
        connection: &mut AsyncPgConnection,
        meeting: &Meeting,
        (start_ms, end_ms): (i64, i64),
    ) -> Uuid {
        let record = NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: SEGMENT_SOURCE_KIND.to_owned(),
            connector_id: None,
            external_id: Some(format!("meeting:{}#{start_ms}", meeting.id)),
            occurred_at: meeting.started_at,
            actor: "Ada".to_owned(),
            title: "Launch date".to_owned(),
            body: "[0:00] Ada: Jon ships Friday".to_owned(),
            url: None,
            media: Some(json!({ "meetingId": meeting.id, "startMs": start_ms, "endMs": end_ms })),
            metadata: json!({ "meetingId": meeting.id, "summary": "The launch is Friday." }),
            created_by: meeting.created_by,
        };
        timeline_record::insert_new(connection, record)
            .await
            .expect("stores")
            .expect("a new record")
            .id
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn each_change_to_a_thread_tells_everyone_it_concerns_but_whoever_made_it() {
        let (_url, mut connection) = migrated_database().await;
        let (found, _, words) =
            meeting_with_grace(&mut connection, &[("Ada", "Jon ships Friday")]).await;
        let jon = word(&words, "Jon");
        let title = found.title.trim().to_owned();

        // Starting a thread tells whoever it mentions, and nobody else.
        let started = start(
            &mut connection,
            &found,
            range(jon, jon),
            writing("Is this John, @Grace?", &[GRACE_ID]),
            person(),
        )
        .await
        .expect("starts");
        let entry = started.entry.as_ref().expect("an entry");
        assert_eq!(entry.kind(), Some(EventKind::Marked));
        let mark_id = started.comment.as_ref().expect("a comment").comment.mark_id;
        assert_eq!(
            told(&mut connection, &started.announcement).await,
            vec![(
                GRACE_ID,
                format!("Tester mentioned Grace Hopper on {title}")
            )]
        );
        let raised_id = started.announcement.raised[0].record.id;
        assert_eq!(
            meeting_comment::thread_of_item(&mut connection, raised_id)
                .await
                .expect("reads"),
            Some((found.id, mark_id))
        );

        // A reply tells everyone else taking part, here the thread's author.
        let replied = reply(
            &mut connection,
            &found,
            mark_id,
            writing("It is.", &[]),
            grace(),
        )
        .await
        .expect("replies");
        assert!(replied.entry.is_none());
        assert_eq!(
            told(&mut connection, &replied.announcement).await,
            vec![(
                TEST_PERSON_ID,
                format!("Grace Hopper replied in a thread on {title}")
            )]
        );

        // Resolving tells the others, and resolving again changes nothing.
        let resolved = set_resolved(&mut connection, &found, mark_id, true, person())
            .await
            .expect("resolves");
        let resolution = resolved.entry.as_ref().expect("an entry").id;
        assert_eq!(
            told(&mut connection, &resolved.announcement).await,
            vec![(GRACE_ID, format!("Tester resolved a thread on {title}"))]
        );
        assert!(matches!(
            set_resolved(&mut connection, &found, mark_id, true, person()).await,
            Err(CommentError::Unchanged)
        ));
        let thread = meeting_mark::find(&mut connection, found.id, mark_id)
            .await
            .expect("the thread");
        assert!(is_resolved(&thread));

        // Undoing the resolve opens it again, telling Grace as a reopen does; undoing that undo
        // resolves it once more, and the first resolve, moved on since, stays.
        let undone = history::undo(&mut connection, found.id, resolution, person())
            .await
            .expect("undoes");
        assert_eq!(
            told(&mut connection, &undone.announcement).await,
            vec![(GRACE_ID, format!("Tester reopened a thread on {title}"))]
        );
        let reopened = undone.entry;
        assert_eq!(reopened.kind(), Some(EventKind::ThreadReopened));
        assert_eq!(reopened.undid(), Some(resolution));
        let thread = meeting_mark::find(&mut connection, found.id, mark_id)
            .await
            .expect("the thread");
        assert!(!is_resolved(&thread));
        assert!(matches!(
            history::undo(&mut connection, found.id, resolution, person()).await,
            Err(UndoError::ChangedSince)
        ));
        history::undo(&mut connection, found.id, reopened.id, person())
            .await
            .expect("undoes the undo");
        let thread = meeting_mark::find(&mut connection, found.id, mark_id)
            .await
            .expect("the thread");
        assert!(is_resolved(&thread));

        // Reopening by hand tells Grace too.
        let reopened = set_resolved(&mut connection, &found, mark_id, false, person())
            .await
            .expect("reopens");
        assert_eq!(
            told(&mut connection, &reopened.announcement).await,
            vec![(GRACE_ID, format!("Tester reopened a thread on {title}"))]
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_persons_mention_of_elysia_queues_her_and_nobody_else_can_be_mentioned() {
        let (_url, mut connection) = migrated_database().await;
        let (found, _, words) =
            meeting_with_grace(&mut connection, &[("Ada", "Sheriff ships Friday")]).await;
        let sheriff = word(&words, "Sheriff");

        let asked = start(
            &mut connection,
            &found,
            range(sheriff, sheriff),
            writing("@Elysia this was Sharath", &[ELYSIA_USER_ID]),
            person(),
        )
        .await
        .expect("starts");
        let comment = asked.comment.as_ref().expect("a comment").comment.clone();
        let turn = asked.announcement.turn.as_ref().expect("her turn");
        assert_eq!(turn.trigger_kind, trigger::MEETING_COMMENT);
        assert_eq!(
            turn.trigger_detail,
            json!({ "meetingId": found.id, "markId": comment.mark_id, "commentId": comment.id })
        );
        // Elysia takes part without an inbox.
        assert!(asked.announcement.raised.is_empty());

        // Her answer, which names her, never starts her again, and tells the thread's author.
        let elysia = Author {
            user_id: ELYSIA_USER_ID,
            agent: None,
            at: Utc::now(),
        };
        let answered = reply(
            &mut connection,
            &found,
            comment.mark_id,
            writing("Fixed: it reads Sharath now.", &[ELYSIA_USER_ID]),
            elysia,
        )
        .await
        .expect("answers");
        assert!(answered.announcement.turn.is_none());
        assert_eq!(answered.announcement.raised.len(), 1);

        // Someone mentioned in a reply, twice even, hears of it once, as a mention.
        let mentioned = reply(
            &mut connection,
            &found,
            comment.mark_id,
            writing("Grace, see above.", &[GRACE_ID, GRACE_ID]),
            person(),
        )
        .await
        .expect("replies");
        assert_eq!(
            mentioned.comment.as_ref().expect("a comment").mentions,
            vec![GRACE_ID]
        );
        assert_eq!(
            told(&mut connection, &mentioned.announcement).await,
            vec![(
                GRACE_ID,
                "Tester mentioned Grace Hopper on Weekly sync".to_owned()
            )]
        );

        for stranger in [SYSTEM_USER_ID, Uuid::now_v7()] {
            let refused = reply(
                &mut connection,
                &found,
                comment.mark_id,
                writing("Hello?", &[stranger]),
                person(),
            )
            .await;
            assert!(
                matches!(refused, Err(CommentError::Unmentionable)),
                "{refused:?}"
            );
        }
        for blank in ["", "  \n "] {
            let refused = reply(
                &mut connection,
                &found,
                comment.mark_id,
                writing(blank, &[]),
                person(),
            )
            .await;
            assert!(matches!(refused, Err(CommentError::Empty)), "{refused:?}");
        }
        let too_long = "a".repeat(BODY_MAX_CHARACTERS + 1);
        assert!(matches!(
            reply(
                &mut connection,
                &found,
                comment.mark_id,
                writing(&too_long, &[]),
                person()
            )
            .await,
            Err(CommentError::TooLong)
        ));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_thread_is_indexed_with_the_segment_its_range_lies_in_while_it_stands() {
        let (_url, mut connection) = migrated_database().await;
        let (found, _, words) = meeting_with_grace(
            &mut connection,
            &[("Ada", "Jon ships Friday"), ("Grace", "Then we celebrate")],
        )
        .await;
        let (jon, friday) = (word(&words, "Jon"), word(&words, "Friday"));

        // Written before the meeting is segmented, it is indexed as its segment is stored.
        let started = start(
            &mut connection,
            &found,
            range(jon, friday),
            writing("Friday is the launch, not the freeze.", &[]),
            person(),
        )
        .await
        .expect("starts");
        let mark_id = started.comment.expect("a comment").comment.mark_id;
        let first = segment(&mut connection, &found, (0, 2_000)).await;
        let second = segment(&mut connection, &found, (2_000, 4_000)).await;
        let expected =
            "Comment on \"Jon ships Friday\"\nTester: Friday is the launch, not the freeze.";
        assert!(
            indexed_text(&mut connection, first)
                .await
                .ends_with(expected)
        );
        assert!(
            !indexed_text(&mut connection, second)
                .await
                .contains("Comment on")
        );

        // A reply joins it.
        reply(
            &mut connection,
            &found,
            mark_id,
            writing("Agreed.", &[]),
            grace(),
        )
        .await
        .expect("replies");
        assert!(
            indexed_text(&mut connection, first)
                .await
                .ends_with("Grace Hopper: Agreed.")
        );

        // Removing the thread takes it out; undoing the removal puts it back.
        let removed = remove(&mut connection, &found, mark_id, person())
            .await
            .expect("removes");
        assert!(
            !indexed_text(&mut connection, first)
                .await
                .contains("Comment on")
        );
        assert!(matches!(
            reply(
                &mut connection,
                &found,
                mark_id,
                writing("Late.", &[]),
                grace()
            )
            .await,
            Err(CommentError::Removed)
        ));
        history::undo(
            &mut connection,
            found.id,
            removed.entry.expect("an entry").id,
            person(),
        )
        .await
        .expect("puts it back");
        assert!(
            indexed_text(&mut connection, first)
                .await
                .ends_with("Grace Hopper: Agreed.")
        );

        // An edit of the words it is on changes its quote.
        edit_words(
            &mut connection,
            &found,
            Replacement {
                range: range(jon, jon),
                text: "John",
            },
            person(),
        )
        .await
        .expect("edits");
        assert!(
            indexed_text(&mut connection, first)
                .await
                .contains("Comment on \"John ships Friday\"")
        );
    }
}
