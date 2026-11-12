// Copyright © 2026 Jalapeno Labs

//! Meetings people upload, while their parts arrive and once they became a capture. The rules for
//! what an upload takes are `crate::meeting_uploads`; see `docs/meetings.md`, Uploads.
//!
//! An upload is one row and a row per file, a recording, a transcript, or one of each. A file's
//! `received_bytes` is how much of it arrived in whole parts from its start, and moves only from
//! the value a writer read ([`record_part`]), so two requests carrying the same part count it
//! once. Finishing ([`record_finished`]) names the capture the upload became and keeps the lines
//! its transcript was read as, for the meeting's fetch to store; an upload is finished once.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::dsl::{exists, not, sql};
use diesel::prelude::*;
use diesel::sql_types::Text;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::{
    meeting_attachments, meeting_captures, meeting_upload_files, meeting_uploads, workflow_runs,
};
use crate::meeting_uploads::FileKind;
use crate::meetings::{attachment_workflow, workflow};
use crate::models::workflow_run::WorkflowRunState;

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = meeting_uploads, check_for_backend(diesel::pg::Pg))]
pub struct MeetingUpload {
    pub id: Uuid,
    /// The meeting it adds a capture to; `None` for a new meeting.
    pub meeting_id: Option<Uuid>,
    /// A new meeting's title and when it started.
    pub title: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    /// Where its recording is kept; `None` for a transcript alone.
    pub storage_location_id: Option<Uuid>,
    pub part_bytes: i32,
    /// The lines its transcript was read as, set when it finished.
    pub transcript: Option<Value>,
    /// The capture it became, once it finished.
    pub capture_id: Option<Uuid>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// When its staged parts were removed from the workspace; `None` while they may be there.
    pub parts_removed_at: Option<DateTime<Utc>>,
    /// The attachment it became, once an upload of a file attached to its meeting finished.
    pub attachment_id: Option<Uuid>,
}

impl MeetingUpload {
    /// How many bytes each part carries but the last.
    pub fn part_bytes(&self) -> u32 {
        u32::try_from(self.part_bytes).expect("meeting_uploads_part_bytes_check keeps it positive")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = meeting_upload_files, check_for_backend(diesel::pg::Pg))]
pub struct MeetingUploadFile {
    pub upload_id: Uuid,
    /// See [`FileKind`].
    pub kind: String,
    pub file_name: String,
    pub size_bytes: i64,
    pub modified_at: Option<DateTime<Utc>>,
    pub received_bytes: i64,
}

impl MeetingUploadFile {
    /// What the file is.
    ///
    /// # Panics
    /// Panics on a kind `meeting_upload_files_kind_check` does not allow, which only a migration
    /// that drifted from [`FileKind`] can cause.
    pub fn kind(&self) -> FileKind {
        FileKind::parse(&self.kind).expect("meeting_upload_files_kind_check allows only FileKind")
    }

    pub fn size_bytes(&self) -> u64 {
        u64::try_from(self.size_bytes)
            .expect("meeting_upload_files_size_bytes_check keeps it positive")
    }

    pub fn received_bytes(&self) -> u64 {
        u64::try_from(self.received_bytes)
            .expect("meeting_upload_files_received_bytes_check keeps it at zero or more")
    }

    /// Whether every byte of it arrived.
    pub fn is_received(&self) -> bool {
        self.received_bytes == self.size_bytes
    }
}

/// An upload with its files, the recording first.
#[derive(Debug, Clone)]
pub struct Staged {
    pub upload: MeetingUpload,
    pub files: Vec<MeetingUploadFile>,
}

impl Staged {
    /// Its file of `kind`, if it has one.
    pub fn file(&self, kind: FileKind) -> Option<&MeetingUploadFile> {
        self.files.iter().find(|file| file.kind() == kind)
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_uploads)]
pub struct NewMeetingUpload {
    pub id: Uuid,
    pub meeting_id: Option<Uuid>,
    pub title: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub storage_location_id: Option<Uuid>,
    pub part_bytes: i32,
    pub created_by: Uuid,
}

/// One file an upload will carry.
#[derive(Debug, Clone)]
pub struct NewUploadFile {
    pub kind: FileKind,
    pub file_name: String,
    pub size_bytes: i64,
    pub modified_at: Option<DateTime<Utc>>,
}

#[derive(Insertable)]
#[diesel(table_name = meeting_upload_files)]
struct FileRow {
    upload_id: Uuid,
    kind: &'static str,
    file_name: String,
    size_bytes: i64,
    modified_at: Option<DateTime<Utc>>,
}

/// Stores `upload` with `files`, together.
///
/// # Errors
/// Propagates any database error, such as a meeting or storage location that does not exist.
pub async fn create(
    connection: &mut AsyncPgConnection,
    upload: NewMeetingUpload,
    files: Vec<NewUploadFile>,
) -> QueryResult<Staged> {
    connection
        .transaction(async move |connection| {
            let stored: MeetingUpload = diesel::insert_into(meeting_uploads::table)
                .values(upload)
                .returning(MeetingUpload::as_returning())
                .get_result(connection)
                .await?;
            let rows: Vec<FileRow> = files
                .into_iter()
                .map(|file| FileRow {
                    upload_id: stored.id,
                    kind: file.kind.as_str(),
                    file_name: file.file_name,
                    size_bytes: file.size_bytes,
                    modified_at: file.modified_at,
                })
                .collect();
            diesel::insert_into(meeting_upload_files::table)
                .values(rows)
                .execute(connection)
                .await?;
            let files = files_of(connection, stored.id).await?;
            Ok(Staged {
                upload: stored,
                files,
            })
        })
        .await
}

/// Upload `id`, when `owner` uploads it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist or is someone else's.
pub async fn find_owned(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<Staged> {
    let upload: MeetingUpload = meeting_uploads::table
        .filter(meeting_uploads::id.eq(id))
        .filter(meeting_uploads::created_by.eq(owner))
        .select(MeetingUpload::as_select())
        .first(connection)
        .await?;
    let files = files_of(connection, upload.id).await?;
    Ok(Staged { upload, files })
}

/// Upload `id`, when `owner` uploads it and it has not finished, locked until the transaction
/// ends, so it finishes once.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist, is someone else's, or finished.
pub async fn lock_unfinished(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<Staged> {
    let upload: MeetingUpload = meeting_uploads::table
        .filter(meeting_uploads::id.eq(id))
        .filter(meeting_uploads::created_by.eq(owner))
        .filter(meeting_uploads::finished_at.is_null())
        .for_update()
        .select(MeetingUpload::as_select())
        .first(connection)
        .await?;
    let files = files_of(connection, upload.id).await?;
    Ok(Staged { upload, files })
}

/// `owner`'s unfinished uploads, the most recently moved first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_unfinished(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
) -> QueryResult<Vec<Staged>> {
    let uploads: Vec<MeetingUpload> = meeting_uploads::table
        .filter(meeting_uploads::created_by.eq(owner))
        .filter(meeting_uploads::finished_at.is_null())
        .order(meeting_uploads::updated_at.desc())
        .select(MeetingUpload::as_select())
        .load(connection)
        .await?;
    let ids: Vec<Uuid> = uploads.iter().map(|upload| upload.id).collect();
    let files: Vec<MeetingUploadFile> = meeting_upload_files::table
        .filter(meeting_upload_files::upload_id.eq_any(&ids))
        .order(meeting_upload_files::kind)
        .select(MeetingUploadFile::as_select())
        .load(connection)
        .await?;
    let mut files_by_upload: HashMap<Uuid, Vec<MeetingUploadFile>> = HashMap::new();
    for file in files {
        files_by_upload
            .entry(file.upload_id)
            .or_default()
            .push(file);
    }
    Ok(uploads
        .into_iter()
        .map(|upload| {
            let files = files_by_upload.remove(&upload.id).unwrap_or_default();
            Staged { upload, files }
        })
        .collect())
}

/// The upload capture `capture_id` was made from, if it was one.
///
/// # Errors
/// Propagates any database error.
pub async fn of_capture(
    connection: &mut AsyncPgConnection,
    capture_id: Uuid,
) -> QueryResult<Option<Staged>> {
    let upload: Option<MeetingUpload> = meeting_uploads::table
        .filter(meeting_uploads::capture_id.eq(capture_id))
        .select(MeetingUpload::as_select())
        .first(connection)
        .await
        .optional()?;
    let Some(upload) = upload else {
        return Ok(None);
    };
    let files = files_of(connection, upload.id).await?;
    Ok(Some(Staged { upload, files }))
}

/// Upload `upload_id`'s file of `kind`, locked until the transaction ends, so one request at a time
/// decides whether a send of its next part is the one counted.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the upload has no such file.
pub async fn lock_file(
    connection: &mut AsyncPgConnection,
    (upload_id, kind): (Uuid, FileKind),
) -> QueryResult<MeetingUploadFile> {
    meeting_upload_files::table
        .filter(meeting_upload_files::upload_id.eq(upload_id))
        .filter(meeting_upload_files::kind.eq(kind.as_str()))
        .for_update()
        .select(MeetingUploadFile::as_select())
        .first(connection)
        .await
}

/// Counts a part of upload `upload_id`'s file of `kind` as arrived, moving its received bytes
/// from `from` to `to`, and marks the upload as moved. Answers `false`, changing nothing, when
/// another request counted it first.
///
/// # Errors
/// Propagates any database error.
pub async fn record_part(
    connection: &mut AsyncPgConnection,
    (upload_id, kind): (Uuid, FileKind),
    from: i64,
    to: i64,
) -> QueryResult<bool> {
    connection
        .transaction(async move |connection| {
            let moved = diesel::update(
                meeting_upload_files::table
                    .filter(meeting_upload_files::upload_id.eq(upload_id))
                    .filter(meeting_upload_files::kind.eq(kind.as_str()))
                    .filter(meeting_upload_files::received_bytes.eq(from)),
            )
            .set(meeting_upload_files::received_bytes.eq(to))
            .execute(connection)
            .await?;
            if moved == 0 {
                return Ok(false);
            }
            // What the stale sweep reads: an upload that is still arriving is not abandoned.
            diesel::update(meeting_uploads::table.find(upload_id))
                .set(meeting_uploads::updated_at.eq(diesel::dsl::now))
                .execute(connection)
                .await?;
            Ok(true)
        })
        .await
}

/// Marks unfinished upload `id` finished as capture `capture_id`, keeping `transcript`, the lines
/// its transcript was read as. Answers `None`, changing nothing, when it finished already.
///
/// # Errors
/// Propagates any database error.
pub async fn record_finished(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    capture_id: Uuid,
    transcript: Option<Value>,
) -> QueryResult<Option<MeetingUpload>> {
    diesel::update(meeting_uploads::table.find(id))
        .filter(meeting_uploads::finished_at.is_null())
        .set((
            meeting_uploads::capture_id.eq(Some(capture_id)),
            meeting_uploads::transcript.eq(transcript),
            meeting_uploads::finished_at.eq(Some(Utc::now())),
        ))
        .returning(MeetingUpload::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// Marks unfinished upload `id` finished as attachment `attachment_id`. Answers `None`, changing
/// nothing, when it finished already.
///
/// # Errors
/// Propagates any database error.
pub async fn record_attached(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    attachment_id: Uuid,
) -> QueryResult<Option<MeetingUpload>> {
    diesel::update(meeting_uploads::table.find(id))
        .filter(meeting_uploads::finished_at.is_null())
        .set((
            meeting_uploads::attachment_id.eq(Some(attachment_id)),
            meeting_uploads::finished_at.eq(Some(Utc::now())),
        ))
        .returning(MeetingUpload::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// Deletes `owner`'s unfinished upload `id` and its files' rows. Answers whether there was one.
///
/// # Errors
/// Propagates any database error.
pub async fn delete_unfinished(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<bool> {
    let deleted = diesel::delete(
        meeting_uploads::table
            .filter(meeting_uploads::id.eq(id))
            .filter(meeting_uploads::created_by.eq(owner))
            .filter(meeting_uploads::finished_at.is_null()),
    )
    .execute(connection)
    .await?;
    Ok(deleted > 0)
}

/// The finished uploads whose parts may still be in the workspace and are no longer needed: those
/// whose capture was stored, and any finished before `before` whose meeting's run settled without
/// storing it, such as one whose fetch a person skipped. A run still going, held, or failed and
/// waiting for a person to retry its fetch keeps the parts that fetch reads, however old the
/// upload. At most `limit`, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn finished_with_parts(
    connection: &mut AsyncPgConnection,
    before: DateTime<Utc>,
    limit: i64,
) -> QueryResult<Vec<Uuid>> {
    // `meetings::workflow::live_run`, for the meeting of the capture each row names.
    let live_meeting_run = workflow_runs::table
        .filter(workflow_runs::workflow.eq(workflow::NAME))
        .filter(workflow_runs::source_kind.eq(workflow::SOURCE_KIND))
        .filter(workflow_runs::source_id.eq(sql::<Text>("meeting_captures.meeting_id::text")))
        .filter(workflow_runs::state.ne(WorkflowRunState::Succeeded));
    meeting_uploads::table
        .inner_join(
            meeting_captures::table.on(meeting_captures::id
                .nullable()
                .eq(meeting_uploads::capture_id)),
        )
        .filter(meeting_uploads::finished_at.is_not_null())
        .filter(meeting_uploads::parts_removed_at.is_null())
        .filter(
            meeting_captures::transcript_hash
                .is_not_null()
                .or(meeting_uploads::finished_at
                    .lt(before)
                    .and(not(exists(live_meeting_run)))),
        )
        .order(meeting_uploads::finished_at.asc())
        .limit(limit)
        .select(meeting_uploads::id)
        .load(connection)
        .await
}

/// The finished uploads of attached files whose parts may still be in the workspace and are no
/// longer needed: those whose file was stored, whose store could not remove them, and any finished
/// before `before` whose attachment's run settled without storing it, such as one a person
/// skipped. A run still going, or failed and waiting for a person to retry its store, keeps the
/// parts it reads. At most `limit`, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn finished_attachments_with_parts(
    connection: &mut AsyncPgConnection,
    before: DateTime<Utc>,
    limit: i64,
) -> QueryResult<Vec<Uuid>> {
    let live_attachment_run = workflow_runs::table
        .filter(workflow_runs::workflow.eq(attachment_workflow::NAME))
        .filter(workflow_runs::source_kind.eq(attachment_workflow::SOURCE_KIND))
        .filter(workflow_runs::source_id.eq(sql::<Text>("meeting_attachments.id::text")))
        .filter(workflow_runs::state.ne(WorkflowRunState::Succeeded));
    meeting_uploads::table
        .inner_join(
            meeting_attachments::table.on(meeting_attachments::id
                .nullable()
                .eq(meeting_uploads::attachment_id)),
        )
        .filter(meeting_uploads::finished_at.is_not_null())
        .filter(meeting_uploads::parts_removed_at.is_null())
        .filter(
            meeting_attachments::stored_at
                .is_not_null()
                .or(meeting_uploads::finished_at
                    .lt(before)
                    .and(not(exists(live_attachment_run)))),
        )
        .order(meeting_uploads::finished_at.asc())
        .limit(limit)
        .select(meeting_uploads::id)
        .load(connection)
        .await
}

/// Records that upload `id`'s staged parts were removed.
///
/// # Errors
/// Propagates any database error.
pub async fn record_parts_removed(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    diesel::update(meeting_uploads::table.find(id))
        .set(meeting_uploads::parts_removed_at.eq(Some(Utc::now())))
        .execute(connection)
        .await?;
    Ok(())
}

/// Deletes every unfinished upload nothing moved since `before`, answering their ids, so their
/// staged parts can be removed too.
///
/// # Errors
/// Propagates any database error.
pub async fn delete_stale(
    connection: &mut AsyncPgConnection,
    before: DateTime<Utc>,
) -> QueryResult<Vec<Uuid>> {
    diesel::delete(
        meeting_uploads::table
            .filter(meeting_uploads::finished_at.is_null())
            .filter(meeting_uploads::updated_at.lt(before)),
    )
    .returning(meeting_uploads::id)
    .get_results(connection)
    .await
}

async fn files_of(
    connection: &mut AsyncPgConnection,
    upload_id: Uuid,
) -> QueryResult<Vec<MeetingUploadFile>> {
    // `recording` sorts before `transcript`, so the recording comes first.
    meeting_upload_files::table
        .filter(meeting_upload_files::upload_id.eq(upload_id))
        .order(meeting_upload_files::kind)
        .select(MeetingUploadFile::as_select())
        .load(connection)
        .await
}
