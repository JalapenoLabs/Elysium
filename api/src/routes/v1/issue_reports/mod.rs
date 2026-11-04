// Copyright © 2026 Jalapeno Labs

//! `/api/v1/issue-reports`: report a problem to Elysium's GitHub repository, as yourself.
//!
//! A preview renders exactly what would be posted, and lists the person's own GitHub tokens
//! with whether each can file it. Filing posts the same rendering. See `crate::issue_reports`
//! and `docs/issue-reporting.md`.

mod create_issue_report;
mod preview_issue_report;

use axum::Router;
use axum::http::HeaderMap;
use axum::http::header::USER_AGENT;
use axum::routing::post;
use serde::Deserialize;

use crate::errors::ApiError;
use crate::issue_reports::draft::{Environment, NOTE_MAX_CHARACTERS};
use crate::issue_reports::subject::Subject;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_issue_report::handle))
        .route("/preview", post(preview_issue_report::handle))
}

/// What a report is about, and what its person adds, as both routes take them.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    subject: Subject,
    /// What the person was doing, or anything else they want to say. Redacted like the rest.
    #[serde(default)]
    note: Option<String>,
}

impl Report {
    /// The note, trimmed, or `None` for none.
    ///
    /// # Errors
    /// [`ApiError::BadRequest`] for a note longer than a report carries.
    fn note(&self) -> Result<Option<&str>, ApiError> {
        let note = self
            .note
            .as_deref()
            .map(str::trim)
            .filter(|note| !note.is_empty());
        if note.is_some_and(|note| note.chars().count() > NOTE_MAX_CHARACTERS) {
            return Err(ApiError::BadRequest(format!(
                "note must hold at most {NOTE_MAX_CHARACTERS} characters"
            )));
        }
        Ok(note)
    }
}

/// The build that is running and the browser asking, from the request's `User-Agent`.
fn environment(state: &AppState, headers: &HeaderMap) -> Environment {
    let user_agent = headers
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok());
    crate::issue_reports::environment(&state.version, user_agent)
}
