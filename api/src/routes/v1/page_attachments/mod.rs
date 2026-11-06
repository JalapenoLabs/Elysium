// Copyright © 2026 Jalapeno Labs

//! `/api/v1/page-attachments`: what every page's attachments share. A page's own are under
//! `/api/v1/pages/{id}/attachments`. See `docs/knowledge.md`, Pages, Attachments.

mod choose_destination;
mod get_policy;

use axum::Router;
use axum::routing::{get, put};
use serde::Serialize;
use uuid::Uuid;

use crate::models::user::WorkspaceSettings;
use crate::page_attachments::{MAX_ATTACHMENT_BYTES, accepted_extensions};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_policy::handle))
        .route("/destination", put(choose_destination::handle))
}

/// What a person can attach, which the web app says before anyone chooses a file. The limits are
/// constants in code, and read here so nothing keeps a second copy.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyResponse {
    /// The storage location new attachments go to; `null` until an admin chooses one, and nothing
    /// can be attached while it is.
    destination_id: Option<Uuid>,
    max_bytes: u64,
    /// Lowercase, without their dots.
    extensions: Vec<&'static str>,
}

impl From<WorkspaceSettings> for PolicyResponse {
    fn from(settings: WorkspaceSettings) -> Self {
        Self {
            destination_id: settings.page_attachment_storage_location_id,
            max_bytes: MAX_ATTACHMENT_BYTES,
            extensions: accepted_extensions(),
        }
    }
}
