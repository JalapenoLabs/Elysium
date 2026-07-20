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
/// Includes suspended members; use this to inspect a management *target*. Accepts
/// any executor (`&PgPool`, or a `&mut PgConnection` from a transaction) so callers
/// can resolve org membership on their own connection, which the project-tier
/// resolver in [`crate::projects`] relies on.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn membership(
    executor: impl sqlx::PgExecutor<'_>,
    user_id: Uuid,
    org_id: Uuid,
) -> sqlx::Result<Option<Membership>> {
    let row = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
        "SELECT role, suspended_at FROM org_members WHERE user_id = $1 AND org_id = $2",
    )
    .bind(user_id)
    .bind(org_id)
    .fetch_optional(executor)
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

/// A delegated admin capability an org can grant to a member.
///
/// Owners and admins hold every capability implicitly; a member holds one only if
/// it was delegated (`org_admin_grants`). There is deliberately no billing
/// capability: the project gates nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    ManageMembers,
    ManageSecurity,
    ManageTeams,
    ManageProjects,
    ViewAudit,
}

impl Capability {
    /// Parses a stored capability token; an unknown value is `None`.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "manage_members" => Some(Self::ManageMembers),
            "manage_security" => Some(Self::ManageSecurity),
            "manage_teams" => Some(Self::ManageTeams),
            "manage_projects" => Some(Self::ManageProjects),
            "view_audit" => Some(Self::ViewAudit),
            _ => None,
        }
    }

    /// The stored token for this capability.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ManageMembers => "manage_members",
            Self::ManageSecurity => "manage_security",
            Self::ManageTeams => "manage_teams",
            Self::ManageProjects => "manage_projects",
            Self::ViewAudit => "view_audit",
        }
    }
}

/// Whether a member has been granted `capability` in `org_id`.
async fn has_grant(
    pool: &PgPool,
    user_id: Uuid,
    org_id: Uuid,
    capability: Capability,
) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM org_admin_grants
                         WHERE org_id = $1 AND user_id = $2 AND capability = $3)",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(capability.as_str())
    .fetch_one(pool)
    .await
}

/// Ensures the caller may act in `capability`'s area of `org_id`.
///
/// Owner and admin hold every capability implicitly; a member holds one only if it
/// was delegated to them; guests and suspended members hold none.
///
/// # Errors
/// Returns a ready `401`/`403`/`503` response when the check does not pass.
pub async fn require_capability(
    state: &AppState,
    principal: &Principal,
    org_id: Uuid,
    capability: Capability,
) -> Result<(), Response> {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "authz user lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    match role_in_org(&state.pool, user_id, org_id).await {
        Ok(Some(role)) if role >= OrgRole::Admin => Ok(()),
        Ok(Some(OrgRole::Member)) => {
            match has_grant(&state.pool, user_id, org_id, capability).await {
                Ok(true) => Ok(()),
                Ok(false) => Err(StatusCode::FORBIDDEN.into_response()),
                Err(error) => {
                    tracing::error!(error = %error, "capability grant lookup failed");
                    Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
                }
            }
        }
        Ok(_) => Err(StatusCode::FORBIDDEN.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "authz role lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// The caller's authority to manage members in an org.
///
/// A member may be acted on only if their role is strictly below `manage_below`,
/// and may be assigned a role no higher than `grant_ceiling`. An owner manages
/// everyone below owner and grants up to owner; an admin manages below admin and
/// grants up to admin; a member delegated `manage_members` manages the same set as
/// an admin but grants only up to member, so a delegate can never mint an admin.
pub struct ManageAuthority {
    pub actor_id: Uuid,
    pub manage_below: OrgRole,
    pub grant_ceiling: OrgRole,
}

/// Resolves the caller's member-management authority in `org_id`.
///
/// # Errors
/// Returns a ready `401`/`403`/`503` response when the caller has no authority.
pub async fn member_management_authority(
    state: &AppState,
    principal: &Principal,
    org_id: Uuid,
) -> Result<ManageAuthority, Response> {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "authz user lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    let role = match role_in_org(&state.pool, user_id, org_id).await {
        Ok(role) => role,
        Err(error) => {
            tracing::error!(error = %error, "authz role lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    match role {
        Some(OrgRole::Owner) => Ok(ManageAuthority {
            actor_id: user_id,
            manage_below: OrgRole::Owner,
            grant_ceiling: OrgRole::Owner,
        }),
        Some(OrgRole::Admin) => Ok(ManageAuthority {
            actor_id: user_id,
            manage_below: OrgRole::Admin,
            grant_ceiling: OrgRole::Admin,
        }),
        Some(OrgRole::Member) => {
            match has_grant(&state.pool, user_id, org_id, Capability::ManageMembers).await {
                Ok(true) => Ok(ManageAuthority {
                    actor_id: user_id,
                    manage_below: OrgRole::Admin,
                    grant_ceiling: OrgRole::Member,
                }),
                Ok(false) => Err(StatusCode::FORBIDDEN.into_response()),
                Err(error) => {
                    tracing::error!(error = %error, "capability grant lookup failed");
                    Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
                }
            }
        }
        _ => Err(StatusCode::FORBIDDEN.into_response()),
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
