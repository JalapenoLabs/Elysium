//! Org-wide audit log: an append-only trail of admin-consequential actions.
//!
//! Admin actions (policy changes, member role/lifecycle changes, invites,
//! ownership transfer) call [`record`] on success. Recording is fire-and-forget:
//! a failed write is logged but never blocks the action itself, matching the
//! login-event trail (`crate::login_events`). Admins read the trail through
//! [`list`]. See `docs/org-admin.md`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::authz::{self, Capability};
use crate::session::SessionUser;
use crate::state::AppState;

/// How many recent entries the audit view returns.
const AUDIT_LIMIT: i64 = 100;

/// Records one audit entry; logs (never propagates) a write failure.
///
/// `target_user_id` is the member an action was performed on, or `None` for
/// actions with no member subject (e.g. a policy change). `detail` is
/// action-specific JSON context.
pub async fn record(
    pool: &PgPool,
    org_id: Uuid,
    actor_user_id: Uuid,
    action: &str,
    target_user_id: Option<Uuid>,
    detail: Value,
) {
    let result = sqlx::query(
        "INSERT INTO audit_log (org_id, actor_user_id, action, target_user_id, detail)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(org_id)
    .bind(actor_user_id)
    .bind(action)
    .bind(target_user_id)
    .bind(detail)
    .execute(pool)
    .await;
    if let Err(error) = result {
        tracing::error!(error = %error, action, "failed to record audit entry");
    }
}

/// Row shape for the audit read, with actor and target display info joined in.
type AuditRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Value,
    DateTime<Utc>,
);

/// `GET /api/v1/orgs/:id/audit-log`: the org's recent audit trail (admin+).
pub async fn list(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
) -> Response {
    if let Err(response) =
        authz::require_capability(&state, &principal, org_id, Capability::ViewAudit).await
    {
        return response;
    }

    let rows = sqlx::query_as::<_, AuditRow>(
        "SELECT a.action,
                actor.email, actor.display_name,
                target.email, target.display_name,
                a.detail, a.created_at
           FROM audit_log a
           LEFT JOIN users actor ON actor.id = a.actor_user_id
           LEFT JOIN users target ON target.id = a.target_user_id
          WHERE a.org_id = $1
          ORDER BY a.created_at DESC
          LIMIT $2",
    )
    .bind(org_id)
    .bind(AUDIT_LIMIT)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => Json(json!({
            "entries": rows
                .iter()
                .map(|(action, actor_email, actor_name, target_email, target_name, detail, created_at)| {
                    json!({
                        "action": action,
                        "actor": actor_email.as_ref().map(|email| json!({ "email": email, "display_name": actor_name })),
                        "target": target_email.as_ref().map(|email| json!({ "email": email, "display_name": target_name })),
                        "detail": detail,
                        "created_at": created_at.to_rfc3339(),
                    })
                })
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "audit-log read failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}
