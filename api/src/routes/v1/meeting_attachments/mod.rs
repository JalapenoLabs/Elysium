// Copyright © 2026 Jalapeno Labs

//! `/api/v1/meeting-attachments`: what every meeting's files and frames share. A meeting's own are
//! under `/api/v1/meetings/{id}/attachments`, and a file arrives as an upload
//! (`/api/v1/meeting-uploads`). See `crate::meeting_attachments` and `docs/meetings.md`,
//! Attachments and frames.

mod get_policy;
mod set_limit;

use axum::Router;
use axum::routing::{get, put};
use serde::Serialize;
use uuid::Uuid;

use crate::images::MAX_FRAME_UPLOAD_BYTES;
use crate::meeting_attachments::{
    MAX_BYTES_CEILING, MAX_CAPTION_CHARACTERS, MAX_PER_MEETING, TEXT_MAX_CHARACTERS,
};
use crate::meeting_uploads::PART_BYTES;
use crate::models::user::WorkspaceSettings;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_policy::handle))
        .route("/limit", put(set_limit::handle))
}

/// What a person can attach to a meeting, which its page says before anyone chooses a file. All
/// but the largest file are constants in code, read here so nothing keeps a second copy.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyResponse {
    /// Where files and frames go: the location page attachments use, `null` until an admin
    /// chooses one, and nothing can be attached while it is.
    destination_id: Option<Uuid>,
    /// The largest file a meeting takes, the admin's setting, and the most it may be set to.
    max_bytes: u64,
    max_bytes_ceiling: u64,
    /// How many bytes each part of a file's upload carries but the last.
    part_bytes: u32,
    /// The most files and frames one meeting holds.
    max_per_meeting: i64,
    /// The largest still a frame is taken from, and the longest its caption.
    frame_max_bytes: usize,
    caption_max_characters: usize,
    /// The most of a file's text read for search, in characters.
    text_max_characters: usize,
}

impl From<WorkspaceSettings> for PolicyResponse {
    fn from(settings: WorkspaceSettings) -> Self {
        Self {
            destination_id: settings.page_attachment_storage_location_id,
            max_bytes: u64::try_from(settings.meeting_attachment_max_bytes).unwrap_or_default(),
            max_bytes_ceiling: MAX_BYTES_CEILING,
            part_bytes: PART_BYTES,
            max_per_meeting: MAX_PER_MEETING,
            frame_max_bytes: MAX_FRAME_UPLOAD_BYTES,
            caption_max_characters: MAX_CAPTION_CHARACTERS,
            text_max_characters: TEXT_MAX_CHARACTERS,
        }
    }
}
