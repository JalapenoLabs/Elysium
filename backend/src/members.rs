//! Organization member management: view the roster, change a role, remove a member.
//!
//! Admin-gated management on top of the org RBAC guard (see [`crate::authz`]).
//! Viewing the roster is open to any member; mutations require admin or owner. To
//! prevent escalation and coups, an actor may only manage members strictly below
//! their own role and may never grant a role above their own, so owners cannot be
//! demoted or removed here (nobody outranks them) and admins can only manage
//! members. Ownership transfer and self-removal (leaving an org) are separate
//! flows. See `docs/authz.md`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::audit;
use crate::authz::{self, OrgRole};
use crate::events;
use crate::session::{Principal, SessionUser};
use crate::state::AppState;
use crate::users;

/// `GET /api/v1/orgs/:id/members`: the org roster. Any member may view it.
pub async fn list(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
) -> Response {
    if let Err(response) =
        authz::require_org_role(&state, &principal, org_id, OrgRole::Member).await
    {
        return response;
    }

    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            String,
            String,
            String,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
        ),
    >(
        "SELECT u.id, u.email, u.display_name, u.avatar_url, m.role, m.joined_at, m.suspended_at
           FROM org_members m
           JOIN users u ON u.id = m.user_id
          WHERE m.org_id = $1
          ORDER BY m.joined_at ASC",
    )
    .bind(org_id)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => Json(json!({
            "members": rows
                .iter()
                .map(|(id, email, display_name, avatar_url, role, joined_at, suspended_at)| {
                    json!({
                        "user_id": id,
                        "email": email,
                        "display_name": display_name,
                        "avatar_url": avatar_url,
                        "role": role,
                        "joined_at": joined_at.to_rfc3339(),
                        "suspended": suspended_at.is_some(),
                    })
                })
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "member list failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Body of the change-role request.
#[derive(Deserialize)]
pub struct ChangeRole {
    role: String,
}

/// `PATCH /api/v1/orgs/:id/members/:user_id`: change a member's role.
///
/// The actor must outrank the target and may not grant a role above their own.
pub async fn change_role(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path((org_id, target_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ChangeRole>,
) -> Response {
    let Some(new_role) = OrgRole::from_db(&body.role) else {
        return (StatusCode::BAD_REQUEST, "invalid role").into_response();
    };

    let (actor_id, actor_role) = match authorize_manager(&state, &principal, org_id).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    // Manageable only if the target ranks strictly below the actor, and the new
    // role does not exceed the actor's own (no escalation, no self-change).
    let target_role = match manageable_target(&state, org_id, target_id, actor_id, actor_role).await
    {
        Ok(role) => role,
        Err(response) => return response,
    };
    if new_role > actor_role {
        return (StatusCode::FORBIDDEN, "cannot grant a role above your own").into_response();
    }
    if new_role == target_role {
        return StatusCode::NO_CONTENT.into_response();
    }

    let result = sqlx::query("UPDATE org_members SET role = $1 WHERE user_id = $2 AND org_id = $3")
        .bind(new_role.as_db())
        .bind(target_id)
        .bind(org_id)
        .execute(&state.pool)
        .await;
    if let Err(error) = result {
        tracing::error!(error = %error, "member role update failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    audit::record(
        &state.pool,
        org_id,
        actor_id,
        "member.role_changed",
        Some(target_id),
        json!({ "role": new_role.as_db() }),
    )
    .await;
    // Nudge the affected member's clients to re-read their context (role changed).
    notify_membership_changed(&state, target_id).await;
    StatusCode::NO_CONTENT.into_response()
}

/// `DELETE /api/v1/orgs/:id/members/:user_id`: remove a member from the org.
pub async fn remove(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path((org_id, target_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let (actor_id, actor_role) = match authorize_manager(&state, &principal, org_id).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    if let Err(response) = manageable_target(&state, org_id, target_id, actor_id, actor_role).await
    {
        return response;
    }

    let result = sqlx::query("DELETE FROM org_members WHERE user_id = $1 AND org_id = $2")
        .bind(target_id)
        .bind(org_id)
        .execute(&state.pool)
        .await;
    if let Err(error) = result {
        tracing::error!(error = %error, "member removal failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    audit::record(
        &state.pool,
        org_id,
        actor_id,
        "member.removed",
        Some(target_id),
        json!({}),
    )
    .await;
    // The removed member lost access to this org; nudge their clients to re-read.
    notify_membership_changed(&state, target_id).await;
    StatusCode::NO_CONTENT.into_response()
}

/// `POST /api/v1/orgs/:id/members/:user_id/revoke-sessions`: force sign-out a member.
///
/// Deletes every session the target is part of and force-drops their live SSE, so a
/// revoked member is signed out everywhere at once. Same admin + outrank rules as
/// the other mutations.
pub async fn revoke_sessions(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path((org_id, target_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let (actor_id, actor_role) = match authorize_manager(&state, &principal, org_id).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    if let Err(response) = manageable_target(&state, org_id, target_id, actor_id, actor_role).await
    {
        return response;
    }

    let deleted = sqlx::query(
        "DELETE FROM sessions
          WHERE id IN (SELECT session_id FROM session_accounts WHERE user_id = $1)",
    )
    .bind(target_id)
    .execute(&state.pool)
    .await;
    if let Err(error) = deleted {
        tracing::error!(error = %error, "session revocation failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    audit::record(
        &state.pool,
        org_id,
        actor_id,
        "member.sessions_revoked",
        Some(target_id),
        json!({}),
    )
    .await;
    // Cut the target's live realtime immediately, then announce them offline.
    if let Ok(Some(subject)) = users::subject_for_id(&state.pool, target_id).await {
        state.sse.disconnect(&subject);
        events::announce_presence(&state, subject, false);
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `POST /api/v1/orgs/:id/members/:user_id/suspend`: suspend a member's access.
pub async fn suspend(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path((org_id, target_id)): Path<(Uuid, Uuid)>,
) -> Response {
    set_suspended(&state, &principal, org_id, target_id, true).await
}

/// `POST /api/v1/orgs/:id/members/:user_id/reactivate`: restore a member's access.
pub async fn reactivate(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path((org_id, target_id)): Path<(Uuid, Uuid)>,
) -> Response {
    set_suspended(&state, &principal, org_id, target_id, false).await
}

/// Suspends (or reactivates) a member; the guard does the rest live.
///
/// A suspended member keeps their row but the RBAC guard treats them as having no
/// access, and realtime fan-out already excludes them, so no session teardown is
/// needed; a `session.changed` nudge routes their client to `/suspended`.
async fn set_suspended(
    state: &AppState,
    principal: &Principal,
    org_id: Uuid,
    target_id: Uuid,
    suspend: bool,
) -> Response {
    let (actor_id, actor_role) = match authorize_manager(state, principal, org_id).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    if let Err(response) = manageable_target(state, org_id, target_id, actor_id, actor_role).await {
        return response;
    }

    let result =
        sqlx::query("UPDATE org_members SET suspended_at = $1 WHERE user_id = $2 AND org_id = $3")
            .bind(suspend.then(Utc::now))
            .bind(target_id)
            .bind(org_id)
            .execute(&state.pool)
            .await;
    if let Err(error) = result {
        tracing::error!(error = %error, "member suspension update failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let action = if suspend {
        "member.suspended"
    } else {
        "member.reactivated"
    };
    audit::record(
        &state.pool,
        org_id,
        actor_id,
        action,
        Some(target_id),
        json!({}),
    )
    .await;
    notify_membership_changed(state, target_id).await;
    StatusCode::NO_CONTENT.into_response()
}

/// Resolves the actor and requires they are at least an admin of `org_id`.
///
/// # Errors
/// Returns a ready `401`/`403`/`503` response when the actor is unknown, not a
/// sufficiently-privileged member, or the lookup fails.
async fn authorize_manager(
    state: &AppState,
    principal: &Principal,
    org_id: Uuid,
) -> Result<(Uuid, OrgRole), Response> {
    let actor_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "actor lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    match authz::role_in_org(&state.pool, actor_id, org_id).await {
        Ok(Some(role)) if role >= OrgRole::Admin => Ok((actor_id, role)),
        Ok(_) => Err(StatusCode::FORBIDDEN.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "actor role lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Confirms `target_id` is a member of `org_id` the actor may manage.
///
/// A target the actor does not strictly outrank (including the actor themselves,
/// who cannot rank below themselves) is a `403`, indistinguishable from a target
/// that is not a member, so the endpoint never reveals another org's membership.
///
/// # Errors
/// Returns a ready `403`/`503` response, or `404` when there is no such member.
async fn manageable_target(
    state: &AppState,
    org_id: Uuid,
    target_id: Uuid,
    actor_id: Uuid,
    actor_role: OrgRole,
) -> Result<OrgRole, Response> {
    if target_id == actor_id {
        return Err((StatusCode::FORBIDDEN, "cannot manage your own membership").into_response());
    }
    // `membership` (not `role_in_org`) so a suspended target stays visible to be
    // reactivated or otherwise managed.
    match authz::membership(&state.pool, target_id, org_id).await {
        Ok(Some(entry)) if entry.role < actor_role => Ok(entry.role),
        Ok(Some(_)) => Err(StatusCode::FORBIDDEN.into_response()),
        Ok(None) => Err(StatusCode::NOT_FOUND.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "target role lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Nudges a member's clients to re-read their session context after a change.
async fn notify_membership_changed(state: &AppState, user_id: Uuid) {
    match users::subject_for_id(&state.pool, user_id).await {
        Ok(Some(subject)) => events::announce_session_change(state, &subject, "membership_changed"),
        Ok(None) => {}
        Err(error) => tracing::error!(error = %error, "membership-change notify lookup failed"),
    }
}
