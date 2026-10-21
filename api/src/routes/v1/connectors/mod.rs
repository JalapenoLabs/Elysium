// Copyright © 2026 Jalapeno Labs

//! `/api/v1/connectors`: the services Elysium reads as the signed-in person, each through one
//! of their browsers or with a credential they gave it. See `crate::connectors`,
//! `docs/connectors.md`, and `docs/meetings.md`.
//!
//! An account belongs to whoever created it, and every route answers them alone. A
//! credential is never returned.

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
    /// The browser it reads through, or `null` for an account that signs in with a
    /// credential.
    browser_session_id: Option<Uuid>,
    /// Whether it signs in with a credential Elysium holds.
    has_credential: bool,
    /// Where it keeps files, such as meeting recordings.
    storage_location_id: Option<Uuid>,
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
            has_credential: account.credential_encrypted.is_some(),
            storage_location_id: account.storage_location_id,
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
