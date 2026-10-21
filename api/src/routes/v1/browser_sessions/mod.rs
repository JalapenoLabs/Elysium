// Copyright © 2026 Jalapeno Labs

//! `/api/v1/browser-sessions`: browsers the host runs for a person to sign in to their
//! accounts in, and the stream they watch one through. See `crate::browser` and
//! `docs/browser.md`.
//!
//! A browser belongs to the person who created it. Every route answers its owner alone,
//! and another person's browser is indistinguishable from one that does not exist.

mod create_browser_session;
mod delete_browser_session;
mod get_browser_session;
mod list_browser_sessions;
mod stream_browser_session;

use axum::Router;
use axum::routing::get;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::browser::BrowserStatus;
use crate::models::browser_session::BrowserSession;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_browser_sessions::handle).post(create_browser_session::handle),
        )
        .route(
            "/{id}",
            get(get_browser_session::handle).delete(delete_browser_session::handle),
        )
        .route("/{id}/stream", get(stream_browser_session::handle))
}

/// A browser as its owner sees it. The password is never included, sealed or not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSessionResponse {
    id: Uuid,
    name: String,
    start_url: String,
    /// The host port its media arrives on.
    port: i32,
    status: BrowserStatus,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl BrowserSessionResponse {
    pub fn new(session: BrowserSession, status: BrowserStatus) -> Self {
        Self {
            id: session.id,
            name: session.name,
            start_url: session.start_url,
            port: session.port,
            status,
            created_by: session.created_by,
            created_at: session.created_at,
            updated_at: session.updated_at,
        }
    }
}
