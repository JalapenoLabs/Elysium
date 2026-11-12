// Copyright © 2026 Jalapeno Labs

//! Files and frames on a meeting: attaching them, pinning them to ranges of its words, taking them
//! off, and undoing each. See `docs/meetings.md`, Attachments and frames.
//!
//! # Pins
//!
//! An attachment is about the meeting, and a pin says which of its words it is about: a mark of
//! kind `pin` over a range (`models::meeting_mark`), naming the attachment in its data as
//! `attachmentId`. Pins move with edits as every mark does, and an excerpt carries every
//! attachment pinned over its span, to the index and to the Project Managers and workers that
//! read it ([`crate::models::meeting_attachment::pinned_over`]), so pinning a deck to an excerpt
//! is pinning it over the excerpt's range. A frame a person takes is pinned as it is taken, over
//! the words they selected or the word being spoken at its moment; segmenting pins its own over
//! the lines it judged each against.
//!
//! # History
//!
//! Attaching (`attached`) and taking off (`detached`) are changes of the meeting's history, each
//! naming the pins that went with it, so an undo takes a frame away with its pins, or puts a file
//! back with the pins that went with it. A pin made or removed on its own is a `marked` or
//! `unmarked` change, undone as any mark's (`models::meeting_mark`). Every change takes the
//! meeting's lock, and refreshes what reads the words it touched (`super::excerpts::refresh`).

use diesel::QueryResult;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::models::meeting_attachment::{AttachmentKind, TextState};
use crate::models::timeline_record::{self, TimelineRecord};

use super::excerpts;
use crate::meeting_attachments::MAX_PER_MEETING;
use crate::models::agent::Author;
use crate::models::meeting::Meeting;
use crate::models::meeting_attachment::{
    self, MeetingAttachment, NewAttachment, StoredFrame, UploadedFile,
};
use crate::models::meeting_capture;
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::meeting_mark::{self, Mark, MarkError, MarkKind, NewMark, Range};
use crate::models::meeting_word;

/// Why a file or a frame could not be attached, pinned, or taken off.
#[derive(Debug, thiserror::Error)]
pub enum AttachmentError {
    /// The range, or the meeting's lock, refused (`MarkError`), the meeting merged away among them.
    #[error(transparent)]
    Mark(#[from] MarkError),
    /// The meeting holds no such attachment, or a frame's capture is not one of the meeting's.
    #[error("not found")]
    NotFound,
    /// The attachment was taken off already.
    #[error("it was taken off already")]
    Removed,
    /// The meeting holds as many files and frames as one may.
    #[error("the meeting holds {MAX_PER_MEETING} files and frames already")]
    TooMany,
    /// A frame's capture keeps no recording to have taken it from.
    #[error("that capture keeps no recording")]
    NoRecording,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// A file or a frame attached, and its entry in the meeting's history.
#[derive(Debug, Clone)]
pub struct Attached {
    pub attachment: MeetingAttachment,
    pub entry: MeetingEvent,
}

/// Records `file`, which an upload brought, as attached to its meeting by `author`, before its
/// parts are stored. The caller holds the meeting's lock, in the transaction that finishes the
/// upload.
///
/// # Errors
/// [`AttachmentError::TooMany`] for a meeting holding [`MAX_PER_MEETING`]; any database error.
pub async fn attach_file(
    connection: &mut AsyncPgConnection,
    file: UploadedFile,
    author: Author<'_>,
) -> Result<Attached, AttachmentError> {
    require_room(connection, file.meeting_id).await?;
    let attachment =
        meeting_attachment::insert(connection, NewAttachment::file(file, author)).await?;
    let entry = record(
        connection,
        EventKind::Attached,
        &attachment,
        &[],
        author,
        None,
    )
    .await?;
    Ok(Attached { attachment, entry })
}

/// Records `frame`, already stored, as attached to `meeting` by `author`, pinned over `range`, or
/// over the word being spoken at its moment when no range is given. A meeting with no words to
/// pin it over keeps it unpinned.
///
/// # Errors
/// [`AttachmentError::Mark`] for a range that is not one of the transcript as it reads, or a
/// meeting merged away, [`AttachmentError::NotFound`] for a capture of another meeting,
/// [`AttachmentError::NoRecording`] for one with no recording, and [`AttachmentError::TooMany`];
/// any database error.
pub async fn attach_frame(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    frame: StoredFrame,
    range: Option<Range>,
    author: Author<'_>,
) -> Result<Attached, AttachmentError> {
    connection
        .transaction(async |connection| {
            meeting_mark::lock_meeting(connection, meeting.id).await?;
            let attached = frame_on(connection, meeting, frame, range, author).await?;
            Ok(attached)
        })
        .await
}

/// Records `frame` on `meeting`, pinned over `range` or over the word at its moment: what
/// [`attach_frame`] does under the meeting's lock, which segmenting's own transaction already
/// holds when it pins its frames.
pub(crate) async fn frame_on(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    frame: StoredFrame,
    range: Option<Range>,
    author: Author<'_>,
) -> Result<Attached, AttachmentError> {
    let capture = match meeting_capture::find(connection, frame.capture_id).await {
        Ok(capture) if capture.meeting_id == meeting.id => capture,
        Ok(_) | Err(diesel::result::Error::NotFound) => return Err(AttachmentError::NotFound),
        Err(error) => return Err(error.into()),
    };
    if capture.stored_recording().is_none() {
        return Err(AttachmentError::NoRecording);
    }
    require_room(connection, meeting.id).await?;
    let range = if let Some(range) = range {
        meeting_mark::check_range(connection, meeting.id, range).await?;
        Some(range)
    } else {
        word_at(connection, meeting.id, capture.offset_ms + frame.capture_ms).await?
    };
    let attachment =
        meeting_attachment::insert(connection, NewAttachment::frame(frame, author)).await?;
    let mut pins = Vec::new();
    if let Some(range) = range {
        let data = json!({ "attachmentId": attachment.id });
        let new = NewMark::new(meeting.id, MarkKind::Pin, range, data, author);
        pins.push(meeting_mark::insert(connection, new).await?);
    }
    let entry = record(
        connection,
        EventKind::Attached,
        &attachment,
        &pins,
        author,
        None,
    )
    .await?;
    refresh_over_pins(connection, meeting, &pins).await?;
    Ok(Attached { attachment, entry })
}

/// Pins standing attachment `attachment_id` of `meeting` over `range`, as `author`: a `marked`
/// change, undone as any mark's. Pinning it over an excerpt's range carries it with that excerpt.
///
/// # Errors
/// [`AttachmentError::NotFound`] for an attachment the meeting does not hold,
/// [`AttachmentError::Removed`] for one taken off, and [`AttachmentError::Mark`] for a range that
/// is not one of the transcript as it reads or a meeting merged away; any database error.
pub async fn pin(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    attachment_id: Uuid,
    range: Range,
    author: Author<'_>,
) -> Result<(Mark, MeetingEvent), AttachmentError> {
    connection
        .transaction(async |connection| {
            meeting_mark::lock_meeting(connection, meeting.id).await?;
            let attachment = standing(connection, meeting.id, attachment_id).await?;
            meeting_mark::check_range(connection, meeting.id, range).await?;
            let data = json!({ "attachmentId": attachment.id });
            let new = NewMark::new(meeting.id, MarkKind::Pin, range, data, author);
            let mark = meeting_mark::insert(connection, new).await?;
            let entry =
                meeting_mark::record_made(connection, &mark, author, None, json!({})).await?;
            refresh_over_pins(connection, meeting, std::slice::from_ref(&mark)).await?;
            Ok((mark, entry))
        })
        .await
}

/// Takes standing attachment `attachment_id` off `meeting` as `author`, with every pin of it, as
/// one `detached` change whose undo puts both back. Its file stays where it is.
///
/// # Errors
/// [`AttachmentError::NotFound`] for an attachment the meeting does not hold,
/// [`AttachmentError::Removed`] for one taken off already, and [`AttachmentError::Mark`] for a
/// meeting merged away; any database error.
pub async fn detach(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    attachment_id: Uuid,
    author: Author<'_>,
) -> Result<MeetingEvent, AttachmentError> {
    connection
        .transaction(async |connection| {
            meeting_mark::lock_meeting(connection, meeting.id).await?;
            let attachment = standing(connection, meeting.id, attachment_id).await?;
            Ok(take_off(connection, meeting, &attachment, author, None).await?)
        })
        .await
}

/// Undoes `undone`, an `attached` or `detached` entry of `meeting`, as `author`: an attachment
/// attached is taken off with the pins it came with, and one taken off is put back with the pins
/// it took away. Answers `None` when the attachment moved on since. The caller holds the
/// meeting's lock.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn undo(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> QueryResult<Option<MeetingEvent>> {
    let Some(attachment_id) = id_in(&undone.data, "attachmentId") else {
        tracing::event!(
            name: "meetings.history.unreadable",
            tracing::Level::WARN,
            meeting_event.id = %undone.id,
            "an attachment's history entry names no attachment; it cannot be undone",
        );
        return Ok(None);
    };
    let attachment = meeting_attachment::find(connection, meeting.id, attachment_id).await?;
    match undone.kind() {
        Some(EventKind::Attached) if attachment.removed_by.is_none() => Ok(Some(
            take_off(connection, meeting, &attachment, author, Some(undone.id)).await?,
        )),
        Some(EventKind::Detached) if attachment.removed_by == Some(undone.id) => {
            meeting_attachment::restore_all(connection, &[attachment.id], undone.id).await?;
            let restored =
                meeting_mark::restore_all(connection, &ids_in(&undone.data, "pins"), undone.id)
                    .await?;
            let pins = meeting_mark::find_many(connection, meeting.id, &restored).await?;
            let entry = record(
                connection,
                EventKind::Attached,
                &attachment,
                &pins,
                author,
                Some(undone.id),
            )
            .await?;
            refresh_over_pins(connection, meeting, &pins).await?;
            Ok(Some(entry))
        }
        _ => Ok(None),
    }
}

/// The standing files and frames pinned in `record`, when it is a meeting's excerpt, in the order
/// their pins start: what travels with it to the Project Managers and workers that read it. Any
/// other record carries none.
///
/// # Errors
/// Propagates any database error.
pub async fn pinned_in(
    connection: &mut AsyncPgConnection,
    record: &TimelineRecord,
) -> QueryResult<Vec<MeetingAttachment>> {
    match timeline_record::segment_span(record) {
        Some((meeting_id, start_ms, end_ms)) => {
            meeting_attachment::pinned_over(connection, meeting_id, start_ms, end_ms).await
        }
        None => Ok(Vec::new()),
    }
}

/// What an agent reads of `attachment` beside the excerpt it is pinned in: what it is and what it
/// shows, and the id it opens it by (`meeting_attachment_get`, `crate::tools::knowledge`).
pub fn agent_view(attachment: &MeetingAttachment) -> Value {
    json!({
        "attachmentId": attachment.id,
        "kind": attachment.kind,
        "fileName": attachment.file_name,
        "caption": attachment.caption,
        "sizeBytes": attachment.size_bytes,
        "hasText": attachment.text_state() == Some(TextState::Read),
    })
}

/// How a pinned attachment is named in a line of an agent's prompt: a frame by its caption, a
/// file by its name, each with the id that opens it.
pub fn prompt_line(attachment: &MeetingAttachment) -> String {
    match attachment.kind() {
        AttachmentKind::Frame => format!(
            "the frame \"{}\" (attachment {})",
            attachment.caption.as_deref().unwrap_or_default(),
            attachment.id
        ),
        AttachmentKind::File => format!(
            "the file \"{}\" (attachment {})",
            attachment.file_name, attachment.id
        ),
    }
}

/// Refreshes what reads attachment `attachment_id` of `meeting`: every excerpt it is pinned in,
/// and the meeting's own document, as reading its text does once it is read.
///
/// # Errors
/// Propagates any database error.
pub async fn refresh_pinned(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    attachment_id: Uuid,
) -> QueryResult<()> {
    let spans = meeting_attachment::pin_spans(connection, meeting.id, attachment_id).await?;
    refresh_spans(connection, meeting, &spans).await
}

/// Takes `attachment` off with every standing pin of it, as change `undid`'s undo when it is one.
async fn take_off(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    attachment: &MeetingAttachment,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> QueryResult<MeetingEvent> {
    let spans = meeting_attachment::pin_spans(connection, meeting.id, attachment.id).await?;
    let pins = pins_of(connection, meeting.id, attachment.id).await?;
    let entry = record(
        connection,
        EventKind::Detached,
        attachment,
        &pins,
        author,
        undid,
    )
    .await?;
    meeting_attachment::remove_all(connection, &[attachment.id], entry.id).await?;
    let pin_ids: Vec<Uuid> = pins.iter().map(|pin| pin.id).collect();
    meeting_mark::remove_all(connection, &pin_ids, entry.id).await?;
    refresh_spans(connection, meeting, &spans).await?;
    Ok(entry)
}

/// Meeting `meeting_id`'s standing pins of attachment `attachment_id`.
async fn pins_of(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    attachment_id: Uuid,
) -> QueryResult<Vec<Mark>> {
    let pins = meeting_mark::standing(connection, meeting_id)
        .await?
        .into_iter()
        .filter(|mark| {
            mark.kind() == Some(MarkKind::Pin)
                && id_in(&mark.data, "attachmentId") == Some(attachment_id)
        })
        .collect();
    Ok(pins)
}

/// Standing attachment `id` of meeting `meeting_id`.
async fn standing(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    id: Uuid,
) -> Result<MeetingAttachment, AttachmentError> {
    match meeting_attachment::find(connection, meeting_id, id).await {
        Ok(attachment) if attachment.removed_by.is_some() => Err(AttachmentError::Removed),
        Ok(attachment) => Ok(attachment),
        Err(diesel::result::Error::NotFound) => Err(AttachmentError::NotFound),
        Err(error) => Err(error.into()),
    }
}

/// Refuses another attachment on a meeting holding as many as one may. Exact under its lock.
async fn require_room(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> Result<(), AttachmentError> {
    if meeting_attachment::count_standing(connection, meeting_id).await? >= MAX_PER_MEETING {
        return Err(AttachmentError::TooMany);
    }
    Ok(())
}

/// The one word of meeting `meeting_id`'s transcript being spoken at `moment` on its clock, or the
/// one nearest it, as a range; `None` for a meeting with no words.
async fn word_at(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    moment: i64,
) -> QueryResult<Option<Range>> {
    let words = meeting_word::standing(connection, meeting_id).await?;
    let nearest = words.iter().min_by_key(|word| {
        if word.start_ms <= moment && moment <= word.end_ms.max(word.start_ms) {
            0
        } else {
            (word.start_ms - moment)
                .abs()
                .min((word.end_ms - moment).abs())
        }
    });
    Ok(nearest.map(|word| Range {
        start_word_id: word.id,
        end_word_id: word.id,
    }))
}

/// Records `attachment`'s `kind` of change with the `pins` that went with it.
async fn record(
    connection: &mut AsyncPgConnection,
    kind: EventKind,
    attachment: &MeetingAttachment,
    pins: &[Mark],
    author: Author<'_>,
    undid: Option<Uuid>,
) -> QueryResult<MeetingEvent> {
    let pin_ids: Vec<Uuid> = pins.iter().map(|pin| pin.id).collect();
    meeting_event::record(
        connection,
        Change {
            id: Uuid::now_v7(),
            meeting_id: attachment.meeting_id,
            kind,
            author,
            data: json!({
                "attachmentId": attachment.id,
                "attachmentKind": attachment.kind,
                "fileName": attachment.file_name,
                "pins": pin_ids,
            }),
            undid,
        },
    )
    .await
}

/// Refreshes what reads the words under each of `pins`, and the meeting's own document.
async fn refresh_over_pins(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    pins: &[Mark],
) -> QueryResult<()> {
    let mut spans = Vec::with_capacity(pins.len());
    for pin in pins {
        if let Some(span) = excerpts::span_of(connection, meeting.id, pin.range()).await? {
            spans.push(span);
        }
    }
    refresh_spans(connection, meeting, &spans).await
}

/// Refreshes every excerpt over any of `spans`, then the meeting's own document, which lists
/// every attachment whether or not an excerpt carries it. One refresh over the stretch the spans
/// cover does both: an excerpt it touches that reads the same writes nothing.
async fn refresh_spans(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
    spans: &[(i64, i64)],
) -> QueryResult<()> {
    let start = spans.iter().map(|(start, _)| *start).min();
    let end = spans.iter().map(|(_, end)| *end).max();
    match start.zip(end) {
        Some(stretch) => excerpts::refresh(connection, meeting, Some(stretch)).await,
        None => excerpts::reindex_transcript(connection, meeting).await,
    }
}

fn id_in(data: &Value, key: &str) -> Option<Uuid> {
    data.get(key)
        .and_then(Value::as_str)
        .and_then(|id| id.parse().ok())
}

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
