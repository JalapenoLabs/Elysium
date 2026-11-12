// Copyright © 2026 Jalapeno Labs

//! `/api/v1/meeting-uploads`: meetings people upload, a recording, a transcript, or both, as a new
//! meeting or a capture of one they have. See `crate::meeting_uploads` and `docs/meetings.md`,
//! Uploads.
//!
//! An upload is made with its files' names and sizes, then each file is sent in parts, in order,
//! and finishing it makes the meeting or the capture and starts the meeting's run. Every route
//! answers the person who uploads alone, as every meeting route does.

mod cancel_upload;
mod create_upload;
mod finish_upload;
mod get_policy;
mod get_upload;
mod list_uploads;
mod upload_part;

#[cfg(test)]
mod tests;

/// How a meeting's live run refuses a change that would start another, which Done reviewing
/// answers too.
pub(crate) use create_upload::meeting_busy;

use anyhow::Context;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post, put};
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use serde::Serialize;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::meeting_uploads::staging::Staging;
use crate::meeting_uploads::transcript::{self, TranscriptError};
use crate::meeting_uploads::{self, FileKind, PART_BYTES};
use crate::meetings::source::TranscriptLine;
use crate::models::meeting_upload::{MeetingUploadFile, Staged};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    // A part streams to its staging directory, so its route takes a whole part; nothing else here
    // takes more than the API's ordinary limit.
    let parts = Router::new()
        .route("/{id}/{kind}/parts/{index}", put(upload_part::handle))
        .layer(DefaultBodyLimit::max(
            usize::try_from(PART_BYTES).expect("a part's size fits in memory"),
        ));
    Router::new()
        .route("/", get(list_uploads::handle).post(create_upload::handle))
        .route("/policy", get(get_policy::handle))
        .route(
            "/{id}",
            get(get_upload::handle).delete(cancel_upload::handle),
        )
        .route("/{id}/finish", post(finish_upload::handle))
        .merge(parts)
}

/// An upload as its person sees it: what it adds to, its files and how much of each arrived, and
/// the capture it became once it finished.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResponse {
    id: Uuid,
    meeting_id: Option<Uuid>,
    title: Option<String>,
    started_at: Option<DateTime<Utc>>,
    storage_location_id: Option<Uuid>,
    /// How many bytes each part carries but the last.
    part_bytes: u32,
    files: Vec<UploadFileResponse>,
    capture_id: Option<Uuid>,
    finished_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFileResponse {
    /// `recording` or `transcript`.
    kind: &'static str,
    file_name: String,
    size_bytes: u64,
    modified_at: Option<DateTime<Utc>>,
    received_bytes: u64,
}

impl From<&MeetingUploadFile> for UploadFileResponse {
    fn from(file: &MeetingUploadFile) -> Self {
        Self {
            kind: file.kind().as_str(),
            file_name: file.file_name.clone(),
            size_bytes: file.size_bytes(),
            modified_at: file.modified_at,
            received_bytes: file.received_bytes(),
        }
    }
}

impl From<Staged> for UploadResponse {
    fn from(staged: Staged) -> Self {
        let files = staged.files.iter().map(UploadFileResponse::from).collect();
        let upload = staged.upload;
        Self {
            part_bytes: upload.part_bytes(),
            id: upload.id,
            meeting_id: upload.meeting_id,
            title: upload.title,
            started_at: upload.started_at,
            storage_location_id: upload.storage_location_id,
            files,
            capture_id: upload.capture_id,
            finished_at: upload.finished_at,
            created_at: upload.created_at,
            updated_at: upload.updated_at,
        }
    }
}

/// The lines `staged`'s transcript says, read whole from its parts in `staging`, or `None` when it
/// uploads a recording alone. Read when the transcript's last part arrives, so one that cannot be
/// read is refused before a recording sent after it, and again when the upload finishes.
///
/// # Errors
/// `400` with `meeting_upload_not_text`, `meeting_upload_nothing_said`, or
/// `meeting_upload_too_many_lines`, and the workspace's refusal.
pub async fn read_transcript(
    staging: &Staging,
    staged: &Staged,
) -> Result<Option<Vec<TranscriptLine>>, ApiError> {
    let Some(file) = staged.file(FileKind::Transcript) else {
        return Ok(None);
    };
    let format = meeting_uploads::type_of(FileKind::Transcript, &file.file_name)
        .context("a stored file's name keeps the extension it was taken by")?
        .format();
    let parts = meeting_uploads::part_count(file.size_bytes(), staged.upload.part_bytes());
    // At most `MAX_TRANSCRIPT_BYTES`, which `create_upload` holds every transcript to.
    let bytes: Vec<u8> = staging
        .read((staged.upload.id, FileKind::Transcript), parts)
        .try_fold(Vec::new(), |mut bytes, chunk| async move {
            bytes.extend_from_slice(&chunk);
            Ok(bytes)
        })
        .await?;
    let lines = transcript::read(&bytes, format).map_err(|error| {
        let code = match error {
            TranscriptError::NotText => "meeting_upload_not_text",
            TranscriptError::Empty => "meeting_upload_nothing_said",
            TranscriptError::TooLong => "meeting_upload_too_many_lines",
        };
        ApiError::CodedBadRequest {
            code,
            message: error.to_string(),
        }
    })?;
    Ok(Some(lines))
}
