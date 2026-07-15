//! Organization-level authorization (role-based access control).
//!
//! Every org-scoped action checks the caller's role in the target org against the
//! minimum the action requires. Roles are the values stored in `org_members.role`
//! (`owner`, `admin`, `member`) and are totally ordered (owner > admin > member),
//! so one [`require_org_role`] gate expresses "at least admin", "at least member",
//! and so on. Handlers hand it the session principal, the target org, and the
//! minimum; it resolves the caller, checks membership and rank, and on failure
//! returns a ready HTTP response the handler returns as-is.
//!
//! Insufficient role and non-membership return the same `403`, so a non-member
//! cannot use these endpoints to discover that an org exists.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::session::Principal;
use crate::state::AppState;
use crate::users;

/// A member's role within an organization, ordered by privilege.
///
/// The derived ordering follows declaration order, so `Member < Admin < Owner`;
/// a `role >= minimum` comparison is therefore "at least this privileged".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrgRole {
    Guest,
    Member,
    Admin,
    Owner,
}

impl OrgRole {
    /// Parses a stored `org_members.role` value; an unknown value is `None`.
    ///
    /// The same tokens are used for invited roles, so callers comparing a
    /// requested role against the caller's (anti-escalation) parse through here.
    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            "guest" => Some(Self::Guest),
            _ => None,
        }
    }

    /// The stored `org_members.role` value for this role.
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Guest => "guest",
        }
    }
}

/// A raw membership: the stored role and whether it is currently suspended.
///
/// For inspecting a *target* of management, where a suspended member must still be
/// visible (to reactivate or otherwise manage them). Access decisions use
/// [`role_in_org`], which hides suspended members instead.
pub struct Membership {
    pub role: OrgRole,
    pub suspended: bool,
}

/// The raw membership of `user_id` in `org_id`, or `None` when not a member.
///
/// Includes suspended members; use this to inspect a management *target*.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn membership(
    pool: &PgPool,
    user_id: Uuid,
    org_id: Uuid,
) -> sqlx::Result<Option<Membership>> {
    let row = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
        "SELECT role, suspended_at FROM org_members WHERE user_id = $1 AND org_id = $2",
    )
    .bind(user_id)
    .bind(org_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(role, suspended_at)| {
        OrgRole::from_db(&role).map(|role| Membership {
            role,
            suspended: suspended_at.is_some(),
        })
    }))
}

/// The caller's effective role in `org_id`, or `None` when they have no access.
///
/// A suspended member has no effective role, so access checks treat them as a
/// non-member (indistinguishable, per the non-enumeration rule).
///
/// # Errors
/// Returns an error if the query fails.
pub async fn role_in_org(
    pool: &PgPool,
    user_id: Uuid,
    org_id: Uuid,
) -> sqlx::Result<Option<OrgRole>> {
    Ok(membership(pool, user_id, org_id)
        .await?
        .filter(|entry| !entry.suspended)
        .map(|entry| entry.role))
}

/// Ensures the caller holds at least `minimum` in `org_id`, returning their role.
///
/// Resolves the session principal to a user id, then checks membership and rank.
///
/// # Errors
/// Returns a ready `Response` the handler returns as-is: `401` when the session
/// does not resolve to a user, `403` when the caller lacks the role or is not a
/// member (indistinguishable, so non-members cannot probe org existence), and
/// `503` on a database error.
pub async fn require_org_role(
    state: &AppState,
    principal: &Principal,
    org_id: Uuid,
    minimum: OrgRole,
) -> Result<OrgRole, Response> {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "authz user lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };

    match role_in_org(&state.pool, user_id, org_id).await {
        Ok(Some(role)) if role >= minimum => Ok(role),
        Ok(_) => Err(StatusCode::FORBIDDEN.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "authz role lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_ordered_by_privilege() {
        assert!(OrgRole::Owner > OrgRole::Admin);
        assert!(OrgRole::Admin > OrgRole::Member);
        // "At least admin" admits owner and admin, not member.
        assert!(OrgRole::Owner >= OrgRole::Admin);
        assert!(OrgRole::Member < OrgRole::Admin);
    }

    #[test]
    fn parses_known_roles_only() {
        assert_eq!(OrgRole::from_db("owner"), Some(OrgRole::Owner));
        assert_eq!(OrgRole::from_db("admin"), Some(OrgRole::Admin));
        assert_eq!(OrgRole::from_db("member"), Some(OrgRole::Member));
        assert_eq!(OrgRole::from_db("root"), None);
    }
}
