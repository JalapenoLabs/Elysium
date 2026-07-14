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

    let rows = sqlx::query_as::<_, (Uuid, String, String, String, String, DateTime<Utc>)>(
        "SELECT u.id, u.email, u.display_name, u.avatar_url, m.role, m.joined_at
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
                .map(|(id, email, display_name, avatar_url, role, joined_at)| {
                    json!({
                        "user_id": id,
                        "email": email,
                        "display_name": display_name,
                        "avatar_url": avatar_url,
                        "role": role,
                        "joined_at": joined_at.to_rfc3339(),
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

    // The removed member lost access to this org; nudge their clients to re-read.
    notify_membership_changed(&state, target_id).await;
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
    match authz::role_in_org(&state.pool, target_id, org_id).await {
        Ok(Some(role)) if role < actor_role => Ok(role),
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
