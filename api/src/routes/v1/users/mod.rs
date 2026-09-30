// Copyright © 2026 Jalapeno Labs

//! `/api/v1/users`: everyone who can create something in Elysium, and, for admins, managing
//! people's accounts. See `docs/auth.md`.
//!
//! Every active person may list users, since every row names its creator. Only admins see
//! how people sign in, and only admins change accounts.

mod approve_user;
mod create_recovery_link;
mod list_users;
mod reject_user;
mod reset_mfa;
mod revoke_sessions;
mod update_user;

use axum::Router;
use axum::routing::{delete, get, post};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::models::user::{User, UserRole, UserStatus};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users::handle))
        .route(
            "/{id}",
            delete(reject_user::handle).patch(update_user::handle),
        )
        .route("/{id}/approve", post(approve_user::handle))
        .route("/{id}/revoke-sessions", post(revoke_sessions::handle))
        .route("/{id}/reset-mfa", post(reset_mfa::handle))
        .route("/{id}/recovery-link", post(create_recovery_link::handle))
}

/// Whether a user signs in or acts on its own.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UserKind {
    Person,
    Machine,
}

/// A user as clients see it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    id: Uuid,
    kind: UserKind,
    name: String,
    /// `None` for machines.
    email: Option<String>,
    /// `None` for a person waiting for approval.
    role: Option<UserRole>,
    /// `None` for machines.
    status: Option<UserStatus>,
    approved_at: Option<DateTime<Utc>>,
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    /// How the person can sign in (`password`, `passkey`, `totp`, `lookup_secret`), for admins
    /// only; `None` for everyone else, and for machines.
    #[serde(skip_serializing_if = "Option::is_none")]
    sign_in_methods: Option<Vec<String>>,
}

impl UserResponse {
    pub fn new(user: User, sign_in_methods: Option<Vec<String>>) -> Self {
        let kind = if user.kratos_identity_id.is_some() {
            UserKind::Person
        } else {
            UserKind::Machine
        };
        Self {
            id: user.id,
            kind,
            name: user.name,
            email: user.email,
            role: user.role,
            status: user.status,
            approved_at: user.approved_at,
            last_seen_at: user.last_seen_at,
            created_at: user.created_at,
            updated_at: user.updated_at,
            sign_in_methods,
        }
    }
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self::new(user, None)
    }
}

/// The Kratos identity behind a person, or `NotFound` for a machine: nothing here manages
/// a machine's account.
fn identity_of(user: &User) -> Result<Uuid, crate::errors::ApiError> {
    user.kratos_identity_id
        .ok_or(crate::errors::ApiError::NotFound)
}
