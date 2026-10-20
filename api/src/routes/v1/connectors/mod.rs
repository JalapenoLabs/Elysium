// Copyright © 2026 Jalapeno Labs

//! `/api/v1/connectors`: the services Elysium reads as the signed-in person, each through one
//! of their browsers. See `crate::connectors` and `docs/connectors.md`.
//!
//! An account belongs to whoever owns its browser, and every route answers them alone.

mod create_connector_account;
mod delete_connector_account;
mod list_connector_accounts;

use axum::Router;
use axum::routing::{delete, get};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::models::connector_account::ConnectorAccount;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_connector_accounts::handle).post(create_connector_account::handle),
        )
        .route("/{id}", delete(delete_connector_account::handle))
}

/// An account as its owner sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorAccountResponse {
    id: Uuid,
    kind: String,
    browser_session_id: Uuid,
    /// `signed_in`, `signed_out`, or `unknown`, as last read.
    sign_in_state: String,
    last_polled_at: Option<DateTime<Utc>>,
    /// Why the last read failed, or `null` when it succeeded.
    last_error: Option<String>,
    is_active: bool,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl ConnectorAccountResponse {
    pub fn new(account: ConnectorAccount) -> Self {
        Self {
            id: account.id,
            kind: account.kind,
            browser_session_id: account.browser_session_id,
            sign_in_state: account.sign_in_state,
            last_polled_at: account.last_polled_at,
            last_error: account.last_error,
            is_active: account.is_active,
            created_by: account.created_by,
            created_at: account.created_at,
            updated_at: account.updated_at,
        }
    }
}
