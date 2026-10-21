// Copyright © 2026 Jalapeno Labs

//! `/api/v1/coding-sessions`: agents working on a project's repositories, and the turns sent
//! to them.
//!
//! Each turn runs on the runner through the sessions service (`crate::sessions`), which keeps
//! the session's directory, events, and activity.

mod create_coding_session;
mod delete_coding_session;
mod first_turn;
pub mod github_token;
mod list_coding_sessions;
mod list_session_events;
mod rename_coding_session;
mod start_turn;
mod watch_desktop;

pub use create_coding_session::{CreateCodingSessionRequest, create};
pub use list_coding_sessions::{find, list};
pub use list_session_events::{EventWindow, Keep, read_events};
pub use rename_coding_session::{RenameCodingSessionRequest, rename};
pub use start_turn::{StartTurnRequest, start_turn};

use axum::Router;
use axum::routing::{get, patch, post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use validator::ValidationError;

use crate::models::coding_session::CodingSession;
use crate::sessions::views::SessionActivity;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_coding_sessions::handle).post(create_coding_session::handle),
        )
        .route(
            "/{id}",
            patch(rename_coding_session::handle).delete(delete_coding_session::handle),
        )
        .route("/{id}/events", get(list_session_events::handle))
        .route("/{id}/turns", post(start_turn::handle))
        .route("/{id}/desktop", get(watch_desktop::handle))
}

/// A session as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionResponse {
    id: i64,
    project_id: Option<Uuid>,
    title: String,
    github_credential_id: Option<Uuid>,
    action_item_id: Option<Uuid>,
    studio_item_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    created_by: Uuid,
    updated_at: DateTime<Utc>,
    /// What the session is doing now.
    activity: SessionActivity,
}

impl CodingSessionResponse {
    pub fn new(session: CodingSession, activity: SessionActivity) -> Self {
        Self {
            id: session.id,
            project_id: session.project_id,
            title: session.title,
            github_credential_id: session.github_credential_id,
            action_item_id: session.action_item_id,
            studio_item_id: session.studio_item_id,
            created_at: session.created_at,
            created_by: session.created_by,
            updated_at: session.updated_at,
            activity,
        }
    }
}

/// Rejects text that is only whitespace; `length` alone would accept `"   "`.
pub fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}
