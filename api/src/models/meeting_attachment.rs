// Copyright © 2026 Jalapeno Labs

//! Files attached to a meeting and stills of its video, one row each, its bytes with a storage
//! provider. The rules for what a meeting takes are `crate::meeting_attachments`, and what a
//! meeting does with them, history and pins included, is `crate::meetings::attachments`. See
//! `docs/meetings.md`, Attachments and frames.
//!
//! An attachment is a frozen copy kept with its meeting, never changed once stored: five meetings
//! over one document keep five copies, each the version it discussed. Removing one marks it with
//! the change of the meeting's history that removed it (`removed_by`) and leaves its file where it
//! is, so an undo puts it back at the same address. Nothing deletes a file yet; no foreign key
//! cascades to these rows, so nothing can drop the only record of a file by accident.
//!
//! An attachment is pinned to ranges of the final transcript by marks of kind `pin`
//! (`models::meeting_mark`), each naming it in its data as `attachmentId`, and an excerpt carries
//! every attachment pinned over its span ([`pinned_over`]).

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::meeting_attachments;
use crate::models::agent::Author;

/// What an attachment is, as `meeting_attachments.kind` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    /// A file a person attached.
    File,
    /// A still of a capture's video at one moment, with its caption.
    Frame,
}

impl AttachmentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Frame => "frame",
        }
    }
}

/// How far reading a file's text got, as `meeting_attachments.text_state` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextState {
    /// Not read yet.
    Pending,
    /// Read: its text is kept, empty for a file that holds none.
    Read,
    /// A kind of file with no text to read, such as a frame, a video, or an archive.
    None,
    /// It could not be read.
    Unreadable,
}

impl TextState {
    const ALL: [Self; 4] = [Self::Pending, Self::Read, Self::None, Self::Unreadable];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Read => "read",
            Self::None => "none",
            Self::Unreadable => "unreadable",
        }
    }

    /// The state stored as `text`, if this build knows it.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = meeting_attachments, check_for_backend(diesel::pg::Pg))]
pub struct MeetingAttachment {
    pub id: Uuid,
    pub meeting_id: Uuid,
    /// An [`AttachmentKind`] name.
    pub kind: String,
    pub storage_location_id: Uuid,
    /// Where in its location; `None` while an upload's parts wait to be stored.
    pub storage_path: Option<String>,
    pub stored_at: Option<DateTime<Utc>>,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    /// A frame's capture and its moment on that capture's own clock.
    pub capture_id: Option<Uuid>,
    pub capture_ms: Option<i64>,
    pub caption: Option<String>,
    /// A frame's fingerprint (`crate::images::fingerprint`).
    pub fingerprint: Option<Vec<u8>>,
    /// The text read from a file, once read.
    pub text: Option<String>,
    /// A [`TextState`] name.
    pub text_state: String,
    pub created_by: Uuid,
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
    /// The change that removed it; `None` while it stands.
    pub removed_by: Option<Uuid>,
}

impl MeetingAttachment {
    /// Its kind.
    ///
    /// # Panics
    /// Panics on a kind `meeting_attachments_kind_check` does not allow, which only a migration
    /// that drifted from [`AttachmentKind`] can cause.
    pub fn kind(&self) -> AttachmentKind {
        match self.kind.as_str() {
            "file" => AttachmentKind::File,
            "frame" => AttachmentKind::Frame,
            other => panic!("meeting_attachments_kind_check allows no kind {other:?}"),
        }
    }

    /// How far reading its text got, when this build knows the state.
    pub fn text_state(&self) -> Option<TextState> {
        TextState::parse(&self.text_state)
    }
}

/// A file or a frame to record: for a frame, where it is already stored and what it shows.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_attachments)]
pub struct NewAttachment {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub kind: String,
    pub storage_location_id: Uuid,
    pub storage_path: Option<String>,
    pub stored_at: Option<DateTime<Utc>>,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub capture_id: Option<Uuid>,
    pub capture_ms: Option<i64>,
    pub caption: Option<String>,
    pub fingerprint: Option<Vec<u8>>,
    pub text_state: String,
    pub created_by: Uuid,
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A file an upload brought, to record before its parts are stored.
#[derive(Debug, Clone)]
pub struct UploadedFile {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub storage_location_id: Uuid,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    /// Whether its kind has text to read.
    pub has_text: bool,
}

/// A frame already written to its storage location.
#[derive(Debug, Clone)]
pub struct StoredFrame {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub storage_location_id: Uuid,
    pub storage_path: String,
    pub file_name: String,
    pub size_bytes: i64,
    pub capture_id: Uuid,
    pub capture_ms: i64,
    pub caption: String,
    pub fingerprint: Vec<u8>,
}

impl NewAttachment {
    /// The row for `file`, unstored until its parts are, its text waiting to be read when it has
    /// any.
    pub fn file(file: UploadedFile, author: Author<'_>) -> Self {
        let text_state = if file.has_text {
            TextState::Pending
        } else {
            TextState::None
        };
        Self {
            id: file.id,
            meeting_id: file.meeting_id,
            kind: AttachmentKind::File.as_str().to_owned(),
            storage_location_id: file.storage_location_id,
            storage_path: None,
            stored_at: None,
            file_name: file.file_name,
            content_type: file.content_type,
            size_bytes: file.size_bytes,
            capture_id: None,
            capture_ms: None,
            caption: None,
            fingerprint: None,
            text_state: text_state.as_str().to_owned(),
            created_by: author.user_id,
            agent: author.agent.map(|agent| agent.id().to_owned()),
            agent_name: author
                .agent
                .and_then(|agent| agent.name().map(ToOwned::to_owned)),
            created_at: author.at,
        }
    }

    /// The row for `frame`, stored already, with no text but its caption.
    pub fn frame(frame: StoredFrame, author: Author<'_>) -> Self {
        Self {
            id: frame.id,
            meeting_id: frame.meeting_id,
            kind: AttachmentKind::Frame.as_str().to_owned(),
            storage_location_id: frame.storage_location_id,
            storage_path: Some(frame.storage_path),
            stored_at: Some(author.at),
            file_name: frame.file_name,
            content_type: "image/webp".to_owned(),
            size_bytes: frame.size_bytes,
            capture_id: Some(frame.capture_id),
            capture_ms: Some(frame.capture_ms),
            caption: Some(frame.caption),
            fingerprint: Some(frame.fingerprint),
            text_state: TextState::None.as_str().to_owned(),
            created_by: author.user_id,
            agent: author.agent.map(|agent| agent.id().to_owned()),
            agent_name: author
                .agent
                .and_then(|agent| agent.name().map(ToOwned::to_owned)),
            created_at: author.at,
        }
    }
}

/// Records `new`. The caller holds the meeting's lock and records the change in its history.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn insert(
    connection: &mut AsyncPgConnection,
    new: NewAttachment,
) -> QueryResult<MeetingAttachment> {
    diesel::insert_into(meeting_attachments::table)
        .values(new)
        .returning(MeetingAttachment::as_returning())
        .get_result(connection)
        .await
}

/// Meeting `meeting_id`'s standing files and frames, in the order they arrived.
///
/// # Errors
/// Propagates any database error.
pub async fn of_meeting(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Vec<MeetingAttachment>> {
    meeting_attachments::table
        .filter(meeting_attachments::meeting_id.eq(meeting_id))
        .filter(meeting_attachments::removed_by.is_null())
        .order(meeting_attachments::id.asc())
        .select(MeetingAttachment::as_select())
        .load(connection)
        .await
}

/// How many files and frames meeting `meeting_id` holds. Exact while the caller holds its lock.
///
/// # Errors
/// Propagates any database error.
pub async fn count_standing(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<i64> {
    meeting_attachments::table
        .filter(meeting_attachments::meeting_id.eq(meeting_id))
        .filter(meeting_attachments::removed_by.is_null())
        .count()
        .get_result(connection)
        .await
}

/// Attachment `id` of meeting `meeting_id`, standing or removed.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the meeting holds no such attachment.
pub async fn find(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    id: Uuid,
) -> QueryResult<MeetingAttachment> {
    meeting_attachments::table
        .find(id)
        .filter(meeting_attachments::meeting_id.eq(meeting_id))
        .select(MeetingAttachment::as_select())
        .first(connection)
        .await
}

/// Attachment `id`, of whichever meeting.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is none.
pub async fn find_by_id(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<MeetingAttachment> {
    meeting_attachments::table
        .find(id)
        .select(MeetingAttachment::as_select())
        .first(connection)
        .await
}

/// The fingerprints of meeting `meeting_id`'s standing frames, which a new frame is compared with
/// to drop near duplicates.
///
/// # Errors
/// Propagates any database error.
pub async fn standing_fingerprints(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Vec<Vec<u8>>> {
    let fingerprints: Vec<Option<Vec<u8>>> = meeting_attachments::table
        .filter(meeting_attachments::meeting_id.eq(meeting_id))
        .filter(meeting_attachments::removed_by.is_null())
        .filter(meeting_attachments::kind.eq(AttachmentKind::Frame.as_str()))
        .select(meeting_attachments::fingerprint)
        .load(connection)
        .await?;
    Ok(fingerprints.into_iter().flatten().collect())
}

/// Marks every standing attachment among `ids` removed by change `event_id`, and answers the
/// ones it removed. The caller holds the meeting's lock.
pub(crate) async fn remove_all(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
    event_id: Uuid,
) -> QueryResult<Vec<Uuid>> {
    diesel::update(
        meeting_attachments::table
            .filter(meeting_attachments::id.eq_any(ids))
            .filter(meeting_attachments::removed_by.is_null()),
    )
    .set(meeting_attachments::removed_by.eq(event_id))
    .returning(meeting_attachments::id)
    .get_results(connection)
    .await
}

/// Puts back every attachment among `ids` that change `removed_by` removed, and answers the ones
/// it put back. The caller holds the meeting's lock.
pub(crate) async fn restore_all(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
    removed_by: Uuid,
) -> QueryResult<Vec<Uuid>> {
    diesel::update(
        meeting_attachments::table
            .filter(meeting_attachments::id.eq_any(ids))
            .filter(meeting_attachments::removed_by.eq(removed_by)),
    )
    .set(meeting_attachments::removed_by.eq(None::<Uuid>))
    .returning(meeting_attachments::id)
    .get_results(connection)
    .await
}

/// Records that file `id`'s bytes are stored at `storage_path`, once: a second attempt that
/// stored the same file finds it stored and changes nothing. Answers the row as it now reads.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such file.
pub async fn record_stored(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    storage_path: &str,
    at: DateTime<Utc>,
) -> QueryResult<MeetingAttachment> {
    let stored: Option<MeetingAttachment> = diesel::update(
        meeting_attachments::table
            .find(id)
            .filter(meeting_attachments::stored_at.is_null()),
    )
    .set((
        meeting_attachments::storage_path.eq(storage_path),
        meeting_attachments::stored_at.eq(at),
    ))
    .returning(MeetingAttachment::as_returning())
    .get_result(connection)
    .await
    .optional()?;
    match stored {
        Some(stored) => Ok(stored),
        None => find_by_id(connection, id).await,
    }
}

/// Records what reading file `id`'s text found: `text` for [`TextState::Read`], and nothing for
/// any other state. Cut to [`crate::meeting_attachments::TEXT_MAX_CHARACTERS`] by the caller.
///
/// # Errors
/// Propagates any database error.
pub async fn record_text(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    state: TextState,
    text: Option<&str>,
) -> QueryResult<MeetingAttachment> {
    let text = text.filter(|_| state == TextState::Read);
    diesel::update(meeting_attachments::table.find(id))
        .set((
            meeting_attachments::text_state.eq(state.as_str()),
            meeting_attachments::text.eq(text),
        ))
        .returning(MeetingAttachment::as_returning())
        .get_result(connection)
        .await
}

/// One attachment as the knowledge index holds it, beside the meeting or the excerpt it is pinned
/// in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedAttachment {
    pub kind: AttachmentKind,
    pub file_name: String,
    /// A frame's caption.
    pub caption: Option<String>,
    /// A file's text, once read.
    pub text: Option<String>,
}

impl From<MeetingAttachment> for IndexedAttachment {
    fn from(attachment: MeetingAttachment) -> Self {
        Self {
            kind: attachment.kind(),
            caption: attachment.caption,
            text: attachment.text,
            file_name: attachment.file_name,
        }
    }
}

#[derive(Debug, QueryableByName)]
struct ExistsRow {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    pinned: bool,
}

#[derive(Debug, QueryableByName)]
struct PinnedRow {
    #[diesel(sql_type = SqlUuid)]
    attachment_id: Uuid,
}

#[derive(Debug, QueryableByName)]
struct PinSpanRow {
    #[diesel(sql_type = BigInt)]
    start_ms: i64,
    #[diesel(sql_type = BigInt)]
    end_ms: i64,
}

/// The standing pins of meeting `$1` whose range overlaps `$2` to `$3` on its clock, with the
/// standing attachment each names, as the tables `marks`, `first` (its first word), `last` (its
/// last word), and `attachments`. A range overlaps the span as a thread's does
/// (`models::meeting_comment`): it starts before the span ends and ends after it starts, a range
/// of one instant counting as a millisecond long.
const PINS_OVER: &str = "
    FROM meeting_marks marks
    JOIN meeting_words first ON first.id = marks.start_word_id
    JOIN meeting_words last ON last.id = marks.end_word_id
    JOIN meeting_attachments attachments
      ON attachments.id::text = marks.data->>'attachmentId'
     AND attachments.meeting_id = marks.meeting_id
    WHERE marks.meeting_id = $1
      AND marks.kind = 'pin'
      AND marks.removed_by IS NULL
      AND attachments.removed_by IS NULL
      AND first.start_ms < $3
      AND GREATEST(last.end_ms, first.start_ms + 1) > $2";

/// Meeting `meeting_id`'s standing attachments pinned over `start_ms` to `end_ms` on its clock,
/// each once, in the order their first pins start: what an excerpt over that span carries beside
/// its own text, to the index and to every agent that reads it.
///
/// # Errors
/// Propagates any database error.
pub async fn pinned_over(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    start_ms: i64,
    end_ms: i64,
) -> QueryResult<Vec<MeetingAttachment>> {
    let rows: Vec<PinnedRow> = diesel::sql_query(format!(
        "SELECT attachments.id AS attachment_id {PINS_OVER}
         GROUP BY attachments.id
         ORDER BY min(first.position), attachments.id"
    ))
    .bind::<SqlUuid, _>(meeting_id)
    .bind::<BigInt, _>(start_ms)
    .bind::<BigInt, _>(end_ms)
    .load(connection)
    .await?;
    let ids: Vec<Uuid> = rows.iter().map(|row| row.attachment_id).collect();
    let mut found: Vec<MeetingAttachment> = meeting_attachments::table
        .filter(meeting_attachments::id.eq_any(&ids))
        .select(MeetingAttachment::as_select())
        .load(connection)
        .await?;
    found.sort_by_key(|attachment| ids.iter().position(|id| *id == attachment.id));
    Ok(found)
}

/// Whether standing attachment `attachment_id` of meeting `meeting_id` is pinned in one of its
/// standing excerpts: a pin of it overlaps an excerpt's words, by the rule [`pinned_over`] carries
/// it with. What is pinned in an excerpt travels with it, so it is read wherever the excerpt is.
///
/// # Errors
/// Propagates any database error.
pub async fn is_pinned_in_an_excerpt(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    attachment_id: Uuid,
) -> QueryResult<bool> {
    let row: ExistsRow = diesel::sql_query(format!(
        "SELECT EXISTS (
             SELECT 1 {PINS_OVER}
               AND attachments.id = $4
               AND EXISTS (
                   SELECT 1
                   FROM meeting_marks excerpts
                   JOIN meeting_words excerpt_first ON excerpt_first.id = excerpts.start_word_id
                   JOIN meeting_words excerpt_last ON excerpt_last.id = excerpts.end_word_id
                   WHERE excerpts.meeting_id = marks.meeting_id
                     AND excerpts.kind = 'excerpt'
                     AND excerpts.removed_by IS NULL
                     AND first.start_ms
                         < GREATEST(excerpt_last.end_ms, excerpt_first.start_ms + 1)
                     AND GREATEST(last.end_ms, first.start_ms + 1) > excerpt_first.start_ms
               )
         ) AS pinned"
    ))
    .bind::<SqlUuid, _>(meeting_id)
    .bind::<BigInt, _>(i64::MIN)
    .bind::<BigInt, _>(i64::MAX)
    .bind::<SqlUuid, _>(attachment_id)
    .get_result(connection)
    .await?;
    Ok(row.pinned)
}

/// Where on its meeting's clock each standing pin of attachment `attachment_id` lies, from its
/// first word's start to its last word's end: the spans whose excerpts a change to the
/// attachment, such as its text being read, indexes again.
///
/// # Errors
/// Propagates any database error.
pub async fn pin_spans(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    attachment_id: Uuid,
) -> QueryResult<Vec<(i64, i64)>> {
    let rows: Vec<PinSpanRow> = diesel::sql_query(format!(
        "SELECT first.start_ms, GREATEST(last.end_ms, first.start_ms) AS end_ms {PINS_OVER}
           AND attachments.id = $4"
    ))
    .bind::<SqlUuid, _>(meeting_id)
    .bind::<BigInt, _>(i64::MIN)
    .bind::<BigInt, _>(i64::MAX)
    .bind::<SqlUuid, _>(attachment_id)
    .load(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.start_ms, row.end_ms))
        .collect())
}
