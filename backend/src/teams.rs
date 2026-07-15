//! Team management within an org.
//!
//! Teams are sub-groups of an org (`teams`, `team_members`). Creating, renaming,
//! deleting a team, and managing its members are org-admin capabilities, gated on
//! the org RBAC guard (see [`crate::authz`]); viewing teams and rosters is open to
//! any org member. A team's own roles exist for future team-scoped permissions;
//! today they are metadata an org admin sets. See `docs/authz.md`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::authz::{self, Capability, OrgRole};
use crate::orgs;
use crate::session::{Principal, SessionUser};
use crate::state::AppState;

/// Body of the create-team request.
#[derive(Deserialize)]
pub struct CreateTeam {
    name: String,
}

/// `POST /api/v1/orgs/:id/teams`: create a team in the org (admin+).
pub async fn create(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
    Json(body): Json<CreateTeam>,
) -> Response {
    let name = body.name.trim();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }
    if let Err(response) =
        authz::require_capability(&state, &principal, org_id, Capability::ManageTeams).await
    {
        return response;
    }

    let slug = orgs::slugify(name);
    let team_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO teams (org_id, name, slug) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(org_id)
    .bind(name)
    .bind(&slug)
    .fetch_one(&state.pool)
    .await;

    match team_id {
        Ok(team_id) => Json(json!({ "id": team_id, "name": name, "slug": slug })).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "team creation failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /api/v1/orgs/:id/teams`: list the org's teams (any member may view).
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

    let rows = sqlx::query_as::<_, (Uuid, String, String, DateTime<Utc>)>(
        "SELECT id, name, slug, created_at FROM teams WHERE org_id = $1 ORDER BY name ASC",
    )
    .bind(org_id)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => Json(json!({
            "teams": rows
                .iter()
                .map(|(id, name, slug, created_at)| json!({
                    "id": id,
                    "name": name,
                    "slug": slug,
                    "created_at": created_at.to_rfc3339(),
                }))
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "team list failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Body of the rename-team request.
#[derive(Deserialize)]
pub struct RenameTeam {
    name: String,
}

/// `PATCH /api/v1/teams/:team_id`: rename a team (org admin+).
pub async fn rename(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(team_id): Path<Uuid>,
    Json(body): Json<RenameTeam>,
) -> Response {
    let name = body.name.trim();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }
    if let Err(response) =
        require_team_capability(&state, &principal, team_id, Capability::ManageTeams).await
    {
        return response;
    }

    let result = sqlx::query("UPDATE teams SET name = $1 WHERE id = $2")
        .bind(name)
        .bind(team_id)
        .execute(&state.pool)
        .await;
    match result {
        Ok(_) => Json(json!({ "id": team_id, "name": name })).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "team rename failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `DELETE /api/v1/teams/:team_id`: delete a team (org admin+).
pub async fn delete(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(team_id): Path<Uuid>,
) -> Response {
    if let Err(response) =
        require_team_capability(&state, &principal, team_id, Capability::ManageTeams).await
    {
        return response;
    }

    let result = sqlx::query("DELETE FROM teams WHERE id = $1")
        .bind(team_id)
        .execute(&state.pool)
        .await;
    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "team deletion failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /api/v1/teams/:team_id/members`: the team roster (any org member may view).
pub async fn members(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(team_id): Path<Uuid>,
) -> Response {
    if let Err(response) = require_team_org_role(&state, &principal, team_id, OrgRole::Member).await
    {
        return response;
    }

    let rows = sqlx::query_as::<_, (Uuid, String, String, String, String, DateTime<Utc>)>(
        "SELECT u.id, u.email, u.display_name, u.avatar_url, tm.role, tm.joined_at
           FROM team_members tm
           JOIN users u ON u.id = tm.user_id
          WHERE tm.team_id = $1
          ORDER BY tm.joined_at ASC",
    )
    .bind(team_id)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => Json(json!({
            "members": rows
                .iter()
                .map(|(id, email, display_name, avatar_url, role, joined_at)| json!({
                    "user_id": id,
                    "email": email,
                    "display_name": display_name,
                    "avatar_url": avatar_url,
                    "role": role,
                    "joined_at": joined_at.to_rfc3339(),
                }))
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "team member list failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Body of the add-team-member request.
#[derive(Deserialize)]
pub struct AddMember {
    user_id: Uuid,
    #[serde(default)]
    role: Option<String>,
}

/// `POST /api/v1/teams/:team_id/members`: add an org member to the team (admin+).
pub async fn add_member(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(team_id): Path<Uuid>,
    Json(body): Json<AddMember>,
) -> Response {
    let role = match body.role.as_deref().unwrap_or("member") {
        role @ ("member" | "admin" | "owner") => role,
        _ => return (StatusCode::BAD_REQUEST, "invalid role").into_response(),
    };
    let org_id =
        match require_team_capability(&state, &principal, team_id, Capability::ManageTeams).await {
            Ok(org_id) => org_id,
            Err(response) => return response,
        };

    // A team member must first be a member of the team's org.
    match authz::role_in_org(&state.pool, body.user_id, org_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return (StatusCode::BAD_REQUEST, "not an org member").into_response(),
        Err(error) => {
            tracing::error!(error = %error, "org membership check failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let result = sqlx::query(
        "INSERT INTO team_members (user_id, team_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, team_id) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(body.user_id)
    .bind(team_id)
    .bind(role)
    .execute(&state.pool)
    .await;
    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "team member add failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `DELETE /api/v1/teams/:team_id/members/:user_id`: remove a team member (admin+).
pub async fn remove_member(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path((team_id, user_id)): Path<(Uuid, Uuid)>,
) -> Response {
    if let Err(response) =
        require_team_capability(&state, &principal, team_id, Capability::ManageTeams).await
    {
        return response;
    }

    let result = sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
        .bind(team_id)
        .bind(user_id)
        .execute(&state.pool)
        .await;
    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "team member removal failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Resolves a team's org and requires the caller holds at least `minimum` in it.
///
/// Returns the team's `org_id` on success. A missing team is `404`; the RBAC guard
/// supplies the `401`/`403`/`503` responses.
///
/// # Errors
/// Returns a ready HTTP response when the team is unknown or the role check fails.
async fn require_team_org_role(
    state: &AppState,
    principal: &Principal,
    team_id: Uuid,
    minimum: OrgRole,
) -> Result<Uuid, Response> {
    let org_id = sqlx::query_scalar::<_, Uuid>("SELECT org_id FROM teams WHERE id = $1")
        .bind(team_id)
        .fetch_optional(&state.pool)
        .await;
    let org_id = match org_id {
        Ok(Some(org_id)) => org_id,
        Ok(None) => return Err(StatusCode::NOT_FOUND.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "team org lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    authz::require_org_role(state, principal, org_id, minimum).await?;
    Ok(org_id)
}

/// Resolves a team's org and requires the caller holds `capability` in it.
///
/// The `manage_teams`-capable path for team mutations, mirroring
/// [`require_team_org_role`] for the view path. Returns the team's `org_id`.
///
/// # Errors
/// Returns a ready HTTP response when the team is unknown or the capability check
/// fails.
async fn require_team_capability(
    state: &AppState,
    principal: &Principal,
    team_id: Uuid,
    capability: Capability,
) -> Result<Uuid, Response> {
    let org_id = sqlx::query_scalar::<_, Uuid>("SELECT org_id FROM teams WHERE id = $1")
        .bind(team_id)
        .fetch_optional(&state.pool)
        .await;
    let org_id = match org_id {
        Ok(Some(org_id)) => org_id,
        Ok(None) => return Err(StatusCode::NOT_FOUND.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "team org lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    authz::require_capability(state, principal, org_id, capability).await?;
    Ok(org_id)
}
