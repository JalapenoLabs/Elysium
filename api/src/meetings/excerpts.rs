// Copyright © 2026 Jalapeno Labs

//! Excerpts and black-outs: what of a meeting is taken into projects, and what is left out of
//! everything. See `docs/meetings.md`, Excerpts and black-outs.
//!
//! # Excerpts
//!
//! An excerpt is a mark over a range of a meeting's words (`models::meeting_mark`, kind
//! `excerpt`) and the timeline record its projects read, which names its mark
//! (`timeline_records.excerpt_mark_id`). Its projects are its record's attributions
//! (`models::attribution`), so what a person decides about an excerpt is recorded, undoable, and
//! never overridden by Elysia, as for any record, and a record newly on a project wakes that
//! project's Project Manager. Segmenting makes Elysia's ([`take`]); a person assigns a range to a
//! project ([`assign`]) and removes an excerpt ([`remove`]). Removing one takes its record off
//! every project and out of the index, and its undo puts both back ([`undo`]).
//!
//! # Black-outs
//!
//! A black-out is a mark of kind `black_out` over words that matter to nobody. Segmenting makes
//! Elysia's; a person blacks a range out ([`black_out`]), which takes in every black-out it
//! overlaps or touches, so a stretch is blacked out once and growing one is blacking out the
//! larger range. Its words stay on the meeting's page and in its recording, and leave everything
//! indexed and everything an agent reads: an excerpt's text, the meeting's own document, and the
//! quote of a thread over them.
//!
//! # What an excerpt reads
//!
//! An excerpt's record reads its words as they stand, blacked-out ones left out ([`MeetingText`]),
//! as `[m:ss] Speaker: text` lines. An edit, a black-out, an undo of either, or lining the
//! transcript up again changes what it reads, so each refreshes the excerpts over what it changed
//! and indexes them again ([`refresh`]) in its own transaction. Its clip is cut again only when
//! the meeting is segmented again (`super::segment`).
//!
//! # Tasks
//!
//! Segmenting's answer names the tasks heard in a meeting too, taken last, in the same change: each
//! a mark of kind `task` and its row (`models::meeting_task`), sent where its share says
//! (`super::tasks`). A task read again keeps its mark, moved onto the words it was heard over, so
//! only a task that changed reaches anyone again.
//!
//! # The meeting in the index
//!
//! Beside its excerpts, a meeting segmented at least once is a document of its own in the
//! knowledge index: its whole transcript, blacked-out words left out, and its threads
//! ([`index_transcript`]), so what no excerpt took is still found.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::ops::{Range as Places, RangeInclusive};

use chrono::{Duration, Utc};
use diesel::QueryResult;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::SEGMENT_SOURCE_KIND;
use super::correction::lines_of;
use super::segmentation::{SegmentProject, clock};
use super::tasks::{self, Placed, Reach};
use crate::action_items::WorkError;
use crate::knowledge::{passages, records};
use crate::models::action_item::ActionItem;
use crate::models::action_item_event::Recorded;
use crate::models::agent::Author;
use crate::models::attribution::{self, Attributor};
use crate::models::knowledge_document::{self, IndexedDocument, KnowledgeSource};
use crate::models::meeting::{Meeting, MeetingState};
use crate::models::meeting_capture::{self, MeetingCapture};
use crate::models::meeting_comment;
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::meeting_mark::{self, Mark, MarkError, MarkKind, NewMark, Range};
use crate::models::meeting_task::{self, MeetingTask, TaskReading};
use crate::models::meeting_word::{self, Word};
use crate::models::project;
use crate::models::timeline_record::{self, ExcerptText, NewTimelineRecord, TimelineRecord};
use crate::models::user::{self, ELYSIA_USER_ID};

/// The longest `actor` a record takes: the excerpt's speakers.
const ACTOR_MAX_CHARACTERS: usize = 512;

/// How many words of a person's excerpt name it, which has no title of its own.
const OPENING_WORDS: usize = 12;

/// The version of how a meeting's whole transcript becomes a document ([`transcript_document`]).
/// Bump it whenever that changes what the document's text is, so the knowledge index's backfill
/// (`crate::knowledge::backfill`) indexes every segmented meeting again.
pub const TRANSCRIPT_INDEXED_AS: i32 = 1;

/// The most of a meeting's transcript its document holds, in characters: about eight hours of
/// speech, so every meeting is found whole, while one pathological transcript cannot hold the
/// embedder for long. An hour's meeting is some 60,000.
pub const TRANSCRIPT_MAX_CHARACTERS: usize = 500_000;

/// Why an excerpt or a black-out could not be made or removed.
#[derive(Debug, thiserror::Error)]
pub enum ExcerptError {
    /// The range, or the mark, cannot be marked or removed (`MarkError`).
    #[error(transparent)]
    Mark(#[from] MarkError),
    /// The mark is neither an excerpt nor a black-out; threads have routes of their own.
    #[error("the mark is neither an excerpt nor a black-out")]
    NotAnExcerpt,
    /// An excerpt names a project that does not exist.
    #[error("a project named does not exist")]
    UnknownProject,
    /// An excerpt names no project, which is no excerpt at all.
    #[error("an excerpt is on at least one project")]
    NoProject,
    /// Every word of the range is blacked out, so an excerpt of it would read nothing.
    #[error("every word of the range is blacked out")]
    BlackedOut,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// A meeting's final transcript as excerpts and the index read it: its standing words in order,
/// and which of them a standing black-out covers.
#[derive(Debug, Clone)]
pub struct MeetingText {
    words: Vec<Word>,
    /// Whether each word lies under a black-out.
    blacked: Vec<bool>,
    /// Each word's place in `words`, by its id.
    places: HashMap<Uuid, usize>,
}

/// What a run of a meeting's words reads as, blacked-out words left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// `[m:ss] Speaker: text` lines, without the time for a meeting in words alone.
    pub body: String,
    /// Who spoke, in the order they first spoke.
    pub speakers: Vec<String>,
    /// Its first words, which name a person's excerpt.
    pub opening: String,
    /// Its span on the meeting's clock, blacked-out words included, which its clip covers.
    pub start_ms: i64,
    pub end_ms: i64,
}

impl MeetingText {
    /// Meeting `meeting_id`'s final transcript as it reads, with its standing marks.
    ///
    /// # Errors
    /// Propagates any database error.
    pub async fn load(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> QueryResult<Self> {
        let words = meeting_word::standing(connection, meeting_id).await?;
        let marks = meeting_mark::standing(connection, meeting_id).await?;
        Ok(Self::new(words, &marks))
    }

    /// `words`, the standing words in order, under the black-outs among `marks`.
    pub fn new(words: Vec<Word>, marks: &[Mark]) -> Self {
        let places: HashMap<Uuid, usize> = words
            .iter()
            .enumerate()
            .map(|(place, word)| (word.id, place))
            .collect();
        let mut blacked = vec![false; words.len()];
        for mark in marks {
            if mark.kind() != Some(MarkKind::BlackOut) {
                continue;
            }
            let ends = places
                .get(&mark.start_word_id)
                .zip(places.get(&mark.end_word_id));
            let Some((&start, &end)) = ends else {
                // Marks move with every edit, so a standing mark is always over standing words.
                tracing::event!(
                    name: "meetings.black_out.unplaced",
                    tracing::Level::WARN,
                    meeting_mark.id = %mark.id,
                    "a standing black-out is not over standing words; its words are left as they are",
                );
                continue;
            };
            for covered in blacked.iter_mut().take(end + 1).skip(start) {
                *covered = true;
            }
        }
        Self {
            words,
            blacked,
            places,
        }
    }

    /// The standing words, in order.
    pub fn words(&self) -> &[Word] {
        &self.words
    }

    /// Whether the word at `place` lies under a black-out.
    pub fn is_blacked(&self, place: usize) -> bool {
        self.blacked.get(place).copied().unwrap_or(false)
    }

    /// Whether the meeting has a clock: a meeting in words alone has every word at `0`.
    pub fn is_timed(&self) -> bool {
        self.words
            .iter()
            .any(|word| word.start_ms != 0 || word.end_ms != 0)
    }

    /// The places of `range`'s first and last words, when both stand and run in order.
    pub fn places_of(&self, range: Range) -> Option<RangeInclusive<usize>> {
        let start = *self.places.get(&range.start_word_id)?;
        let end = *self.places.get(&range.end_word_id)?;
        (start <= end).then_some(start..=end)
    }

    /// The range from the word at the start of `places` to the word at its end.
    pub fn range_of(&self, places: &RangeInclusive<usize>) -> Range {
        Range {
            start_word_id: self.words[*places.start()].id,
            end_word_id: self.words[*places.end()].id,
        }
    }

    /// The transcript in lines, as correction reads it (`super::correction::lines_of`): a new line
    /// where the speaker changes, after a pause, and every so many words.
    pub fn lines(&self) -> Vec<Places<usize>> {
        lines_of(&self.words)
    }

    /// The words at `places` as an agent reads them: their text, with each run of blacked-out
    /// words in its place as `[blacked out]`, so the line is still there to be named and its words
    /// are not.
    pub fn agent_text(&self, places: Places<usize>) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for place in places {
            if !self.blacked[place] {
                parts.push(&self.words[place].text);
            } else if parts.last() != Some(&"[blacked out]") {
                parts.push("[blacked out]");
            }
        }
        parts.join(" ")
    }

    /// What the words at `places` read as, blacked-out words left out: each run between
    /// black-outs cut into lines as the transcript is, so no line reads across words left out.
    pub fn read(&self, places: RangeInclusive<usize>) -> Reading {
        let timed = self.is_timed();
        let first = &self.words[*places.start()];
        let start_ms = first.start_ms;
        let end_ms = self.words[places.clone()]
            .iter()
            .map(|word| word.end_ms)
            .max()
            .unwrap_or(start_ms)
            .max(start_ms);
        let mut reading = Reading {
            body: String::new(),
            speakers: Vec::new(),
            opening: String::new(),
            start_ms,
            end_ms,
        };
        let mut opening: Vec<&str> = Vec::new();
        let mut run: Vec<Word> = Vec::new();
        for place in places {
            if self.blacked[place] {
                push_lines(&mut reading, &run, timed);
                run.clear();
                continue;
            }
            let word = &self.words[place];
            if opening.len() <= OPENING_WORDS {
                opening.push(&word.text);
            }
            run.push(word.clone());
        }
        push_lines(&mut reading, &run, timed);
        reading.opening = if opening.len() > OPENING_WORDS {
            format!("{}…", opening[..OPENING_WORDS].join(" "))
        } else {
            opening.join(" ")
        };
        reading
    }
}

/// `run`'s words, none blacked out, as lines of `reading`'s body, each speaker heard named once.
fn push_lines(reading: &mut Reading, run: &[Word], timed: bool) {
    for line in lines_of(run) {
        let words = &run[line];
        let speaker = match words[0].speaker.trim() {
            "" => "Someone",
            named => named,
        };
        if speaker != "Someone" && !reading.speakers.iter().any(|known| known == speaker) {
            reading.speakers.push(speaker.to_owned());
        }
        let text = meeting_word::text_of(words);
        let _ = if timed {
            writeln!(
                reading.body,
                "[{}] {speaker}: {text}",
                clock(words[0].start_ms)
            )
        } else {
            writeln!(reading.body, "{speaker}: {text}")
        };
    }
}

/// What an excerpt is called and said: segmenting's answer, or a person's excerpt's first words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heading {
    pub title: String,
    pub summary: String,
}

impl Heading {
    /// A person's excerpt, named by its first words, which says nothing of its own.
    pub fn of_words(reading: &Reading) -> Self {
        Self {
            title: reading.opening.clone(),
            summary: String::new(),
        }
    }
}

/// The record of excerpt `mark_id` of `meeting`, reading `reading`, named by `heading`, from
/// `capture`, the meeting's first, whose connector and page it names.
pub fn new_record(
    meeting: &Meeting,
    capture: &MeetingCapture,
    mark_id: Uuid,
    reading: &Reading,
    heading: &Heading,
) -> NewTimelineRecord {
    NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: SEGMENT_SOURCE_KIND.to_owned(),
        connector_id: capture.connector_id,
        // One record per excerpt, whichever capture's connector it names.
        external_id: Some(format!("excerpt:{mark_id}")),
        occurred_at: meeting.started_at + Duration::milliseconds(reading.start_ms),
        actor: actor_of(reading),
        title: heading.title.clone(),
        body: reading.body.clone(),
        url: capture.url.clone(),
        media: Some(media_of(meeting.id, reading, None)),
        metadata: metadata_of(meeting, capture, heading),
        created_by: meeting.created_by,
    }
}

/// An excerpt record's metadata: its meeting, and the summary its document gives in place of who
/// spoke, since every line of its text names them.
pub fn metadata_of(meeting: &Meeting, capture: &MeetingCapture, heading: &Heading) -> Value {
    json!({
        "meetingId": meeting.id,
        "meetingTitle": meeting.title,
        "meetingSource": capture.source_kind,
        "summary": heading.summary,
    })
}

/// An excerpt record's media: its span on the meeting's clock and its clip, `null` until one is
/// cut.
pub fn media_of(meeting_id: Uuid, reading: &Reading, clip: Option<&Value>) -> Value {
    json!({
        "meetingId": meeting_id,
        "startMs": reading.start_ms,
        "endMs": reading.end_ms,
        "clip": clip,
    })
}

fn actor_of(reading: &Reading) -> String {
    reading
        .speakers
        .join(", ")
        .chars()
        .take(ACTOR_MAX_CHARACTERS)
        .collect()
}

/// What excerpt record `record` of `meeting` reads now, its words reading `reading`, keeping its
/// clip.
fn text_of(meeting: &Meeting, reading: &Reading, record: &TimelineRecord) -> ExcerptText {
    let clip = record
        .media
        .as_ref()
        .and_then(|media| media.get("clip"))
        .filter(|clip| !clip.is_null());
    ExcerptText {
        occurred_at: meeting.started_at + Duration::milliseconds(reading.start_ms),
        actor: actor_of(reading),
        body: reading.body.clone(),
        media: Some(media_of(meeting.id, reading, clip)),
    }
}

/// Whether `record` already reads `text`.
fn reads(record: &TimelineRecord, text: &ExcerptText) -> bool {
    record.occurred_at == text.occurred_at
        && record.actor == text.actor
        && record.body == text.body
        && record.media == text.media
}

/// Makes an excerpt over the words at `places` of `text`, `meeting`'s transcript as it reads, as
/// `author`: its mark and its record, named by `heading`, indexed. The caller holds the meeting's
/// lock, attributes the record, and records the change.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn make(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    capture: &MeetingCapture,
    text: &MeetingText,
    places: RangeInclusive<usize>,
    heading: &Heading,
    author: Author<'_>,
) -> QueryResult<(Mark, TimelineRecord)> {
    let range = text.range_of(&places);
    let new = NewMark::new(meeting.id, MarkKind::Excerpt, range, json!({}), author);
    let mark = meeting_mark::insert(connection, new).await?;
    let reading = text.read(places);
    let record = new_record(meeting, capture, mark.id, &reading, heading);
    let record = timeline_record::insert_excerpt(connection, record, mark.id).await?;
    Ok((mark, record))
}

/// Who an author attributes as: Elysia as herself, anyone else as the person they are.
fn attributor_of(author: Author<'_>) -> Attributor {
    if author.user_id == ELYSIA_USER_ID {
        return Attributor::Elysia;
    }
    Attributor::Person(author.user_id)
}

/// What assigning a range to projects made.
#[derive(Debug)]
pub struct Assigned {
    /// The history entry it recorded (`marked`).
    pub entry: MeetingEvent,
    /// The excerpt's record.
    pub record: TimelineRecord,
}

/// Takes `range` of `meeting` into `projects`, each with how sure the assignment is, as
/// `author`: a new excerpt over it, its record attributed to each project as the author (a
/// person's is certain), recorded as one change, so its undo removes the excerpt again.
///
/// # Errors
/// [`ExcerptError::NoProject`] for no project, [`ExcerptError::UnknownProject`] for one that does
/// not exist, [`ExcerptError::Mark`] for a range that is not one of the transcript as it reads,
/// [`ExcerptError::BlackedOut`] for one whose every word is blacked out; any database error.
pub async fn assign(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    range: Range,
    projects: &[SegmentProject],
    author: Author<'_>,
) -> Result<Assigned, ExcerptError> {
    if projects.is_empty() {
        return Err(ExcerptError::NoProject);
    }
    connection
        .transaction(async |connection| {
            meeting_mark::lock_meeting(connection, meeting.id).await?;
            meeting_mark::check_range(connection, meeting.id, range).await?;
            let existing: HashSet<Uuid> = project::list(connection)
                .await?
                .into_iter()
                .map(|known| known.id)
                .collect();
            if projects
                .iter()
                .any(|assigned| !existing.contains(&assigned.project_id))
            {
                return Err(ExcerptError::UnknownProject);
            }
            let text = MeetingText::load(connection, meeting.id).await?;
            let places = text.places_of(range).ok_or(MarkError::NotStanding)?;
            if places.clone().all(|place| text.is_blacked(place)) {
                return Err(ExcerptError::BlackedOut);
            }
            let capture = meeting_capture::first_of(connection, meeting.id).await?;
            let heading = Heading::of_words(&text.read(places.clone()));
            let (mark, record) = make(
                connection, meeting, &capture, &text, places, &heading, author,
            )
            .await?;
            let by = attributor_of(author);
            let mut attributed = Vec::new();
            for assigned in projects {
                let written = attribution::attribute(
                    connection,
                    record.id,
                    assigned.project_id,
                    assigned.confidence,
                    by,
                )
                .await?;
                attributed.extend(written.entry().map(|entry| entry.id));
            }
            let extra = json!({ "recordId": record.id, "attributed": attributed });
            let entry = meeting_mark::record_made(connection, &mark, author, None, extra).await?;
            Ok(Assigned { entry, record })
        })
        .await
}

/// Blacks `range` of `meeting` out as `author`, taking in every standing black-out it overlaps
/// or touches, recorded as one change, and refreshes what reads those words.
///
/// # Errors
/// [`ExcerptError::Mark`] for a range that is not one of the transcript as it reads; any database
/// error.
pub async fn black_out(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    range: Range,
    author: Author<'_>,
) -> Result<MeetingEvent, ExcerptError> {
    connection
        .transaction(async |connection| {
            meeting_mark::lock_meeting(connection, meeting.id).await?;
            meeting_mark::check_range(connection, meeting.id, range).await?;
            let words = meeting_word::standing(connection, meeting.id).await?;
            let marks = meeting_mark::standing(connection, meeting.id).await?;
            let text = MeetingText::new(words, &marks);
            let places = text.places_of(range).ok_or(MarkError::NotStanding)?;
            let mut others: Vec<(Uuid, RangeInclusive<usize>)> = marks
                .iter()
                .filter(|mark| mark.kind() == Some(MarkKind::BlackOut))
                .filter_map(|mark| text.places_of(mark.range()).map(|other| (mark.id, other)))
                .collect();
            let (mut start, mut end) = (*places.start(), *places.end());
            let mut absorbed: Vec<Uuid> = Vec::new();
            // Taking one in can bring another within reach on either side, so the stretch grows
            // until none is left that overlaps or touches it. Touching counts, so two black-outs
            // side by side become one.
            loop {
                let before = absorbed.len();
                others.retain(|(mark_id, other)| {
                    let reaches = *other.start() <= end + 1 && *other.end() + 1 >= start;
                    if reaches {
                        start = start.min(*other.start());
                        end = end.max(*other.end());
                        absorbed.push(*mark_id);
                    }
                    !reaches
                });
                if absorbed.len() == before {
                    break;
                }
            }
            let union = text.range_of(&(start..=end));
            let new = NewMark::new(meeting.id, MarkKind::BlackOut, union, json!({}), author);
            let mark = meeting_mark::insert(connection, new).await?;
            let extra = json!({ "absorbed": absorbed });
            let entry = meeting_mark::record_made(connection, &mark, author, None, extra).await?;
            meeting_mark::remove_all(connection, &absorbed, entry.id).await?;
            let span = text.read(start..=end);
            refresh(connection, meeting, Some((span.start_ms, span.end_ms))).await?;
            Ok(entry)
        })
        .await
}

/// Removes excerpt or black-out `mark_id` of `meeting` as `author`, recorded so its undo puts it
/// back: an excerpt's record leaves every project and the index, and a black-out's words read
/// again wherever they lie.
///
/// # Errors
/// [`ExcerptError::Mark`] when the meeting has no such mark or it was removed already,
/// [`ExcerptError::NotAnExcerpt`] for any other kind of mark; any database error.
pub async fn remove(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    mark_id: Uuid,
    author: Author<'_>,
) -> Result<MeetingEvent, ExcerptError> {
    connection
        .transaction(async |connection| {
            meeting_mark::lock_meeting(connection, meeting.id).await?;
            let mark = match meeting_mark::find(connection, meeting.id, mark_id).await {
                Ok(mark) => mark,
                Err(diesel::result::Error::NotFound) => return Err(MarkError::NotFound.into()),
                Err(error) => return Err(error.into()),
            };
            if mark.removed_by.is_some() {
                return Err(MarkError::Removed.into());
            }
            match mark.kind() {
                Some(MarkKind::Excerpt) => Ok(take_out(connection, &mark, author, None).await?),
                Some(MarkKind::BlackOut) => {
                    Ok(lift(connection, meeting, &mark, &[], author, None).await?)
                }
                _ => Err(ExcerptError::NotAnExcerpt),
            }
        })
        .await
}

/// Undoes `undone`, a `marked` or `unmarked` entry of an excerpt or a black-out of `meeting`, as
/// `author`. Answers `None` when the mark moved on since. The caller holds the meeting's lock.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn undo(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> QueryResult<Option<MeetingEvent>> {
    let Some(mark_id) = id_in(&undone.data, "markId") else {
        tracing::event!(
            name: "meetings.history.unreadable",
            tracing::Level::WARN,
            meeting_event.id = %undone.id,
            "a mark's history entry names no mark; it cannot be undone",
        );
        return Ok(None);
    };
    let mark = meeting_mark::find(connection, meeting.id, mark_id).await?;
    let standing = mark.removed_by.is_none();
    let removed_by_it = mark.removed_by == Some(undone.id);
    let undo = match (undone.kind(), mark.kind()) {
        (Some(EventKind::Marked), Some(MarkKind::Excerpt)) if standing => {
            Some(take_out(connection, &mark, author, Some(undone.id)).await?)
        }
        (Some(EventKind::Unmarked), Some(MarkKind::Excerpt)) if removed_by_it => {
            Some(put_back(connection, &mark, undone, author).await?)
        }
        (Some(EventKind::Marked), Some(MarkKind::BlackOut)) if standing => {
            let absorbed = ids_in(&undone.data, "absorbed");
            let restore = Restore {
                ids: &absorbed,
                removed_by: undone.id,
            };
            Some(
                lift(
                    connection,
                    meeting,
                    &mark,
                    &[restore],
                    author,
                    Some(undone.id),
                )
                .await?,
            )
        }
        (Some(EventKind::Unmarked), Some(MarkKind::BlackOut)) if removed_by_it => {
            Some(lay_again(connection, meeting, &mark, undone, author).await?)
        }
        _ => None,
    };
    Ok(undo)
}

/// Marks a change put back, as the change that removed them, for a black-out's removal to restore.
struct Restore<'ids> {
    ids: &'ids [Uuid],
    removed_by: Uuid,
}

/// Removes excerpt `mark`, undoing `undid` when it is an undo: its record leaves every project,
/// as `author`, and the index.
async fn take_out(
    connection: &mut AsyncPgConnection,
    mark: &Mark,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> QueryResult<MeetingEvent> {
    let record = timeline_record::of_excerpt(connection, mark.id).await?;
    let mut unattributed = Vec::new();
    if let Some(record) = &record {
        let by = attributor_of(author);
        for attribution in timeline_record::attributions_of(connection, &[record.id]).await? {
            let written =
                attribution::unattribute(connection, record.id, attribution.project_id, by).await?;
            unattributed.extend(written.entry().map(|entry| entry.id));
        }
        let source = KnowledgeSource::TimelineRecord {
            id: record.id,
            kind: record.source_kind.clone(),
        };
        knowledge_document::remove(connection, &source).await?;
    }
    let extra = json!({
        "recordId": record.as_ref().map(|record| record.id),
        "unattributed": unattributed,
    });
    let entry = meeting_mark::record_removed(connection, mark, author, undid, extra).await?;
    meeting_mark::remove_all(connection, &[mark.id], entry.id).await?;
    Ok(entry)
}

/// Puts excerpt `mark` back, undoing `undone`, its removal: its record goes back on each project
/// the removal took it off and has not been put on since, and back in the index.
async fn put_back(
    connection: &mut AsyncPgConnection,
    mark: &Mark,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> QueryResult<MeetingEvent> {
    meeting_mark::restore_all(connection, &[mark.id], undone.id).await?;
    let record = timeline_record::of_excerpt(connection, mark.id).await?;
    let mut attributed = Vec::new();
    if let Some(record) = &record {
        for entry_id in ids_in(&undone.data, "unattributed") {
            match attribution::undo(connection, record.id, entry_id, author.user_id).await {
                Ok(entry) => attributed.push(entry.id),
                // Someone put it on that project again since; theirs stands.
                Err(attribution::UndoError::ChangedSince | attribution::UndoError::NotFound) => {}
                Err(attribution::UndoError::Database(error)) => return Err(error),
            }
        }
        timeline_record::index(connection, record).await?;
    }
    let extra = json!({
        "recordId": record.as_ref().map(|record| record.id),
        "attributed": attributed,
    });
    meeting_mark::record_made(connection, mark, author, Some(undone.id), extra).await
}

/// Removes black-out `mark`, undoing `undid` when it is an undo, and puts back what each of
/// `giving_back` names that it removed: the black-outs a black-out took in. Refreshes what reads its
/// words.
async fn lift(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    mark: &Mark,
    giving_back: &[Restore<'_>],
    author: Author<'_>,
    undid: Option<Uuid>,
) -> QueryResult<MeetingEvent> {
    let mut put_back = Vec::new();
    for restore in giving_back {
        put_back
            .extend(meeting_mark::restore_all(connection, restore.ids, restore.removed_by).await?);
    }
    let extra = json!({ "restored": put_back });
    let entry = meeting_mark::record_removed(connection, mark, author, undid, extra).await?;
    meeting_mark::remove_all(connection, &[mark.id], entry.id).await?;
    refresh_over(connection, meeting, mark).await?;
    Ok(entry)
}

/// Puts black-out `mark` back, undoing `undone`, its removal, and removes again the black-outs
/// the removal put back that still stand.
async fn lay_again(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    mark: &Mark,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> QueryResult<MeetingEvent> {
    meeting_mark::restore_all(connection, &[mark.id], undone.id).await?;
    let restored = ids_in(&undone.data, "restored");
    let absorbed = meeting_mark::standing_among(connection, &restored).await?;
    let extra = json!({ "absorbed": absorbed });
    let entry = meeting_mark::record_made(connection, mark, author, Some(undone.id), extra).await?;
    meeting_mark::remove_all(connection, &absorbed, entry.id).await?;
    refresh_over(connection, meeting, mark).await?;
    Ok(entry)
}

/// Refreshes what reads the words under `mark`'s range.
async fn refresh_over(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    mark: &Mark,
) -> QueryResult<()> {
    let ends = [mark.start_word_id, mark.end_word_id];
    let words = meeting_word::find_many(connection, meeting.id, &ends).await?;
    let start = words.iter().map(|word| word.start_ms).min();
    let end = words.iter().map(|word| word.end_ms).max();
    let span = start.zip(end).map(|(start, end)| (start, end.max(start)));
    refresh(connection, meeting, span).await
}

/// Gives every standing excerpt of `meeting` over `span` of its clock (or every one, for `None`)
/// what its words read now, and indexes it again with the threads over it; then indexes the
/// meeting's whole transcript again ([`index_transcript`]). Call it in the transaction that
/// changed its words, its black-outs, or its threads. What reads the same writes nothing.
///
/// # Errors
/// Propagates any database error.
pub async fn refresh(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    span: Option<(i64, i64)>,
) -> QueryResult<()> {
    let words = meeting_word::standing(connection, meeting.id).await?;
    let marks = meeting_mark::standing(connection, meeting.id).await?;
    let text = MeetingText::new(words, &marks);
    let excerpts: HashMap<Uuid, &Mark> = marks
        .iter()
        .filter(|mark| mark.kind() == Some(MarkKind::Excerpt))
        .map(|mark| (mark.id, mark))
        .collect();
    let mark_ids: Vec<Uuid> = excerpts.keys().copied().collect();
    for record in timeline_record::of_excerpts(connection, &mark_ids).await? {
        let Some(mark) = record.excerpt_mark_id.and_then(|id| excerpts.get(&id)) else {
            continue;
        };
        let Some(places) = text.places_of(mark.range()) else {
            tracing::event!(
                name: "meetings.excerpt.unplaced",
                tracing::Level::WARN,
                meeting_mark.id = %mark.id,
                "a standing excerpt is not over standing words; its record is left as it reads",
            );
            continue;
        };
        let reading = text.read(places);
        let was = timeline_record::segment_span(&record).map(|(_, start, end)| (start, end));
        let touched = span.is_none_or(|span| {
            overlaps(span, (reading.start_ms, reading.end_ms))
                || was.is_some_and(|was| overlaps(span, was))
        });
        if !touched {
            continue;
        }
        let updated = text_of(meeting, &reading, &record);
        let record = if reads(&record, &updated) {
            record
        } else {
            timeline_record::set_excerpt_text(connection, record.id, &updated).await?
        };
        timeline_record::index(connection, &record).await?;
    }
    index_transcript(connection, meeting, &text).await
}

/// Whether two spans of the meeting's clock share a moment; a span of one instant is a
/// millisecond long, as a word with no time of its own is where it lies.
fn overlaps((start, end): (i64, i64), (other_start, other_end): (i64, i64)) -> bool {
    start < other_end.max(other_start + 1) && other_start < end.max(start + 1)
}

/// Puts `meeting`'s whole transcript, as `text` reads it, in the knowledge index with its threads,
/// once segmenting published it, or takes it out once it is merged into another meeting. A
/// meeting being segmented again keeps the document it had until it is published again.
///
/// # Errors
/// Propagates any database error.
pub async fn index_transcript(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    text: &MeetingText,
) -> QueryResult<()> {
    let source = KnowledgeSource::Meeting { id: meeting.id };
    if meeting.merged_into.is_some() {
        return knowledge_document::remove(connection, &source).await;
    }
    if meeting.state() != MeetingState::Done {
        return Ok(());
    }
    let threads = meeting_comment::indexed_over(connection, meeting.id, i64::MIN, i64::MAX).await?;
    match transcript_document(meeting, text, &threads) {
        Some(document) => knowledge_document::upsert(connection, &document)
            .await
            .map(drop),
        None => knowledge_document::remove(connection, &source).await,
    }
}

/// The document `meeting`'s whole transcript is indexed as: its title, then every line of `text`,
/// blacked-out words left out, each its own paragraph so an edit moves as few passages as it can,
/// then `threads`. `None` when nothing in it is left to search.
pub fn transcript_document(
    meeting: &Meeting,
    text: &MeetingText,
    threads: &[meeting_comment::IndexedThread],
) -> Option<IndexedDocument> {
    let body = if text.words().is_empty() {
        String::new()
    } else {
        text.read(0..=text.words().len() - 1).body
    };
    let body: String = body.chars().take(TRANSCRIPT_MAX_CHARACTERS).collect();
    if body.trim().is_empty() && threads.is_empty() {
        return None;
    }
    let mut paragraphs: Vec<String> = vec![meeting.title.clone()];
    paragraphs.extend(body.lines().map(ToOwned::to_owned));
    paragraphs.extend(records::thread_paragraphs(threads));
    let passages = passages::split(&paragraphs.join("\n\n"));
    if passages.is_empty() {
        return None;
    }
    Some(IndexedDocument {
        source: KnowledgeSource::Meeting { id: meeting.id },
        title: meeting.title.clone(),
        url: None,
        occurred_at: meeting.started_at,
        created_by: meeting.created_by,
        // Its readers alone find it (`docs/meetings.md`, Who reads a meeting).
        access_meeting_id: Some(meeting.id),
        passages,
    })
}

/// What segmenting took from a meeting, as its answer is stored: each excerpt, black-out, and task
/// as a range of the words it read, so an attempt that finds it stored takes the same words.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Taken {
    pub excerpts: Vec<TakenExcerpt>,
    pub black_outs: Vec<Range>,
    /// Missing from an answer stored before tasks were read, which takes none.
    #[serde(default)]
    pub tasks: Vec<TakenTask>,
}

/// One task segmenting heard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakenTask {
    pub range: Range,
    #[serde(flatten)]
    pub reading: TaskReading,
}

/// One excerpt segmenting took.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakenExcerpt {
    pub range: Range,
    #[serde(flatten)]
    pub heading: Heading,
    pub projects: Vec<SegmentProject>,
}

/// What [`take`] did.
#[derive(Debug)]
pub struct Took {
    /// Its history entry (`segmented`).
    pub entry: MeetingEvent,
    /// The excerpts' records it made.
    pub created: Vec<TimelineRecord>,
    /// The inbox items its tasks raised at once (`super::tasks`).
    pub raised: Vec<Recorded<ActionItem>>,
}

/// A task the answer names, to be taken: its range, the places of its words in the transcript as
/// it reads, and what it is.
struct HeardTask {
    range: Range,
    places: RangeInclusive<usize>,
    reading: TaskReading,
}

/// One of Elysia's own tasks standing as segmenting began, which the answer may read again.
struct StandingTask {
    mark_id: Uuid,
    places: RangeInclusive<usize>,
    reading: TaskReading,
}

/// A meeting's marks as segmenting finds them, and what taking its answer did so far.
struct Taking<'meeting> {
    meeting: &'meeting Meeting,
    capture: &'meeting MeetingCapture,
    author: Author<'static>,
    /// Every project now, so an attribution to one removed while the turn ran is left out.
    projects: HashSet<Uuid>,
    words: Vec<Word>,
    /// Every mark standing as segmenting began.
    marks: Vec<Mark>,
    /// Each standing excerpt's record, by its mark.
    records: HashMap<Uuid, TimelineRecord>,
    /// The excerpts' records whose projects a person changed, which makes the excerpts theirs.
    changed: HashSet<Uuid>,
    /// The ranges a person removed an excerpt, a black-out, or a task from, which segmenting
    /// leaves be.
    removed_excerpts: HashSet<Range>,
    removed_black_outs: HashSet<Range>,
    removed_tasks: HashSet<Range>,
    /// Each standing task, by its mark.
    tasks: HashMap<Uuid, MeetingTask>,
    /// Every user's name, for whom a task's item is.
    names: HashMap<Uuid, String>,
    /// The marks it made, Elysia's own it kept, and every standing mark the answer took again.
    made: Vec<Uuid>,
    kept: Vec<Uuid>,
    taken_again: HashSet<Uuid>,
    /// Elysia's own tasks it moved onto the words they were heard over again.
    moved: Vec<Uuid>,
    /// How many ranges of the answer no longer stood.
    unanchored: usize,
    created: Vec<TimelineRecord>,
    raised: Vec<Recorded<ActionItem>>,
}

impl<'meeting> Taking<'meeting> {
    async fn read(
        connection: &mut AsyncPgConnection,
        meeting: &'meeting Meeting,
        capture: &'meeting MeetingCapture,
    ) -> QueryResult<Self> {
        let projects = project::list(connection)
            .await?
            .into_iter()
            .map(|known| known.id)
            .collect();
        let words = meeting_word::standing(connection, meeting.id).await?;
        let marks = meeting_mark::standing(connection, meeting.id).await?;
        let excerpt_ids: Vec<Uuid> = marks
            .iter()
            .filter(|mark| mark.kind() == Some(MarkKind::Excerpt))
            .map(|mark| mark.id)
            .collect();
        let records: HashMap<Uuid, TimelineRecord> =
            timeline_record::of_excerpts(connection, &excerpt_ids)
                .await?
                .into_iter()
                .filter_map(|record| record.excerpt_mark_id.map(|mark_id| (mark_id, record)))
                .collect();
        let record_ids: Vec<Uuid> = records.values().map(|record| record.id).collect();
        let changed = attribution::changed_by_people(connection, &record_ids).await?;
        let removed_excerpts =
            meeting_mark::removed_by_people(connection, meeting.id, MarkKind::Excerpt).await?;
        let removed_black_outs =
            meeting_mark::removed_by_people(connection, meeting.id, MarkKind::BlackOut).await?;
        let removed_tasks =
            meeting_mark::removed_by_people(connection, meeting.id, MarkKind::Task).await?;
        let tasks = meeting_task::standing_of(connection, meeting.id).await?;
        let names = user::names(connection).await?;
        Ok(Self {
            meeting,
            capture,
            author: Author {
                user_id: ELYSIA_USER_ID,
                agent: None,
                at: Utc::now(),
            },
            projects,
            words,
            marks,
            records,
            changed,
            removed_excerpts: removed_excerpts.iter().map(Mark::range).collect(),
            removed_black_outs: removed_black_outs.iter().map(Mark::range).collect(),
            removed_tasks: removed_tasks.iter().map(Mark::range).collect(),
            tasks,
            names,
            made: Vec::new(),
            kept: Vec::new(),
            taken_again: HashSet::new(),
            moved: Vec::new(),
            unanchored: 0,
            created: Vec::new(),
            raised: Vec::new(),
        })
    }

    /// Whether `mark` is a person's: made by one, or an excerpt whose projects one changed.
    fn is_persons(&self, mark: &Mark) -> bool {
        mark.created_by != ELYSIA_USER_ID
            || self
                .records
                .get(&mark.id)
                .is_some_and(|record| self.changed.contains(&record.id))
    }

    /// The standing mark of `kind` over exactly `range`, with whether it is a person's.
    fn standing(&self, kind: MarkKind, range: Range) -> Option<(Uuid, bool)> {
        self.marks
            .iter()
            .find(|mark| mark.kind() == Some(kind) && mark.range() == range)
            .map(|mark| (mark.id, self.is_persons(mark)))
    }

    /// Black-outs first, so every excerpt made reads its words without them: each range a person
    /// removed one from stays clear, one standing already is kept, and none covers a word of a
    /// person's excerpt.
    async fn take_black_outs(
        &mut self,
        connection: &mut AsyncPgConnection,
        ranges: &[Range],
    ) -> QueryResult<()> {
        let text = MeetingText::new(self.words.clone(), &self.marks);
        let persons_places: Vec<RangeInclusive<usize>> = self
            .marks
            .iter()
            .filter(|mark| mark.kind() == Some(MarkKind::Excerpt) && self.is_persons(mark))
            .filter_map(|mark| text.places_of(mark.range()))
            .collect();
        for range in ranges {
            let Some(places) = text.places_of(*range) else {
                self.unanchored += 1;
                continue;
            };
            if self.removed_black_outs.contains(range) {
                continue;
            }
            if let Some((mark_id, is_persons)) = self.standing(MarkKind::BlackOut, *range) {
                self.taken_again.insert(mark_id);
                if !is_persons {
                    self.kept.push(mark_id);
                }
                continue;
            }
            let covers_persons = persons_places
                .iter()
                .any(|persons| persons.start() <= places.end() && places.start() <= persons.end());
            if covers_persons {
                continue;
            }
            let new = NewMark::new(
                self.meeting.id,
                MarkKind::BlackOut,
                *range,
                json!({}),
                self.author,
            );
            self.made
                .push(meeting_mark::insert(connection, new).await?.id);
        }
        Ok(())
    }

    /// Excerpts: each range a person removed one from stays out, a person's over the same words
    /// stands as it is, Elysia's own over them takes the answer's title, summary, and projects,
    /// and every other is made.
    async fn take_excerpts(
        &mut self,
        connection: &mut AsyncPgConnection,
        excerpts: &[TakenExcerpt],
    ) -> QueryResult<()> {
        let marks_now = meeting_mark::standing(connection, self.meeting.id).await?;
        let text = MeetingText::new(self.words.clone(), &marks_now);
        for excerpt in excerpts {
            let Some(places) = text.places_of(excerpt.range) else {
                self.unanchored += 1;
                continue;
            };
            if self.removed_excerpts.contains(&excerpt.range) {
                continue;
            }
            if places.clone().all(|place| text.is_blacked(place)) {
                tracing::event!(
                    name: "meetings.segment.excerpt_blacked_out",
                    tracing::Level::DEBUG,
                    meeting.id = %self.meeting.id,
                    "an excerpt the answer named lies wholly in black-outs, so it was left out",
                );
                continue;
            }
            if let Some((mark_id, is_persons)) = self.standing(MarkKind::Excerpt, excerpt.range) {
                self.taken_again.insert(mark_id);
                if !is_persons {
                    self.kept.push(mark_id);
                    self.take_again(connection, mark_id, excerpt).await?;
                }
                continue;
            }
            let (mark, record) = make(
                connection,
                self.meeting,
                self.capture,
                &text,
                places,
                &excerpt.heading,
                self.author,
            )
            .await?;
            attribute_as_elysia(connection, &record, &excerpt.projects, &self.projects).await?;
            self.made.push(mark.id);
            self.created.push(record);
        }
        Ok(())
    }

    /// Gives Elysia's own excerpt `mark_id` what the answer says of it again.
    async fn take_again(
        &self,
        connection: &mut AsyncPgConnection,
        mark_id: Uuid,
        excerpt: &TakenExcerpt,
    ) -> QueryResult<()> {
        let Some(record) = self.records.get(&mark_id) else {
            return Ok(());
        };
        let metadata = metadata_of(self.meeting, self.capture, &excerpt.heading);
        let record =
            timeline_record::set_heading(connection, record.id, &excerpt.heading.title, &metadata)
                .await?;
        attribute_as_elysia(connection, &record, &excerpt.projects, &self.projects).await
    }

    /// Whether `mark`, standing as segmenting began, still stands once the answer is taken: a
    /// person's, one the answer took again, or one it made. Elysia's others give way in
    /// [`Self::finish`].
    fn stays(&self, mark: &Mark) -> bool {
        self.is_persons(mark) || self.taken_again.contains(&mark.id) || self.made.contains(&mark.id)
    }

    /// Tasks, after excerpts and black-outs: each range a person removed one from stays clear, a
    /// person's over the same words stands, and one wholly in black-outs is no task. Every other
    /// the answer names is one of Elysia's read again (`super::tasks::read_again`), moved onto the
    /// words it was heard over and given what the answer says of it, or a new one. A new task, or
    /// one that changed, goes where its share says (`super::tasks::send`); one read again as it was
    /// reaches nobody, so a meeting read again wakes no Project Manager and raises no item twice.
    async fn take_tasks(
        &mut self,
        connection: &mut AsyncPgConnection,
        answered: &[TakenTask],
    ) -> Result<(), WorkError> {
        // Elysia's black-outs the answer gave up are on their way out, so they blank nothing.
        let marks_now: Vec<Mark> = meeting_mark::standing(connection, self.meeting.id)
            .await?
            .into_iter()
            .filter(|mark| mark.kind() != Some(MarkKind::BlackOut) || self.stays(mark))
            .collect();
        let text = MeetingText::new(self.words.clone(), &marks_now);
        let heard = self.heard_tasks(&text, answered);
        // Elysia's own standing tasks, which the answer may read again.
        let before: Vec<StandingTask> = self
            .marks
            .iter()
            .filter(|mark| mark.kind() == Some(MarkKind::Task) && !self.is_persons(mark))
            .filter_map(|mark| {
                Some(StandingTask {
                    mark_id: mark.id,
                    places: text.places_of(mark.range())?,
                    reading: self.tasks.get(&mark.id)?.reading(),
                })
            })
            .collect();
        let matches = {
            let heard_placed: Vec<Placed<'_>> = heard
                .iter()
                .map(|task| Placed {
                    places: &task.places,
                    reading: &task.reading,
                })
                .collect();
            let before_placed: Vec<Placed<'_>> = before
                .iter()
                .map(|task| Placed {
                    places: &task.places,
                    reading: &task.reading,
                })
                .collect();
            tasks::read_again(&heard_placed, &before_placed)
        };
        for (task, matched) in heard.iter().zip(matches) {
            let changed = if let Some(index) = matched {
                self.take_task_again(connection, &before[index], task)
                    .await?
            } else {
                Some(self.make_task(connection, task).await?)
            };
            let Some(changed) = changed else {
                continue;
            };
            let reach = Reach {
                meeting: self.meeting,
                names: &self.names,
                timed: text.is_timed(),
            };
            let start_ms = text.words()[*task.places.start()].start_ms;
            if let Some(raised) = tasks::send(connection, reach, &changed, start_ms).await? {
                self.raised.push(raised);
            }
        }
        Ok(())
    }

    /// The tasks `answered` names that are to be taken, by the places of their words in `text`:
    /// none over a range a person removed one from or holds one over, none wholly blacked out, and
    /// none on a project removed while the turn ran, which is then no project's.
    fn heard_tasks(&mut self, text: &MeetingText, answered: &[TakenTask]) -> Vec<HeardTask> {
        let mut heard = Vec::with_capacity(answered.len());
        for task in answered {
            let Some(places) = text.places_of(task.range) else {
                self.unanchored += 1;
                continue;
            };
            let persons_over_it = self.marks.iter().any(|mark| {
                mark.kind() == Some(MarkKind::Task)
                    && self.is_persons(mark)
                    && mark.range() == task.range
            });
            if self.removed_tasks.contains(&task.range) || persons_over_it {
                continue;
            }
            if places.clone().all(|place| text.is_blacked(place)) {
                tracing::event!(
                    name: "meetings.segment.task_blacked_out",
                    tracing::Level::DEBUG,
                    meeting.id = %self.meeting.id,
                    "a task the answer named lies wholly in black-outs, so it was left out",
                );
                continue;
            }
            let mut reading = task.reading.clone();
            if reading
                .project_id
                .is_some_and(|project_id| !self.projects.contains(&project_id))
            {
                reading.project_id = None;
            }
            heard.push(HeardTask {
                range: task.range,
                places,
                reading,
            });
        }
        heard
    }

    /// Takes Elysia's own task `before` again as `heard`: moved onto its words when they moved, and
    /// given what the answer says of it. Answers the task when it changed, which then reaches
    /// whoever it is for again, and `None` when it reads as it did.
    async fn take_task_again(
        &mut self,
        connection: &mut AsyncPgConnection,
        before: &StandingTask,
        heard: &HeardTask,
    ) -> Result<Option<MeetingTask>, WorkError> {
        self.taken_again.insert(before.mark_id);
        self.kept.push(before.mark_id);
        if before.places != heard.places {
            meeting_mark::set_range(connection, before.mark_id, heard.range).await?;
            self.moved.push(before.mark_id);
        }
        if before.reading == heard.reading {
            return Ok(None);
        }
        let task = meeting_task::set_reading(connection, before.mark_id, &heard.reading).await?;
        Ok(tasks::changed(&before.reading, &heard.reading).then_some(task))
    }

    /// Makes `heard` a new task of Elysia's: its mark and its row.
    async fn make_task(
        &mut self,
        connection: &mut AsyncPgConnection,
        heard: &HeardTask,
    ) -> Result<MeetingTask, WorkError> {
        let new = NewMark::new(
            self.meeting.id,
            MarkKind::Task,
            heard.range,
            json!({}),
            self.author,
        );
        let mark = meeting_mark::insert(connection, new).await?;
        self.made.push(mark.id);
        let task = meeting_task::insert(
            connection,
            mark.id,
            self.meeting.id,
            &heard.reading,
            self.author.user_id,
        )
        .await?;
        Ok(task)
    }

    /// Records what it did as one change, and removes Elysia's own marks the answer did not take
    /// again: an excerpt's record leaves her projects and the index. A task removed keeps the items
    /// made of it, which are their people's to resolve or dismiss.
    async fn finish(self, connection: &mut AsyncPgConnection) -> QueryResult<Took> {
        let removed: Vec<Uuid> = self
            .marks
            .iter()
            .filter(|mark| {
                let replaceable = matches!(
                    mark.kind(),
                    Some(MarkKind::Excerpt | MarkKind::BlackOut | MarkKind::Task)
                );
                replaceable && !self.taken_again.contains(&mark.id) && !self.is_persons(mark)
            })
            .map(|mark| mark.id)
            .collect();
        let entry = meeting_event::record(
            connection,
            Change {
                id: Uuid::now_v7(),
                meeting_id: self.meeting.id,
                kind: EventKind::Segmented,
                author: self.author,
                data: json!({
                    "made": self.made,
                    "kept": self.kept,
                    "moved": self.moved,
                    "removed": removed,
                    "unanchored": self.unanchored,
                }),
                undid: None,
            },
        )
        .await?;
        meeting_mark::remove_all(connection, &removed, entry.id).await?;
        for mark_id in &removed {
            let Some(record) = self.records.get(mark_id) else {
                continue;
            };
            for attribution in timeline_record::attributions_of(connection, &[record.id]).await? {
                attribution::unattribute(
                    connection,
                    record.id,
                    attribution.project_id,
                    Attributor::Elysia,
                )
                .await?;
            }
            let source = KnowledgeSource::TimelineRecord {
                id: record.id,
                kind: record.source_kind.clone(),
            };
            knowledge_document::remove(connection, &source).await?;
        }
        Ok(Took {
            entry,
            created: self.created,
            raised: self.raised,
        })
    }
}

/// Takes `taken`, segmenting's answer, into `meeting` as Elysia, from `capture`, the meeting's
/// first: every excerpt, black-out, and task a person made or changed stands, and so does every
/// one the answer takes again; Elysia's others are removed and the answer's new ones made. An
/// excerpt kept takes the answer's title, summary, and projects as Elysia's, which never override
/// a person's (`models::attribution`), so only a project newly on an excerpt wakes its Project
/// Manager. A black-out never covers a word of a person's excerpt. A task new or changed goes
/// where its share says (`super::tasks`). Recorded as one change. The caller holds the meeting's
/// lock, marks it done, and then refreshes it ([`refresh`]), and announces the items its tasks
/// raised.
///
/// # Errors
/// Propagates any database error, and any refusal of an item a task raises, which an item on no
/// project never meets.
pub(crate) async fn take(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    capture: &MeetingCapture,
    taken: &Taken,
) -> Result<Took, WorkError> {
    let mut taking = Taking::read(connection, meeting, capture).await?;
    taking
        .take_black_outs(connection, &taken.black_outs)
        .await?;
    taking.take_excerpts(connection, &taken.excerpts).await?;
    taking.take_tasks(connection, &taken.tasks).await?;
    Ok(taking.finish(connection).await?)
}

/// Attributes `record` to each of `projects` that still exists as Elysia, and takes it off every
/// project of hers it no longer names. A person's decision about any of them stands.
async fn attribute_as_elysia(
    connection: &mut AsyncPgConnection,
    record: &TimelineRecord,
    projects: &[SegmentProject],
    existing: &HashSet<Uuid>,
) -> QueryResult<()> {
    for assigned in projects {
        // One removed while the turn ran is left out rather than failing the whole write.
        if !existing.contains(&assigned.project_id) {
            continue;
        }
        attribution::attribute(
            connection,
            record.id,
            assigned.project_id,
            assigned.confidence,
            Attributor::Elysia,
        )
        .await?;
    }
    for attribution in timeline_record::attributions_of(connection, &[record.id]).await? {
        let named = projects
            .iter()
            .any(|assigned| assigned.project_id == attribution.project_id);
        if !named && attribution.attributed_by == attribution::ELYSIA_ACTOR {
            attribution::unattribute(
                connection,
                record.id,
                attribution.project_id,
                Attributor::Elysia,
            )
            .await?;
        }
    }
    Ok(())
}

/// The id at `key` of a history entry's data.
fn id_in(data: &Value, key: &str) -> Option<Uuid> {
    data.get(key)
        .and_then(Value::as_str)
        .and_then(|id| id.parse().ok())
}

/// The ids listed at `key` of a history entry's data.
fn ids_in(data: &Value, key: &str) -> Vec<Uuid> {
    data.get(key)
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .filter_map(|id| id.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::history;
    use crate::meetings::source::TranscriptLine;
    use crate::models::meeting::{self, tests::fetched_with};
    use crate::models::meeting_word::Replacement;
    use crate::models::project::NewProject;
    use crate::realtime::EventBus;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    #[test]
    fn what_segmenting_read_is_stored_with_its_tasks_and_reads_back_without_them() {
        use crate::models::meeting_task::OwedBy;

        let range = Range {
            start_word_id: Uuid::now_v7(),
            end_word_id: Uuid::now_v7(),
        };
        let project_id = Uuid::now_v7();
        let taken = Taken {
            excerpts: Vec::new(),
            black_outs: vec![range],
            tasks: vec![
                TakenTask {
                    range,
                    reading: TaskReading {
                        title: "Send the deck".to_owned(),
                        owed_by: OwedBy::Named {
                            name: "Sharath".to_owned(),
                        },
                        project_id: Some(project_id),
                    },
                },
                TakenTask {
                    range,
                    reading: TaskReading {
                        title: "Ask legal".to_owned(),
                        owed_by: OwedBy::Nobody,
                        project_id: None,
                    },
                },
            ],
        };

        let stored = serde_json::to_value(&taken).expect("serializes");
        assert_eq!(
            stored["tasks"][0],
            json!({
                "range": range,
                "title": "Send the deck",
                "owedBy": { "kind": "named", "name": "Sharath" },
                "projectId": project_id,
            })
        );
        assert_eq!(stored["tasks"][1]["owedBy"], json!({ "kind": "nobody" }));
        let read: Taken = serde_json::from_value(stored).expect("reads back");
        assert_eq!(read, taken);

        // An answer stored before tasks were read takes none.
        let before_tasks = json!({ "excerpts": [], "blackOuts": [range] });
        let read: Taken = serde_json::from_value(before_tasks).expect("reads");
        assert!(read.tasks.is_empty());
    }

    fn word(position: i64, speaker: &str, text: &str, start_ms: i64) -> Word {
        Word {
            id: Uuid::now_v7(),
            meeting_id: Uuid::nil(),
            position,
            text: text.to_owned(),
            speaker: speaker.to_owned(),
            start_ms,
            end_ms: start_ms + 400,
            origin: "source".to_owned(),
            capture_id: None,
            introduced_by: Uuid::nil(),
            superseded_by: None,
            created_at: Utc::now(),
        }
    }

    fn black_out_over(start: &Word, end: &Word) -> Mark {
        Mark {
            id: Uuid::now_v7(),
            meeting_id: Uuid::nil(),
            kind: MarkKind::BlackOut.as_str().to_owned(),
            start_word_id: start.id,
            end_word_id: end.id,
            data: json!({}),
            created_by: ELYSIA_USER_ID,
            agent: None,
            agent_name: None,
            created_at: Utc::now(),
            removed_by: None,
        }
    }

    /// A hello blacked out, then the launch.
    fn launch_meeting() -> (Vec<Word>, Mark) {
        let words = vec![
            word(0, "Ada", "Is", 0),
            word(1, "Ada", "my", 500),
            word(2, "Ada", "mic", 1_000),
            word(3, "Ada", "on?", 1_500),
            word(4, "Grace", "Yes.", 2_500),
            word(5, "Ada", "Launch", 61_000),
            word(6, "Ada", "is", 61_500),
            word(7, "Ada", "Friday.", 62_000),
        ];
        let hello = black_out_over(&words[0], &words[4]);
        (words, hello)
    }

    #[test]
    fn a_run_of_words_reads_as_lines_with_its_black_outs_left_out() {
        let (words, hello) = launch_meeting();
        let text = MeetingText::new(words, &[hello]);

        let reading = text.read(0..=7);
        assert_eq!(reading.body, "[1:01] Ada: Launch is Friday.\n");
        assert_eq!(
            reading.speakers,
            ["Ada"],
            "Grace spoke only under the black-out"
        );
        assert_eq!(reading.opening, "Launch is Friday.");
        assert_eq!(
            (reading.start_ms, reading.end_ms),
            (0, 62_400),
            "its span holds the blacked-out words, which still play"
        );
        assert!(text.is_blacked(4));
        assert!(!text.is_blacked(5));
    }

    #[test]
    fn a_black_out_inside_a_line_breaks_it_rather_than_joining_its_ends() {
        let words = vec![
            word(0, "Ada", "We", 0),
            word(1, "Ada", "ship", 400),
            word(2, "Ada", "um", 800),
            word(3, "Ada", "sorry", 1_200),
            word(4, "Ada", "Friday.", 1_600),
        ];
        let aside = black_out_over(&words[2], &words[3]);
        let text = MeetingText::new(words, &[aside]);

        assert_eq!(
            text.read(0..=4).body,
            "[0:00] Ada: We ship\n[0:01] Ada: Friday.\n"
        );
        assert_eq!(text.agent_text(0..5), "We ship [blacked out] Friday.");
    }

    #[test]
    fn a_meeting_in_words_alone_reads_without_times() {
        let words = vec![word(0, "Ada", "Hello.", 0), word(1, "", "Hi.", 0)];
        let mut untimed = words;
        for each in &mut untimed {
            each.end_ms = 0;
        }
        let text = MeetingText::new(untimed, &[]);
        assert!(!text.is_timed());
        assert_eq!(text.read(0..=1).body, "Ada: Hello.\nSomeone: Hi.\n");
    }

    #[test]
    fn a_range_reads_only_between_standing_words_in_order() {
        let (words, _) = launch_meeting();
        let (first, last) = (words[5].id, words[7].id);
        let text = MeetingText::new(words, &[]);
        let range = Range {
            start_word_id: first,
            end_word_id: last,
        };
        assert_eq!(text.places_of(range), Some(5..=7));
        assert_eq!(text.range_of(&(5..=7)), range);
        let backwards = Range {
            start_word_id: last,
            end_word_id: first,
        };
        assert_eq!(text.places_of(backwards), None);
        let elsewhere = Range {
            start_word_id: Uuid::now_v7(),
            end_word_id: last,
        };
        assert_eq!(text.places_of(elsewhere), None);
    }

    #[test]
    fn a_persons_excerpt_is_named_by_its_first_words() {
        let words: Vec<Word> = (0..20)
            .map(|index| word(index, "Ada", &format!("w{index}"), index * 300))
            .collect();
        let text = MeetingText::new(words, &[]);
        let heading = Heading::of_words(&text.read(0..=19));
        assert_eq!(heading.title, "w0 w1 w2 w3 w4 w5 w6 w7 w8 w9 w10 w11…");
        assert!(heading.summary.is_empty());
        assert_eq!(Heading::of_words(&text.read(0..=2)).title, "w0 w1 w2");
    }

    #[test]
    fn a_meetings_document_holds_every_word_but_the_blacked_out_ones() {
        let (words, hello) = launch_meeting();
        let text = MeetingText::new(words, &[hello]);
        let meeting = Meeting {
            id: Uuid::now_v7(),
            title: "Weekly sync".to_owned(),
            started_at: Utc::now(),
            duration_ms: None,
            organizer: String::new(),
            participants: json!([]),
            segments: None,
            state: MeetingState::Done.as_str().to_owned(),
            created_by: TEST_PERSON_ID,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            hold_for_approval: false,
            merged_into: None,
            visibility: "public".to_owned(),
            visibility_item_id: None,
        };
        let document = transcript_document(&meeting, &text, &[]).expect("a document");
        let indexed = document.passages.join("\n");
        assert!(indexed.starts_with("Weekly sync"), "{indexed}");
        assert!(
            indexed.contains("[1:01] Ada: Launch is Friday."),
            "{indexed}"
        );
        assert!(!indexed.contains("mic"), "{indexed}");
        assert_eq!(document.source, KnowledgeSource::Meeting { id: meeting.id });
        assert_eq!(document.url, None);

        let (words, _) = launch_meeting();
        let everything = black_out_over(&words[0], &words[7]);
        let silent = MeetingText::new(words, &[everything]);
        assert!(
            transcript_document(&meeting, &silent, &[]).is_none(),
            "a meeting blacked out whole has nothing to search"
        );
    }

    fn line(start_ms: i64, speaker: &str, text: &str) -> TranscriptLine {
        TranscriptLine {
            start_ms,
            end_ms: start_ms + 2_000,
            speaker: speaker.to_owned(),
            text: text.to_owned(),
        }
    }

    /// A segmented meeting of the test person's, its final transcript lined up from `lines`.
    async fn segmented_meeting(
        connection: &mut AsyncPgConnection,
        lines: &[TranscriptLine],
    ) -> Meeting {
        let (fetched, _capture) =
            fetched_with(connection, serde_json::to_value(lines).expect("serializes")).await;
        crate::meetings::final_transcript::rebuild(connection, fetched.id, person())
            .await
            .expect("lines up");
        meeting::record_done(connection, fetched.id)
            .await
            .expect("done")
    }

    fn person() -> Author<'static> {
        Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: Utc::now(),
        }
    }

    /// What the knowledge index holds of `record`, its passages in order.
    async fn passages_of(
        connection: &mut AsyncPgConnection,
        record: &TimelineRecord,
    ) -> Vec<String> {
        use crate::database::schema::{knowledge_documents, knowledge_passages};
        use diesel::{ExpressionMethods, QueryDsl};
        use diesel_async::RunQueryDsl;

        knowledge_passages::table
            .inner_join(knowledge_documents::table)
            .filter(knowledge_documents::timeline_record_id.eq(record.id))
            .order(knowledge_passages::position.asc())
            .select(knowledge_passages::content)
            .load(connection)
            .await
            .expect("reads")
    }

    async fn indexed_of(connection: &mut AsyncPgConnection, record: &TimelineRecord) -> bool {
        !passages_of(connection, record).await.is_empty()
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_person_takes_a_range_into_a_project_removes_it_and_undoes_both() {
        let (_url, mut connection) = migrated_database().await;
        let launch = project::create(
            &mut connection,
            &NewProject {
                name: "Launch".to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project");
        let current = segmented_meeting(
            &mut connection,
            &[
                line(0, "Ada", "Hello there."),
                line(61_000, "Grace", "Launch is Friday."),
            ],
        )
        .await;
        let words = meeting_word::standing(&mut connection, current.id)
            .await
            .expect("reads");
        let range = Range {
            start_word_id: words[2].id,
            end_word_id: words[4].id,
        };
        let onto_launch = [SegmentProject {
            project_id: launch.id,
            confidence: 0.2,
        }];

        let assigned = assign(&mut connection, &current, range, &onto_launch, person())
            .await
            .expect("assigns");
        assert_eq!(assigned.record.title, "Launch is Friday.");
        assert_eq!(assigned.record.body, "[1:01] Grace: Launch is Friday.\n");
        let attributions = timeline_record::attributions_of(&mut connection, &[assigned.record.id])
            .await
            .expect("reads");
        assert_eq!(
            (attributions[0].project_id, attributions[0].confidence),
            (launch.id, 1.0),
            "a person's assignment is certain, whatever it was sent with"
        );
        assert!(indexed_of(&mut connection, &assigned.record).await);
        assert!(
            assign(&mut connection, &current, range, &[], person())
                .await
                .is_err(),
            "an excerpt is on a project"
        );

        let mark_id = assigned.record.excerpt_mark_id.expect("an excerpt");
        let removed = remove(&mut connection, &current, mark_id, person())
            .await
            .expect("removes");
        assert!(
            timeline_record::attributions_of(&mut connection, &[assigned.record.id])
                .await
                .expect("reads")
                .is_empty(),
            "a removed excerpt's record leaves its projects"
        );
        assert!(!indexed_of(&mut connection, &assigned.record).await);
        assert!(
            timeline_record::of_meeting(&mut connection, current.id)
                .await
                .expect("reads")
                .is_empty()
        );

        let undone = history::undo(&mut connection, current.id, removed.id, person())
            .await
            .expect("undoes");
        assert_eq!(undone.entry.data["recordId"], json!(assigned.record.id));
        let back = timeline_record::attributions_of(&mut connection, &[assigned.record.id])
            .await
            .expect("reads");
        assert_eq!(back.len(), 1, "it is back on its project");
        assert!(indexed_of(&mut connection, &assigned.record).await);

        // Undoing the assignment itself, with the excerpt standing, takes it out again; then the
        // undo that put it back has nothing left to undo.
        history::undo(&mut connection, current.id, assigned.entry.id, person())
            .await
            .expect("undoes the assignment");
        assert!(
            history::undo(&mut connection, current.id, undone.entry.id, person())
                .await
                .is_err(),
            "the excerpt moved on since"
        );
        assert!(
            timeline_record::of_meeting(&mut connection, current.id)
                .await
                .expect("reads")
                .is_empty()
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    #[expect(
        clippy::too_many_lines,
        reason = "three black-outs made, one taking in the others, undone, and one removed"
    )]
    async fn a_black_out_takes_in_what_it_touches_and_its_undo_gives_it_back() {
        let (_url, mut connection) = migrated_database().await;
        let launch = project::create(
            &mut connection,
            &NewProject {
                name: "Launch".to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project");
        let current = segmented_meeting(
            &mut connection,
            &[line(0, "Ada", "We ship um sorry hold on Friday.")],
        )
        .await;
        let words = meeting_word::standing(&mut connection, current.id)
            .await
            .expect("reads");
        let whole = Range {
            start_word_id: words[0].id,
            end_word_id: words[6].id,
        };
        let onto_launch = [SegmentProject {
            project_id: launch.id,
            confidence: 1.0,
        }];
        let assigned = assign(&mut connection, &current, whole, &onto_launch, person())
            .await
            .expect("assigns");
        let range = |first: usize, last: usize| Range {
            start_word_id: words[first].id,
            end_word_id: words[last].id,
        };
        let um = black_out(&mut connection, &current, range(2, 2), person())
            .await
            .expect("blacks out");
        black_out(&mut connection, &current, range(5, 5), person())
            .await
            .expect("blacks out");

        // Over one and touching the other: one black-out from "um" to "on".
        let grown = black_out(&mut connection, &current, range(3, 4), person())
            .await
            .expect("blacks out");
        let standing: Vec<Mark> = meeting_mark::standing(&mut connection, current.id)
            .await
            .expect("reads")
            .into_iter()
            .filter(|mark| mark.kind() == Some(MarkKind::BlackOut))
            .collect();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].range(), range(2, 5));
        let record = timeline_record::of_excerpt(&mut connection, standing[0].id)
            .await
            .expect("reads");
        assert!(record.is_none(), "a black-out has no record");
        let excerpt = timeline_record::of_excerpt(
            &mut connection,
            assigned.record.excerpt_mark_id.expect("an excerpt"),
        )
        .await
        .expect("reads")
        .expect("its record");
        assert_eq!(
            excerpt.body, "[0:00] Ada: We ship\n[0:01] Ada: Friday.\n",
            "the excerpt over the words read them again at once"
        );

        history::undo(&mut connection, current.id, grown.id, person())
            .await
            .expect("undoes");
        let standing: Vec<Range> = meeting_mark::standing(&mut connection, current.id)
            .await
            .expect("reads")
            .into_iter()
            .filter(|mark| mark.kind() == Some(MarkKind::BlackOut))
            .map(|mark| mark.range())
            .collect();
        assert_eq!(
            standing.len(),
            2,
            "the two it took in are back: {standing:?}"
        );
        assert!(standing.contains(&range(2, 2)) && standing.contains(&range(5, 5)));

        // Removing "um" lets its word read again.
        let um_mark = meeting_mark::standing(&mut connection, current.id)
            .await
            .expect("reads")
            .into_iter()
            .find(|mark| mark.range() == range(2, 2))
            .expect("the um");
        assert_eq!(Some(um_mark.id), id_in(&um.data, "markId"));
        remove(&mut connection, &current, um_mark.id, person())
            .await
            .expect("removes");
        let excerpt = timeline_record::of_excerpt(
            &mut connection,
            assigned.record.excerpt_mark_id.expect("an excerpt"),
        )
        .await
        .expect("reads")
        .expect("its record");
        assert!(
            excerpt.body.contains("We ship um sorry hold"),
            "{}",
            excerpt.body
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_black_out_takes_in_a_touching_chain_whole_and_no_excerpt_reads_blacked_out_words_alone()
     {
        let (_url, mut connection) = migrated_database().await;
        let launch = project::create(
            &mut connection,
            &NewProject {
                name: "Launch".to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project");
        let current = segmented_meeting(
            &mut connection,
            &[line(0, "Ada", "One two three four five six seven.")],
        )
        .await;
        let words = meeting_word::standing(&mut connection, current.id)
            .await
            .expect("reads");
        let range = |first: usize, last: usize| Range {
            start_word_id: words[first].id,
            end_word_id: words[last].id,
        };
        // Two black-outs side by side, as segmenting makes them, the first made first: it touches
        // the new one only once the second is taken in.
        for (first, last) in [(0, 2), (3, 5)] {
            let side_by_side = NewMark::new(
                current.id,
                MarkKind::BlackOut,
                range(first, last),
                json!({}),
                person(),
            );
            meeting_mark::insert(&mut connection, side_by_side)
                .await
                .expect("marks");
        }

        black_out(&mut connection, &current, range(6, 6), person())
            .await
            .expect("blacks out");
        let standing: Vec<Range> = meeting_mark::standing(&mut connection, current.id)
            .await
            .expect("reads")
            .into_iter()
            .filter(|mark| mark.kind() == Some(MarkKind::BlackOut))
            .map(|mark| mark.range())
            .collect();
        assert_eq!(
            standing,
            vec![range(0, 6)],
            "the whole chain is one black-out"
        );

        // Every word is blacked out now, so a range of them reads nothing and is no excerpt.
        let onto_launch = [SegmentProject {
            project_id: launch.id,
            confidence: 1.0,
        }];
        let refused = assign(
            &mut connection,
            &current,
            range(1, 3),
            &onto_launch,
            person(),
        )
        .await;
        assert!(
            matches!(refused, Err(ExcerptError::BlackedOut)),
            "{refused:?}"
        );
        assert!(
            timeline_record::of_meeting(&mut connection, current.id)
                .await
                .expect("reads")
                .is_empty()
        );
    }

    /// Whether meeting `meeting_id`'s whole transcript is a document of the index.
    async fn transcript_indexed(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> bool {
        use crate::database::schema::knowledge_documents;
        use diesel::{ExpressionMethods, QueryDsl};
        use diesel_async::RunQueryDsl;

        knowledge_documents::table
            .filter(knowledge_documents::meeting_id.eq(meeting_id))
            .count()
            .get_result::<i64>(connection)
            .await
            .expect("counts")
            == 1
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_merged_away_takes_its_transcript_out_of_the_index_until_it_is_split() {
        use crate::meetings::duplicates;
        use crate::models::meeting_duplicate::{self, NewMeetingDuplicate};
        use crate::models::user::SYSTEM_USER_ID;

        let (_url, mut connection) = migrated_database().await;
        let keeper =
            segmented_meeting(&mut connection, &[line(0, "Ada", "Launch is Friday.")]).await;
        let merging =
            segmented_meeting(&mut connection, &[line(0, "Ada", "Launch is Friday too.")]).await;
        refresh(&mut connection, &merging, None)
            .await
            .expect("indexes");
        assert!(transcript_indexed(&mut connection, merging.id).await);

        let pair = meeting_duplicate::insert_new(
            &mut connection,
            NewMeetingDuplicate {
                id: Uuid::now_v7(),
                meeting_id: merging.id,
                same_as_id: keeper.id,
                action_item_id: None,
                created_by: SYSTEM_USER_ID,
            },
        )
        .await
        .expect("records")
        .expect("a new pair");
        // The test person owns both, as only a person's own meetings are compared.
        let owner = crate::models::meeting::Reader {
            id: TEST_PERSON_ID,
            reach: crate::models::meeting::Reach::Public,
        };
        duplicates::merge(&mut connection, pair.id, owner, Utc::now())
            .await
            .expect("merges");
        assert!(
            !transcript_indexed(&mut connection, merging.id).await,
            "its words are searched in the meeting it went into"
        );

        let merges = meeting_event::of_kind(&mut connection, keeper.id, EventKind::Merged)
            .await
            .expect("reads");
        history::undo(&mut connection, keeper.id, merges[0].id, person())
            .await
            .expect("splits");
        assert!(
            transcript_indexed(&mut connection, merging.id).await,
            "split out, it is its own again"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_edit_refreshes_the_excerpts_over_its_words_at_once() {
        let (_url, mut connection) = migrated_database().await;
        let launch = project::create(
            &mut connection,
            &NewProject {
                name: "Launch".to_owned(),
                description: String::new(),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("a project");
        let current = segmented_meeting(
            &mut connection,
            &[line(61_000, "Grace", "Lunch is Friday.")],
        )
        .await;
        let words = meeting_word::standing(&mut connection, current.id)
            .await
            .expect("reads");
        let whole = Range {
            start_word_id: words[0].id,
            end_word_id: words[2].id,
        };
        let onto_launch = [SegmentProject {
            project_id: launch.id,
            confidence: 1.0,
        }];
        let assigned = assign(&mut connection, &current, whole, &onto_launch, person())
            .await
            .expect("assigns");

        crate::meetings::fixes::edit(
            &mut connection,
            &EventBus::in_process(),
            &current,
            Replacement {
                range: Range {
                    start_word_id: words[0].id,
                    end_word_id: words[0].id,
                },
                text: "Launch",
            },
            person(),
        )
        .await
        .expect("edits");

        let excerpt = timeline_record::of_excerpt(
            &mut connection,
            assigned.record.excerpt_mark_id.expect("an excerpt"),
        )
        .await
        .expect("reads")
        .expect("its record");
        assert_eq!(excerpt.body, "[1:01] Grace: Launch is Friday.\n");
        let passages = passages_of(&mut connection, &excerpt).await;
        assert!(
            passages
                .iter()
                .any(|passage| passage.contains("Launch is Friday.")),
            "{passages:?}"
        );
    }
}
