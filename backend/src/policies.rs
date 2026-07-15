//! Per-org authentication policy: the admin control plane over `org_auth_policies`.
//!
//! Enterprise-style controls an org owner/admin sets for every member: which
//! sign-in methods are allowed, whether a second factor is mandatory, and how long
//! a session lives. Reads and writes are admin-gated (see [`crate::authz`]). At
//! least one sign-in method must stay enabled, so a policy change can never lock an
//! entire org out; the same invariant is a database `CHECK`, enforced here first
//! for a clean `400`. Enforcement of these values lives in the auth flow
//! (`password.rs`, `session.rs`) and the session bootstrap (`context.rs`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::authz::{self, OrgRole};
use crate::session::SessionUser;
use crate::state::AppState;

/// Smallest allowed session lifetime (5 minutes): short, but not self-defeating.
const MIN_SESSION_LIFETIME_SECONDS: i32 = 300;

/// Largest allowed session lifetime (90 days), a sane ceiling on "stay signed in".
const MAX_SESSION_LIFETIME_SECONDS: i32 = 90 * 24 * 60 * 60;

/// The stored policy row.
#[derive(sqlx::FromRow)]
struct Policy {
    allow_passkeys: bool,
    allow_passwords: bool,
    allow_magic_links: bool,
    require_totp: bool,
    session_lifetime_seconds: i32,
}

impl Policy {
    /// Renders the policy as its JSON body.
    fn to_json(&self) -> serde_json::Value {
        json!({
            "allow_passkeys": self.allow_passkeys,
            "allow_passwords": self.allow_passwords,
            "allow_magic_links": self.allow_magic_links,
            "require_totp": self.require_totp,
            "session_lifetime_seconds": self.session_lifetime_seconds,
        })
    }
}

/// Loads an org's policy row.
async fn load(state: &AppState, org_id: Uuid) -> sqlx::Result<Option<Policy>> {
    sqlx::query_as::<_, Policy>(
        "SELECT allow_passkeys, allow_passwords, allow_magic_links, require_totp,
                session_lifetime_seconds
           FROM org_auth_policies WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(&state.pool)
    .await
}

/// `GET /api/v1/orgs/:id/auth-policy`: read the org's auth policy (admin+).
pub async fn get_policy(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
) -> Response {
    if let Err(response) = authz::require_org_role(&state, &principal, org_id, OrgRole::Admin).await
    {
        return response;
    }
    match load(&state, org_id).await {
        Ok(Some(policy)) => Json(policy.to_json()).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "auth-policy read failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Body of the update request; every field is optional (a partial update).
#[derive(Deserialize)]
pub struct UpdatePolicy {
    allow_passkeys: Option<bool>,
    allow_passwords: Option<bool>,
    allow_magic_links: Option<bool>,
    require_totp: Option<bool>,
    session_lifetime_seconds: Option<i32>,
}

/// `PATCH /api/v1/orgs/:id/auth-policy`: update the org's auth policy (admin+).
///
/// Merges the sent fields over the current policy, validates the result (at least
/// one sign-in method enabled; session lifetime within bounds), then writes it.
pub async fn update_policy(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
    Json(body): Json<UpdatePolicy>,
) -> Response {
    if let Err(response) = authz::require_org_role(&state, &principal, org_id, OrgRole::Admin).await
    {
        return response;
    }

    let current = match load(&state, org_id).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "auth-policy read failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Merge the partial update over the current values.
    let merged = Policy {
        allow_passkeys: body.allow_passkeys.unwrap_or(current.allow_passkeys),
        allow_passwords: body.allow_passwords.unwrap_or(current.allow_passwords),
        allow_magic_links: body.allow_magic_links.unwrap_or(current.allow_magic_links),
        require_totp: body.require_totp.unwrap_or(current.require_totp),
        session_lifetime_seconds: body
            .session_lifetime_seconds
            .unwrap_or(current.session_lifetime_seconds),
    };

    // At least one sign-in method must remain, or the org locks itself out.
    if !(merged.allow_passkeys || merged.allow_passwords || merged.allow_magic_links) {
        return (
            StatusCode::BAD_REQUEST,
            "at least one sign-in method must stay enabled",
        )
            .into_response();
    }
    if !(MIN_SESSION_LIFETIME_SECONDS..=MAX_SESSION_LIFETIME_SECONDS)
        .contains(&merged.session_lifetime_seconds)
    {
        return (
            StatusCode::BAD_REQUEST,
            "session_lifetime_seconds is out of range",
        )
            .into_response();
    }

    let result = sqlx::query(
        "UPDATE org_auth_policies
            SET allow_passkeys = $1, allow_passwords = $2, allow_magic_links = $3,
                require_totp = $4, session_lifetime_seconds = $5
          WHERE org_id = $6",
    )
    .bind(merged.allow_passkeys)
    .bind(merged.allow_passwords)
    .bind(merged.allow_magic_links)
    .bind(merged.require_totp)
    .bind(merged.session_lifetime_seconds)
    .bind(org_id)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => Json(merged.to_json()).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "auth-policy update failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
