//! Delegated admin grant management.
//!
//! Owners and admins delegate specific capabilities (`org_admin_grants`) to
//! members. Managing the grants themselves requires being a real owner/admin (not
//! a delegated capability), so the power to delegate never spreads on its own. See
//! `docs/org-admin.md` and [`crate::authz`].

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::audit;
use crate::authz::{self, Capability, OrgRole};
use crate::session::SessionUser;
use crate::state::AppState;
use crate::users;

/// `GET /api/v1/orgs/:id/grants`: every delegated grant in the org (admin+).
pub async fn list(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
) -> Response {
    if let Err(response) = authz::require_org_role(&state, &principal, org_id, OrgRole::Admin).await
    {
        return response;
    }

    let rows = sqlx::query_as::<_, (Uuid, String, String, String)>(
        "SELECT g.user_id, u.email, u.display_name, g.capability
           FROM org_admin_grants g
           JOIN users u ON u.id = g.user_id
          WHERE g.org_id = $1
          ORDER BY u.display_name ASC",
    )
    .bind(org_id)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => Json(json!({
            "grants": rows
                .iter()
                .map(|(user_id, email, display_name, capability)| json!({
                    "user_id": user_id,
                    "email": email,
                    "display_name": display_name,
                    "capability": capability,
                }))
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "grant list failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Body of the set-grants request: the member's full capability set (replace).
#[derive(Deserialize)]
pub struct SetGrants {
    capabilities: Vec<String>,
}

/// `PUT /api/v1/orgs/:id/members/:user_id/grants`: set a member's capabilities (admin+).
///
/// Replaces the member's whole grant set. Managing grants requires a real owner or
/// admin, never a delegated capability, so delegation cannot cascade.
pub async fn set(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path((org_id, target_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SetGrants>,
) -> Response {
    if let Err(response) = authz::require_org_role(&state, &principal, org_id, OrgRole::Admin).await
    {
        return response;
    }

    // Parse every requested capability up front; an unknown one is a clean 400.
    let mut capabilities = Vec::with_capacity(body.capabilities.len());
    for value in &body.capabilities {
        match Capability::parse(value) {
            Some(capability) => capabilities.push(capability),
            None => return (StatusCode::BAD_REQUEST, "unknown capability").into_response(),
        }
    }

    // Grants only apply to members of this org.
    match authz::membership(&state.pool, target_id, org_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "target lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let actor_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "actor lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Replace the member's grants atomically.
    let outcome = async {
        let mut tx = state.pool.begin().await?;
        sqlx::query("DELETE FROM org_admin_grants WHERE org_id = $1 AND user_id = $2")
            .bind(org_id)
            .bind(target_id)
            .execute(&mut *tx)
            .await?;
        for capability in &capabilities {
            sqlx::query(
                "INSERT INTO org_admin_grants (org_id, user_id, capability) VALUES ($1, $2, $3)",
            )
            .bind(org_id)
            .bind(target_id)
            .bind(capability.as_str())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }
    .await;

    if let Err(error) = outcome {
        tracing::error!(error = %error, "grant update failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    audit::record(
        &state.pool,
        org_id,
        actor_id,
        "member.grants_set",
        Some(target_id),
        json!({ "capabilities": body.capabilities }),
    )
    .await;
    if let Ok(Some(subject)) = users::subject_for_id(&state.pool, target_id).await {
        crate::events::announce_session_change(&state, &subject, "grants_changed");
    }
    StatusCode::NO_CONTENT.into_response()
}
