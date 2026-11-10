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

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post, put};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::meeting_uploads::PART_BYTES;
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
