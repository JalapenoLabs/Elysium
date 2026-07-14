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
    Member,
    Admin,
    Owner,
}

impl OrgRole {
    /// Parses a stored `org_members.role` value; an unknown value is `None`.
    ///
    /// The same three tokens are used for invited roles, so callers comparing a
    /// requested role against the caller's (anti-escalation) parse through here.
    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            _ => None,
        }
    }

    /// The stored `org_members.role` value for this role.
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }
}

/// The caller's role in `org_id`, or `None` when they are not a member.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn role_in_org(
    pool: &PgPool,
    user_id: Uuid,
    org_id: Uuid,
) -> sqlx::Result<Option<OrgRole>> {
    let role = sqlx::query_scalar::<_, String>(
        "SELECT role FROM org_members WHERE user_id = $1 AND org_id = $2",
    )
    .bind(user_id)
    .bind(org_id)
    .fetch_optional(pool)
    .await?;
    Ok(role.and_then(|value| OrgRole::from_db(&value)))
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
