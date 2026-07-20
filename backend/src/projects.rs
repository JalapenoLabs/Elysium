//! Projects: the org -> project -> team tenancy tier.
//!
//! A project is a sub-tenant workspace under an org: the real unit of work and
//! isolation. It owns its domains, Drives, service integrations, teams, and
//! membership, while the org above it is a thin umbrella (see `docs/projects.md`).
//!
//! Every org has at least one project. Org creation (first-run bootstrap and
//! `POST /api/v1/orgs`) auto-creates a **default project** and makes the org
//! creator its owner via [`create_default`]. The M1 auto-provisioned default Drive
//! lands in this project, so [`create_default`] returns its id for that wiring.
//!
//! Roles persist as smallints in `project_members.role`, mirroring the
//! `elysium.project.v1.ProjectRole` proto enum (see [`ProjectRole`]).
//!
//! The management API (`/api/v1/projects/*`) is the protobuf CRUD, membership, and
//! domain surface over this model. Creating, deleting, and managing domains are org
//! actions gated on the delegatable [`Capability::ManageProjects`]; a project's own
//! membership is managed by its owners and admins ([`require_project_role`]); a
//! rename accepts either. Every authz decision resolves through
//! [`resolve_project_access`], never by re-deriving access.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use rand::rngs::OsRng;
use rand::RngCore;
use sqlx::{Acquire, PgConnection};
use uuid::Uuid;

use crate::authz::{self, Capability, OrgRole};
use crate::orgs::slug_base;
use crate::proto::common::v1::{Timestamp, Uuid as ProtoUuid};
use crate::proto::project::v1::{
    AddProjectDomainRequest, CreateProjectRequest, ListProjectMembersResponse,
    ListProjectsResponse, Project, ProjectMemberInfo, ProjectRole as ProtoProjectRole,
    SetProjectMemberRequest, UpdateProjectRequest,
};
use crate::protobuf::Protobuf;
use crate::session::{Principal, SessionUser};
use crate::state::AppState;
use crate::users;

/// The name of the default project auto-created for every new org.
const DEFAULT_PROJECT_NAME: &str = "Default";

/// The slug of the default project auto-created for every new org.
///
/// Fixed rather than random: it is the first project in a fresh org and the
/// `UNIQUE (org_id, slug)` constraint scopes it per-org, so "default" never
/// collides across orgs.
const DEFAULT_PROJECT_SLUG: &str = "default";

/// A user's role within a single project, mirroring `elysium.project.v1.ProjectRole`.
///
/// Ordered by privilege (`Owner > Admin > Member`) so a required-role check is a
/// comparison. Persisted as the smallint from [`ProjectRole::as_db`]; the proto's
/// zero `UNSPECIFIED` value is never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectRole {
    Member,
    Admin,
    Owner,
}

impl ProjectRole {
    /// The smallint stored in `project_members.role` for this role.
    pub(crate) fn as_db(self) -> i16 {
        match self {
            Self::Member => 1,
            Self::Admin => 2,
            Self::Owner => 3,
        }
    }

    /// Parses a stored `project_members.role` smallint back into a role.
    ///
    /// Returns `None` for any value outside the known set, including the proto's
    /// zero `UNSPECIFIED` sentinel, which is never persisted.
    pub(crate) fn from_db(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::Member),
            2 => Some(Self::Admin),
            3 => Some(Self::Owner),
            _ => None,
        }
    }

    /// The wire enum for this role.
    fn to_proto(self) -> ProtoProjectRole {
        match self {
            Self::Member => ProtoProjectRole::Member,
            Self::Admin => ProtoProjectRole::Admin,
            Self::Owner => ProtoProjectRole::Owner,
        }
    }

    /// Parses a wire enum into a role; the `UNSPECIFIED` sentinel is `None`.
    fn from_proto(value: ProtoProjectRole) -> Option<Self> {
        match value {
            ProtoProjectRole::Member => Some(Self::Member),
            ProtoProjectRole::Admin => Some(Self::Admin),
            ProtoProjectRole::Owner => Some(Self::Owner),
            ProtoProjectRole::Unspecified => None,
        }
    }
}

/// Creates an org's default project and makes `owner_user_id` its owner.
///
/// Runs on the caller's connection so it is atomic with the org creation that
/// precedes it. Returns the new project's id, the anchor the M1 default Drive
/// attaches to (`drives.project_id`).
///
/// # Errors
/// Returns an error if inserting the project or its owner membership fails.
pub async fn create_default(
    conn: &mut PgConnection,
    org_id: Uuid,
    owner_user_id: Uuid,
) -> sqlx::Result<Uuid> {
    let project_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO projects (org_id, name, slug) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(org_id)
    .bind(DEFAULT_PROJECT_NAME)
    .bind(DEFAULT_PROJECT_SLUG)
    .fetch_one(&mut *conn)
    .await?;

    add_member(conn, project_id, owner_user_id, ProjectRole::Owner).await?;
    Ok(project_id)
}

/// Adds a user to a project with a role, on the caller's connection.
///
/// Idempotent: an existing membership keeps its current role, matching how
/// [`crate::users::add_membership`] treats org membership.
///
/// # Errors
/// Returns an error if the insert fails.
pub async fn add_member(
    conn: &mut PgConnection,
    project_id: Uuid,
    user_id: Uuid,
    role: ProjectRole,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (project_id, user_id) DO NOTHING",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(role.as_db())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The effective role `user_id` holds on `project_id`, or `None` for no access.
///
/// This is the single source of truth for the Org -> Project -> Team -> Individual
/// hierarchy; every resource authz check (storage, integrations, mail) resolves
/// through it rather than re-deriving access. It returns the *highest* role granted
/// by any tier, so a caller reached through several paths gets the strongest one:
///
/// - **Org admin bypass.** An org owner or admin holds [`ProjectRole::Owner`] on
///   every project in the org, mirroring resource-level admin bypass elsewhere.
/// - **Full org member cascade.** A plain org member reaches every project with an
///   implicit [`ProjectRole::Member`]. A **guest does not cascade**: it contributes
///   no implicit access and is scoped to the projects and teams it is explicitly
///   added to.
/// - **Project role.** An explicit `project_members` row (the direct individual
///   grant) contributes its own role.
/// - **Team role.** Membership in an **org-level team** (`teams.project_id IS NULL`,
///   inherited by every project) or in a **team scoped to this project** contributes
///   that team's role. A team scoped to another project never leaks here.
///
/// A **suspended** org member is fully cut: it resolves to `None` regardless of any
/// project or team grant, matching how suspension revokes org access. An unknown
/// project also resolves to `None`.
///
/// Runs on the caller's connection (a `&mut PgConnection`), so it composes inside a
/// transaction; the HTTP guard [`require_project_role`] acquires one from the pool.
///
/// # Errors
/// Returns an error if any of the underlying queries fail.
pub async fn resolve_project_access(
    conn: &mut PgConnection,
    user_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<Option<ProjectRole>> {
    // The project's org anchors the org-tier and org-level-team lookups. An unknown
    // project grants nothing.
    let Some(org_id) = sqlx::query_scalar::<_, Uuid>("SELECT org_id FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&mut *conn)
        .await?
    else {
        return Ok(None);
    };

    // Org tier. Suspension cuts first; owner/admin bypass to the top role and can
    // never be beaten, so short-circuit; a member cascades to a Member floor; a
    // guest cascades to nothing.
    let mut effective = match authz::membership(&mut *conn, user_id, org_id).await? {
        Some(entry) if entry.suspended => return Ok(None),
        Some(entry) => match entry.role {
            OrgRole::Owner | OrgRole::Admin => return Ok(Some(ProjectRole::Owner)),
            OrgRole::Member => Some(ProjectRole::Member),
            OrgRole::Guest => None,
        },
        None => None,
    };

    // Project tier: the explicit per-user grant, if any.
    if let Some(role) = explicit_project_role(&mut *conn, user_id, project_id).await? {
        effective = Some(effective.map_or(role, |current| current.max(role)));
    }

    // Team tier: the strongest role among the teams that apply to this project.
    if let Some(role) = team_project_role(&mut *conn, user_id, org_id, project_id).await? {
        effective = Some(effective.map_or(role, |current| current.max(role)));
    }

    Ok(effective)
}

/// Whether `user_id`'s effective role on `project_id` is at least `minimum`.
///
/// The `can`-style predicate resource authz calls once access has a required role;
/// an "action" is expressed as the [`ProjectRole`] it demands. Builds on
/// [`resolve_project_access`], so it honors the full hierarchy.
///
/// # Errors
/// Returns an error if resolving the access fails.
//
// The threshold predicate for resource authz (storage, integrations, mail); those
// consumers land in later issues, so in the binary target it is currently exercised
// only by tests. `allow` (not `expect`) because the test build does use it.
#[allow(dead_code)]
pub async fn has_project_access(
    conn: &mut PgConnection,
    user_id: Uuid,
    project_id: Uuid,
    minimum: ProjectRole,
) -> sqlx::Result<bool> {
    Ok(resolve_project_access(conn, user_id, project_id)
        .await?
        .is_some_and(|role| role >= minimum))
}

/// The explicit `project_members` role for `user_id` on `project_id`, if any.
async fn explicit_project_role(
    executor: impl sqlx::PgExecutor<'_>,
    user_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<Option<ProjectRole>> {
    let role = sqlx::query_scalar::<_, i16>(
        "SELECT role FROM project_members WHERE project_id = $1 AND user_id = $2",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(executor)
    .await?;
    Ok(role.and_then(ProjectRole::from_db))
}

/// The strongest team role `user_id` holds among the teams applying to a project.
///
/// Considers org-level teams (`project_id IS NULL`, inherited by every project) and
/// teams scoped to this project; teams of another project are excluded. The
/// `member`/`admin`/`owner` text roles map to the same 1/2/3 smallints as
/// [`ProjectRole`], so [`ProjectRole::from_db`] parses the aggregate. `MAX` over no
/// rows is `NULL`, i.e. the user is in no applicable team.
async fn team_project_role(
    executor: impl sqlx::PgExecutor<'_>,
    user_id: Uuid,
    org_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<Option<ProjectRole>> {
    let max = sqlx::query_scalar::<_, Option<i16>>(
        // Cast to smallint so the aggregate matches `project_members.role` and
        // `ProjectRole::from_db`; MAX over integer literals would otherwise be int4.
        "SELECT MAX(CASE tm.role
                        WHEN 'owner'  THEN 3
                        WHEN 'admin'  THEN 2
                        WHEN 'member' THEN 1
                    END)::smallint
           FROM team_members tm
           JOIN teams t ON t.id = tm.team_id
          WHERE tm.user_id = $1
            AND t.org_id = $2
            AND (t.project_id IS NULL OR t.project_id = $3)",
    )
    .bind(user_id)
    .bind(org_id)
    .bind(project_id)
    .fetch_one(executor)
    .await?;
    Ok(max.and_then(ProjectRole::from_db))
}

/// Ensures the caller holds at least `minimum` on `project_id`, returning their role.
///
/// The HTTP guard over [`resolve_project_access`], shaped like
/// [`crate::authz::require_org_role`]: it resolves the session principal to a user,
/// then checks the effective project role.
///
/// # Errors
/// Returns a ready `Response` the handler returns as-is: `401` when the session does
/// not resolve to a user, `403` when the caller's effective role is below `minimum`
/// or absent (indistinguishable, so a non-member cannot probe a project's
/// existence), and `503` on a database error.
pub async fn require_project_role(
    state: &AppState,
    principal: &Principal,
    project_id: Uuid,
    minimum: ProjectRole,
) -> Result<ProjectRole, Response> {
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "project authz user lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };

    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "project authz connection acquire failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };

    match resolve_project_access(&mut conn, user_id, project_id).await {
        Ok(Some(role)) if role >= minimum => Ok(role),
        Ok(_) => Err(StatusCode::FORBIDDEN.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "project access resolution failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

// --- Management API: CRUD, membership, domains ------------------------------
//
// Routes live under `/api/v1/projects` (see `crate::api`). Every handler speaks
// protobuf and enforces the hierarchy through the guards above and the org
// capability system.

/// `GET /api/v1/projects`: the projects the caller can access in their active org.
///
/// A full org member sees every project; a guest sees only the ones they reach
/// through an explicit membership or a team. Access is filtered through
/// [`resolve_project_access`], never re-derived in SQL.
pub async fn list(State(state): State<AppState>, SessionUser(principal): SessionUser) -> Response {
    let Some(org_id) = principal.active_org_id else {
        return (StatusCode::BAD_REQUEST, "no active organization").into_response();
    };
    let user_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "project list user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let candidates = sqlx::query_as::<_, (Uuid, String, String, DateTime<Utc>)>(
        "SELECT id, name, slug, created_at FROM projects WHERE org_id = $1 ORDER BY name ASC",
    )
    .bind(org_id)
    .fetch_all(&state.pool)
    .await;
    let candidates = match candidates {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(error = %error, "project list query failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "project list connection acquire failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Keep only the projects the caller can actually reach. Project counts per org
    // are small, so resolving one at a time is fine and keeps access on the single
    // source of truth rather than duplicating the hierarchy in SQL.
    let mut accessible = Vec::new();
    for (id, name, slug, created_at) in candidates {
        match resolve_project_access(&mut conn, user_id, id).await {
            Ok(Some(_)) => accessible.push((id, name, slug, created_at)),
            Ok(None) => {}
            Err(error) => {
                tracing::error!(error = %error, "project access resolution failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        }
    }

    let ids: Vec<Uuid> = accessible.iter().map(|(id, ..)| *id).collect();
    let domain_rows = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT project_id, domain FROM project_domains
          WHERE project_id = ANY($1) ORDER BY domain ASC",
    )
    .bind(&ids)
    .fetch_all(&state.pool)
    .await;
    let domain_rows = match domain_rows {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(error = %error, "project domain query failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let projects = accessible
        .into_iter()
        .map(|(id, name, slug, created_at)| {
            let domains = domain_rows
                .iter()
                .filter(|(project_id, _)| *project_id == id)
                .map(|(_, domain)| domain.clone())
                .collect();
            Project {
                id: Some(proto_uuid(id)),
                org_id: Some(proto_uuid(org_id)),
                name,
                slug,
                domains,
                created_at: Some(to_timestamp(created_at)),
            }
        })
        .collect();

    Protobuf(ListProjectsResponse { projects }).into_response()
}

/// `POST /api/v1/projects`: create a project in the caller's active org.
///
/// Requires the [`Capability::ManageProjects`] capability. The named owner, or the
/// creator when none is given, becomes the project's OWNER. The slug is derived from
/// the name when the request omits it.
pub async fn create(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Protobuf(request): Protobuf<CreateProjectRequest>,
) -> Response {
    let name = request.name.trim();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }
    let Some(org_id) = principal.active_org_id else {
        return (StatusCode::BAD_REQUEST, "no active organization").into_response();
    };
    if let Err(response) =
        authz::require_capability(&state, &principal, org_id, Capability::ManageProjects).await
    {
        return response;
    }

    let creator_id = match users::id_for_subject(&state.pool, &principal.sub).await {
        Ok(Some(id)) => id,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "project create user lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    // Resolve the owner: a named user must exist and belong to the org, otherwise
    // the creator owns the project they just made.
    let owner_id = match request.owner_user_id.as_ref().map(|id| id.value.trim()) {
        Some(value) if !value.is_empty() => match Uuid::parse_str(value) {
            Ok(id) => id,
            Err(_) => return (StatusCode::BAD_REQUEST, "invalid owner_user_id").into_response(),
        },
        _ => creator_id,
    };
    if owner_id != creator_id {
        match authz::role_in_org(&state.pool, owner_id, org_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return (StatusCode::BAD_REQUEST, "owner is not an org member").into_response()
            }
            Err(error) => {
                tracing::error!(error = %error, "project owner lookup failed");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        }
    }

    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "project create connection acquire failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    let project_id = match create_project(&mut conn, org_id, name, &request.slug, owner_id).await {
        Ok(Some(project_id)) => project_id,
        Ok(None) => return (StatusCode::CONFLICT, "slug already in use").into_response(),
        Err(error) => {
            tracing::error!(error = %error, "project creation failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    drop(conn);

    project_response(&state, project_id).await
}

/// How many times a derived slug is retried against `UNIQUE (org_id, slug)`.
///
/// The first attempt uses the clean base slug; each retry appends a fresh random
/// numeric suffix. A handful of attempts makes a repeated random collision
/// astronomically unlikely while keeping the loop bounded. Only derived slugs
/// retry; an explicit slug conflict is surfaced immediately.
const MAX_SLUG_ATTEMPTS: usize = 5;

/// Inserts a project with a unique slug and makes `owner_id` its OWNER, atomically.
///
/// An empty `requested_slug` derives the slug from `name`: the clean base
/// ([`project_slug_base`]) is used when free within the org, and a numeric suffix is
/// appended only on an actual `UNIQUE (org_id, slug)` collision, which also resolves
/// the race against a concurrent create. A non-empty `requested_slug` is used
/// verbatim; a collision returns `Ok(None)` for the caller to surface as `409`
/// rather than being retried. The new project id is `Ok(Some(id))`.
///
/// Each attempt runs in its own transaction (a nested savepoint when `conn` is
/// already in one), so a rejected insert never poisons the outer state and the
/// project insert plus owner membership always commit together.
///
/// # Errors
/// Returns an error for any database failure other than the slug conflict.
pub(crate) async fn create_project(
    conn: &mut PgConnection,
    org_id: Uuid,
    name: &str,
    requested_slug: &str,
    owner_id: Uuid,
) -> sqlx::Result<Option<Uuid>> {
    let explicit = !requested_slug.trim().is_empty();
    let mut slug = if explicit {
        requested_slug.trim().to_string()
    } else {
        project_slug_base(name)
    };

    for _ in 0..MAX_SLUG_ATTEMPTS {
        let mut tx = conn.begin().await?;
        let attempt = insert_project(&mut tx, org_id, name, &slug, owner_id).await;
        match attempt {
            Ok(project_id) => {
                tx.commit().await?;
                return Ok(Some(project_id));
            }
            Err(error) if is_unique_violation(&error) => {
                // Clear the aborted transaction/savepoint before the next attempt.
                tx.rollback().await?;
                if explicit {
                    return Ok(None);
                }
                slug = format!("{}-{}", project_slug_base(name), slug_suffix());
            }
            Err(error) => {
                // Roll back, ignoring its error so the original cause reaches the caller.
                let _ = tx.rollback().await;
                return Err(error);
            }
        }
    }

    // A derived slug still collided after every attempt: report it as a conflict.
    Ok(None)
}

/// Inserts one project row and its owner membership on the caller's connection.
///
/// # Errors
/// Returns an error if either insert fails, including the `UNIQUE (org_id, slug)`
/// violation that [`create_project`] handles.
async fn insert_project(
    conn: &mut PgConnection,
    org_id: Uuid,
    name: &str,
    slug: &str,
    owner_id: Uuid,
) -> sqlx::Result<Uuid> {
    let project_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO projects (org_id, name, slug) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(org_id)
    .bind(name)
    .bind(slug)
    .fetch_one(&mut *conn)
    .await?;
    add_member(conn, project_id, owner_id, ProjectRole::Owner).await?;
    Ok(project_id)
}

/// The clean base slug for a project name, falling back to `project` when empty.
///
/// Derives from [`slug_base`]; a name with no slug-able characters yields the stable
/// `project` fallback so the slug is always non-empty and URL-safe.
fn project_slug_base(name: &str) -> String {
    let base = slug_base(name);
    if base.is_empty() {
        "project".to_string()
    } else {
        base
    }
}

/// A random six-digit numeric suffix appended to a slug on a collision.
///
/// Zero-padded to six digits, matching the `name-724004` shape shown to users. Six
/// digits give enough spread that a repeated collision across [`MAX_SLUG_ATTEMPTS`]
/// is negligible.
fn slug_suffix() -> String {
    format!("{:06}", OsRng.next_u32() % 1_000_000)
}

/// `PATCH /api/v1/projects/{id}`: rename or re-slug a project.
///
/// A project owner or admin, or an org-level [`Capability::ManageProjects`] holder,
/// may edit it. An omitted (empty) field is left unchanged.
pub async fn update(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(project_id): Path<Uuid>,
    Protobuf(request): Protobuf<UpdateProjectRequest>,
) -> Response {
    if let Err(response) = require_project_admin_or_manage(&state, &principal, project_id).await {
        return response;
    }

    // NULLIF turns an omitted (empty) field into NULL, so COALESCE keeps the current
    // value; a non-empty field replaces it.
    let result = sqlx::query(
        "UPDATE projects
            SET name = COALESCE(NULLIF($1, ''), name),
                slug = COALESCE(NULLIF($2, ''), slug)
          WHERE id = $3",
    )
    .bind(request.name.trim())
    .bind(request.slug.trim())
    .bind(project_id)
    .execute(&state.pool)
    .await;
    match result {
        Ok(_) => project_response(&state, project_id).await,
        Err(error) if is_unique_violation(&error) => {
            (StatusCode::CONFLICT, "slug already in use").into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "project update failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `DELETE /api/v1/projects/{id}`: delete a project, unless it is the org's last.
///
/// Requires [`Capability::ManageProjects`]. An org must always keep at least one
/// project, so deleting the last one is refused with `409`.
pub async fn delete(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(project_id): Path<Uuid>,
) -> Response {
    let org_id = match require_manage_projects(&state, &principal, project_id).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };

    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "project delete connection acquire failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    match delete_unless_last(&mut conn, project_id, org_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::CONFLICT,
            "cannot delete the organization's last project",
        )
            .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "project deletion failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /api/v1/projects/{id}/members`: the project roster.
///
/// Any member of the project (at least [`ProjectRole::Member`]) may view it.
pub async fn members(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(project_id): Path<Uuid>,
) -> Response {
    if let Err(response) =
        require_project_role(&state, &principal, project_id, ProjectRole::Member).await
    {
        return response;
    }

    let rows = sqlx::query_as::<_, (Uuid, String, String, String, i16, DateTime<Utc>)>(
        "SELECT u.id, u.email, u.display_name, u.avatar_url, pm.role, pm.joined_at
           FROM project_members pm
           JOIN users u ON u.id = pm.user_id
          WHERE pm.project_id = $1
          ORDER BY pm.joined_at ASC",
    )
    .bind(project_id)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => {
            let members = rows
                .into_iter()
                .map(
                    |(user_id, email, display_name, avatar_url, role, joined_at)| {
                        ProjectMemberInfo {
                            user_id: Some(proto_uuid(user_id)),
                            email,
                            display_name,
                            avatar_url,
                            role: ProjectRole::from_db(role)
                                .map_or(ProtoProjectRole::Unspecified, ProjectRole::to_proto)
                                as i32,
                            joined_at: Some(to_timestamp(joined_at)),
                        }
                    },
                )
                .collect();
            Protobuf(ListProjectMembersResponse { members }).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "project member list failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `PUT /api/v1/projects/{id}/members/{user_id}`: add a member or set their role.
///
/// A project owner or admin may manage membership. To prevent escalation, the actor
/// may not grant a role above their own, and the target must already belong to the
/// org (a project member is always an org member first).
pub async fn set_member(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path((project_id, target_id)): Path<(Uuid, Uuid)>,
    Protobuf(request): Protobuf<SetProjectMemberRequest>,
) -> Response {
    let actor_role =
        match require_project_role(&state, &principal, project_id, ProjectRole::Admin).await {
            Ok(role) => role,
            Err(response) => return response,
        };

    let Some(new_role) = ProtoProjectRole::try_from(request.role)
        .ok()
        .and_then(ProjectRole::from_proto)
    else {
        return (StatusCode::BAD_REQUEST, "invalid role").into_response();
    };
    if new_role > actor_role {
        return (StatusCode::FORBIDDEN, "cannot grant a role above your own").into_response();
    }

    let org_id = match org_for_project(&state, project_id).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    // A project member must first be a member of the project's org.
    match authz::role_in_org(&state.pool, target_id, org_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return (StatusCode::BAD_REQUEST, "not an org member").into_response(),
        Err(error) => {
            tracing::error!(error = %error, "org membership check failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let result = sqlx::query(
        "INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (project_id, user_id) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(project_id)
    .bind(target_id)
    .bind(new_role.as_db())
    .execute(&state.pool)
    .await;
    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "project member set failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `DELETE /api/v1/projects/{id}/members/{user_id}`: remove a project member.
///
/// A project owner or admin may remove a member, but not one who outranks them.
/// Removing the implicit access of a full org member is not possible here; this only
/// drops the explicit `project_members` edge.
pub async fn remove_member(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path((project_id, target_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let actor_role =
        match require_project_role(&state, &principal, project_id, ProjectRole::Admin).await {
            Ok(role) => role,
            Err(response) => return response,
        };

    // An admin cannot remove someone who explicitly outranks them (e.g. an owner).
    match explicit_project_role(&state.pool, target_id, project_id).await {
        Ok(Some(target_role)) if target_role > actor_role => {
            return StatusCode::FORBIDDEN.into_response();
        }
        Ok(_) => {}
        Err(error) => {
            tracing::error!(error = %error, "project target role lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    let result = sqlx::query("DELETE FROM project_members WHERE project_id = $1 AND user_id = $2")
        .bind(project_id)
        .bind(target_id)
        .execute(&state.pool)
        .await;
    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "project member removal failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /api/v1/projects/{id}/domains`: attach a domain to a project.
///
/// Requires [`Capability::ManageProjects`]. A domain maps to exactly one project
/// across the deployment, so attaching one already owned by another project is
/// refused with `409`.
pub async fn add_domain(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(project_id): Path<Uuid>,
    Protobuf(request): Protobuf<AddProjectDomainRequest>,
) -> Response {
    if let Err(response) = require_manage_projects(&state, &principal, project_id).await {
        return response;
    }

    let domain = normalize_domain(&request.domain);
    if !is_valid_domain(&domain) {
        return (StatusCode::BAD_REQUEST, "invalid domain").into_response();
    }

    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "project domain connection acquire failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    match attach_domain(&mut conn, project_id, &domain).await {
        Ok(true) => {
            drop(conn);
            project_response(&state, project_id).await
        }
        Ok(false) => (StatusCode::CONFLICT, "domain already in use").into_response(),
        Err(error) => {
            tracing::error!(error = %error, "project domain attach failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `DELETE /api/v1/projects/{id}/domains/{domain}`: detach a domain from a project.
///
/// Requires [`Capability::ManageProjects`]. A domain the project does not own is a
/// `404`.
pub async fn remove_domain(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path((project_id, domain)): Path<(Uuid, String)>,
) -> Response {
    if let Err(response) = require_manage_projects(&state, &principal, project_id).await {
        return response;
    }

    let domain = normalize_domain(&domain);
    let result = sqlx::query("DELETE FROM project_domains WHERE project_id = $1 AND domain = $2")
        .bind(project_id)
        .bind(&domain)
        .execute(&state.pool)
        .await;
    match result {
        Ok(outcome) if outcome.rows_affected() == 0 => StatusCode::NOT_FOUND.into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "project domain detach failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Deletes a project unless it is the only one its org has left.
///
/// Returns `true` when the project was deleted, `false` when it was refused as the
/// org's last project. The count and delete are one atomic statement, so two
/// concurrent deletes can never race an org down to zero projects.
///
/// # Errors
/// Returns an error if the delete fails.
pub(crate) async fn delete_unless_last(
    conn: &mut PgConnection,
    project_id: Uuid,
    org_id: Uuid,
) -> sqlx::Result<bool> {
    let affected = sqlx::query(
        "DELETE FROM projects
          WHERE id = $1 AND (SELECT count(*) FROM projects WHERE org_id = $2) > 1",
    )
    .bind(project_id)
    .bind(org_id)
    .execute(conn)
    .await?
    .rows_affected();
    Ok(affected > 0)
}

/// Attaches `domain` to `project_id`, reporting whether the name was free.
///
/// Returns `true` when attached, `false` when the domain is already owned by some
/// project (the deployment-wide `UNIQUE` constraint). Other database errors
/// propagate.
///
/// # Errors
/// Returns an error for any failure other than the unique-violation conflict.
pub(crate) async fn attach_domain(
    conn: &mut PgConnection,
    project_id: Uuid,
    domain: &str,
) -> sqlx::Result<bool> {
    match sqlx::query("INSERT INTO project_domains (project_id, domain) VALUES ($1, $2)")
        .bind(project_id)
        .bind(domain)
        .execute(conn)
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if is_unique_violation(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Resolves a project's org, or a ready `404`/`503` response.
///
/// # Errors
/// Returns a ready response when the project is unknown or the lookup fails.
async fn org_for_project(state: &AppState, project_id: Uuid) -> Result<Uuid, Response> {
    match sqlx::query_scalar::<_, Uuid>("SELECT org_id FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(org_id)) => Ok(org_id),
        Ok(None) => Err(StatusCode::NOT_FOUND.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "project org lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Requires the caller holds [`Capability::ManageProjects`] in the project's org.
///
/// Returns the project's `org_id` on success.
///
/// # Errors
/// Returns a ready response when the project is unknown or the capability is absent.
async fn require_manage_projects(
    state: &AppState,
    principal: &Principal,
    project_id: Uuid,
) -> Result<Uuid, Response> {
    let org_id = org_for_project(state, project_id).await?;
    authz::require_capability(state, principal, org_id, Capability::ManageProjects).await?;
    Ok(org_id)
}

/// Requires the caller may edit a project: a project admin+, or `manage_projects`.
///
/// # Errors
/// Returns a ready response when neither path grants access.
async fn require_project_admin_or_manage(
    state: &AppState,
    principal: &Principal,
    project_id: Uuid,
) -> Result<(), Response> {
    // Project owners/admins (org admins bypass to owner) may edit their project.
    if require_project_role(state, principal, project_id, ProjectRole::Admin)
        .await
        .is_ok()
    {
        return Ok(());
    }
    // Otherwise a delegated org-level manage_projects holder may.
    let org_id = org_for_project(state, project_id).await?;
    authz::require_capability(state, principal, org_id, Capability::ManageProjects).await?;
    Ok(())
}

/// Loads a project and returns it as a protobuf `Project`, or a `404`/`503`.
async fn project_response(state: &AppState, project_id: Uuid) -> Response {
    match load_project(state, project_id).await {
        Ok(Some(project)) => Protobuf(project).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "project load failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Builds the protobuf `Project` for `project_id`, including its domains.
///
/// # Errors
/// Returns an error if either query fails.
async fn load_project(state: &AppState, project_id: Uuid) -> sqlx::Result<Option<Project>> {
    let Some((id, org_id, name, slug, created_at)) =
        sqlx::query_as::<_, (Uuid, Uuid, String, String, DateTime<Utc>)>(
            "SELECT id, org_id, name, slug, created_at FROM projects WHERE id = $1",
        )
        .bind(project_id)
        .fetch_optional(&state.pool)
        .await?
    else {
        return Ok(None);
    };

    let domains = sqlx::query_scalar::<_, String>(
        "SELECT domain FROM project_domains WHERE project_id = $1 ORDER BY domain ASC",
    )
    .bind(project_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Some(Project {
        id: Some(proto_uuid(id)),
        org_id: Some(proto_uuid(org_id)),
        name,
        slug,
        domains,
        created_at: Some(to_timestamp(created_at)),
    }))
}

/// Whether a database error is a unique-constraint violation.
fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
}

/// The canonical stored form of a domain: trimmed and lowercased.
fn normalize_domain(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

/// Whether `domain` is a plausible DNS name: at least two dot-separated labels.
///
/// A light structural check, not a DNS resolution: labels are non-empty ASCII
/// alphanumerics and hyphens, never hyphen-bounded, within the length limits. It
/// keeps obviously malformed input out of the deployment-unique domain table.
fn is_valid_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    labels.iter().all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// Wraps a database `Uuid` in its proto string form.
fn proto_uuid(id: Uuid) -> ProtoUuid {
    ProtoUuid {
        value: id.to_string(),
    }
}

/// Converts a UTC instant into the canonical proto `Timestamp`.
fn to_timestamp(instant: DateTime<Utc>) -> Timestamp {
    Timestamp {
        epoch_seconds: instant.timestamp(),
        nanos: instant.timestamp_subsec_nanos(),
        timezone: "Etc/UTC".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_ordered_by_privilege() {
        assert!(ProjectRole::Owner > ProjectRole::Admin);
        assert!(ProjectRole::Admin > ProjectRole::Member);
        assert!(ProjectRole::Owner >= ProjectRole::Admin);
        assert!(ProjectRole::Member < ProjectRole::Admin);
    }

    #[test]
    fn db_values_match_the_proto_enum() {
        // 1=member, 2=admin, 3=owner, mirroring elysium.project.v1.ProjectRole
        // minus its zero UNSPECIFIED sentinel.
        assert_eq!(ProjectRole::Member.as_db(), 1);
        assert_eq!(ProjectRole::Admin.as_db(), 2);
        assert_eq!(ProjectRole::Owner.as_db(), 3);
    }

    #[test]
    fn parses_known_role_values_only() {
        assert_eq!(ProjectRole::from_db(1), Some(ProjectRole::Member));
        assert_eq!(ProjectRole::from_db(2), Some(ProjectRole::Admin));
        assert_eq!(ProjectRole::from_db(3), Some(ProjectRole::Owner));
        // The proto UNSPECIFIED sentinel is never a stored role.
        assert_eq!(ProjectRole::from_db(0), None);
        assert_eq!(ProjectRole::from_db(4), None);
    }

    #[test]
    fn db_round_trip_is_lossless() {
        for role in [ProjectRole::Member, ProjectRole::Admin, ProjectRole::Owner] {
            assert_eq!(ProjectRole::from_db(role.as_db()), Some(role));
        }
    }

    #[test]
    fn proto_role_round_trips() {
        for role in [ProjectRole::Member, ProjectRole::Admin, ProjectRole::Owner] {
            assert_eq!(ProjectRole::from_proto(role.to_proto()), Some(role));
        }
    }

    #[test]
    fn proto_unspecified_is_not_a_role() {
        // The zero sentinel maps to no role, so an unset wire value is rejected.
        assert_eq!(ProjectRole::from_proto(ProtoProjectRole::Unspecified), None);
    }

    #[test]
    fn proto_and_db_role_values_agree() {
        // The wire enum and the stored smallint share the same 1/2/3 encoding.
        for role in [ProjectRole::Member, ProjectRole::Admin, ProjectRole::Owner] {
            assert_eq!(role.to_proto() as i32, i32::from(role.as_db()));
        }
    }

    #[test]
    fn domain_normalization_trims_and_lowercases() {
        assert_eq!(normalize_domain("  NavarroCity.COM "), "navarrocity.com");
        assert_eq!(normalize_domain("Example.Org"), "example.org");
    }

    #[test]
    fn domain_validation_accepts_real_names_and_rejects_junk() {
        assert!(is_valid_domain("navarrocity.com"));
        assert!(is_valid_domain("mail.jalapenolabs.io"));
        assert!(is_valid_domain("a-b.example.co"));
        // A single label (no TLD), empty input, whitespace, hyphen-bounded labels,
        // and empty labels are all rejected before they reach the unique table.
        assert!(!is_valid_domain("localhost"));
        assert!(!is_valid_domain(""));
        assert!(!is_valid_domain("bad domain.com"));
        assert!(!is_valid_domain("-bad.com"));
        assert!(!is_valid_domain("bad-.com"));
        assert!(!is_valid_domain("double..dot.com"));
    }

    #[test]
    fn project_slug_base_is_clean_and_url_safe() {
        assert_eq!(project_slug_base("Jalapeno Labs"), "jalapeno-labs");
        assert_eq!(project_slug_base("  Weird!!Name  "), "weird-name");
        assert_eq!(project_slug_base("NavarroCity"), "navarrocity");
        // A name with no slug-able characters falls back to a stable, non-empty slug.
        assert_eq!(project_slug_base("!!!"), "project");
        assert_eq!(project_slug_base(""), "project");
    }

    #[test]
    fn slug_suffix_is_six_numeric_digits() {
        // Sampled repeatedly since the suffix is random; the shape must always hold.
        for _ in 0..256 {
            let suffix = slug_suffix();
            assert_eq!(
                suffix.len(),
                6,
                "the suffix is zero-padded to six digits: {suffix}",
            );
            assert!(
                suffix.chars().all(|c| c.is_ascii_digit()),
                "the suffix is purely numeric: {suffix}",
            );
        }
    }

    // Ignored by default because it needs a reachable postgres, exactly like
    // `tests/migrations.rs`. Run with `cargo test -- --ignored` and a DATABASE_URL.
    // Everything happens inside a rolled-back transaction, so the database is left
    // untouched.
    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn default_project_is_created_owned_by_the_creator() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this test");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("could not connect to postgres");

        // Ensure the schema (including this migration) is present.
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");

        let mut tx = pool.begin().await.expect("begin transaction");

        // Seed a throwaway org and a user to own the project.
        let unique = Uuid::new_v4();
        let org_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO orgs (name, slug) VALUES ('Test Org', $1) RETURNING id",
        )
        .bind(format!("test-{unique}"))
        .fetch_one(&mut *tx)
        .await
        .expect("insert org");

        let user_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email) VALUES ($1, $2) RETURNING id",
        )
        .bind(unique.to_string())
        .bind(format!("creator-{unique}@example.test"))
        .fetch_one(&mut *tx)
        .await
        .expect("insert user");

        let project_id = create_default(&mut tx, org_id, user_id)
            .await
            .expect("create default project");

        // Exactly one project exists for the org.
        let project_count =
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM projects WHERE org_id = $1")
                .bind(org_id)
                .fetch_one(&mut *tx)
                .await
                .expect("count projects");
        assert_eq!(project_count, 1, "a fresh org yields exactly one project");

        // The creator is its sole member, with the OWNER role.
        let (member_count, role) = sqlx::query_as::<_, (i64, i16)>(
            "SELECT count(*), min(role) FROM project_members WHERE project_id = $1",
        )
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await
        .expect("read project members");
        assert_eq!(member_count, 1, "the default project has a single member");
        assert_eq!(
            ProjectRole::from_db(role),
            Some(ProjectRole::Owner),
            "the creator owns the default project",
        );

        // Never commit: the seeded rows vanish on drop, leaving the database clean.
        tx.rollback().await.expect("rollback transaction");
    }

    // --- Resolution-matrix helpers (test-only) ------------------------------
    //
    // Each seeds one row and returns the id the matrix test wires together. They
    // take the transaction's connection so everything rolls back at the end.

    /// Inserts a throwaway user and returns its id.
    async fn insert_user(conn: &mut PgConnection, tag: &str) -> Uuid {
        let unique = Uuid::new_v4();
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email) VALUES ($1, $2) RETURNING id",
        )
        .bind(format!("{tag}-{unique}"))
        .bind(format!("{tag}-{unique}@example.test"))
        .fetch_one(conn)
        .await
        .expect("insert user")
    }

    /// Inserts a throwaway org and returns its id.
    async fn insert_org(conn: &mut PgConnection, tag: &str) -> Uuid {
        let unique = Uuid::new_v4();
        sqlx::query_scalar::<_, Uuid>("INSERT INTO orgs (name, slug) VALUES ($1, $2) RETURNING id")
            .bind(format!("{tag} Org"))
            .bind(format!("{tag}-{unique}"))
            .fetch_one(conn)
            .await
            .expect("insert org")
    }

    /// Grants `user` the org `role`, optionally suspended.
    async fn add_org_member(
        conn: &mut PgConnection,
        org: Uuid,
        user: Uuid,
        role: &str,
        suspended: bool,
    ) {
        sqlx::query(
            "INSERT INTO org_members (user_id, org_id, role, suspended_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(user)
        .bind(org)
        .bind(role)
        .bind(suspended.then(chrono::Utc::now))
        .execute(conn)
        .await
        .expect("add org member");
    }

    /// Inserts a project under `org` and returns its id.
    async fn insert_project(conn: &mut PgConnection, org: Uuid, slug: &str) -> Uuid {
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO projects (org_id, name, slug) VALUES ($1, $2, $2) RETURNING id",
        )
        .bind(org)
        .bind(slug)
        .fetch_one(conn)
        .await
        .expect("insert project")
    }

    /// Inserts a team; `project` is `None` for an org-level (inherited) team.
    async fn insert_team(
        conn: &mut PgConnection,
        org: Uuid,
        project: Option<Uuid>,
        slug: &str,
    ) -> Uuid {
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO teams (org_id, name, slug, project_id) VALUES ($1, $2, $2, $3)
             RETURNING id",
        )
        .bind(org)
        .bind(slug)
        .bind(project)
        .fetch_one(conn)
        .await
        .expect("insert team")
    }

    /// Adds `user` to `team` with the given text role.
    async fn add_team_member(conn: &mut PgConnection, team: Uuid, user: Uuid, role: &str) {
        sqlx::query("INSERT INTO team_members (user_id, team_id, role) VALUES ($1, $2, $3)")
            .bind(user)
            .bind(team)
            .bind(role)
            .execute(conn)
            .await
            .expect("add team member");
    }

    /// Resolves `user`'s effective role on `project` and asserts it equals `expected`.
    async fn expect_role(
        conn: &mut PgConnection,
        user: Uuid,
        project: Uuid,
        expected: Option<ProjectRole>,
        who: &str,
    ) {
        let got = resolve_project_access(conn, user, project)
            .await
            .expect("resolve project access");
        assert_eq!(got, expected, "unexpected effective role: {who}");
    }

    // Ignored by default (needs a reachable postgres), like the test above and
    // `tests/migrations.rs`. Run with `cargo test -- --ignored` and a DATABASE_URL.
    // Everything happens inside a rolled-back transaction, so the database is left
    // untouched.
    //
    // Exercises the whole Org -> Project -> Team -> Individual matrix: each caller
    // archetype against a resource in project A and a resource in project B (both in
    // the same org), plus cross-org isolation and suspension.
    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn resolution_matrix_covers_every_tier() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this test");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("could not connect to postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");
        let mut tx = pool.begin().await.expect("begin transaction");

        // One org with two projects, one org-level team (inherited by both) and one
        // team scoped to project A only.
        let org = insert_org(&mut tx, "acme").await;
        let project_a = insert_project(&mut tx, org, "alpha").await;
        let project_b = insert_project(&mut tx, org, "beta").await;
        let org_team = insert_team(&mut tx, org, None, "orgwide").await;
        let project_a_team = insert_team(&mut tx, org, Some(project_a), "alpha-team").await;

        // A full org member cascades to every project as a Member.
        let full_member = insert_user(&mut tx, "full").await;
        add_org_member(&mut tx, org, full_member, "member", false).await;
        expect_role(
            &mut tx,
            full_member,
            project_a,
            Some(ProjectRole::Member),
            "full member / A",
        )
        .await;
        expect_role(
            &mut tx,
            full_member,
            project_b,
            Some(ProjectRole::Member),
            "full member / B",
        )
        .await;

        // A guest does not cascade: no access to any project it was not added to.
        let guest = insert_user(&mut tx, "guest").await;
        add_org_member(&mut tx, org, guest, "guest", false).await;
        expect_role(&mut tx, guest, project_a, None, "unscoped guest / A").await;
        expect_role(&mut tx, guest, project_b, None, "unscoped guest / B").await;

        // Org admin and owner bypass to Owner on every project.
        let org_admin = insert_user(&mut tx, "admin").await;
        add_org_member(&mut tx, org, org_admin, "admin", false).await;
        expect_role(
            &mut tx,
            org_admin,
            project_a,
            Some(ProjectRole::Owner),
            "org admin / A",
        )
        .await;
        expect_role(
            &mut tx,
            org_admin,
            project_b,
            Some(ProjectRole::Owner),
            "org admin / B",
        )
        .await;

        let org_owner = insert_user(&mut tx, "owner").await;
        add_org_member(&mut tx, org, org_owner, "owner", false).await;
        expect_role(
            &mut tx,
            org_owner,
            project_a,
            Some(ProjectRole::Owner),
            "org owner / A",
        )
        .await;
        expect_role(
            &mut tx,
            org_owner,
            project_b,
            Some(ProjectRole::Owner),
            "org owner / B",
        )
        .await;

        // A guest added to project A as its owner is scoped to A: nothing on B.
        let project_owner = insert_user(&mut tx, "proj-owner").await;
        add_org_member(&mut tx, org, project_owner, "guest", false).await;
        add_member(&mut tx, project_a, project_owner, ProjectRole::Owner)
            .await
            .expect("add project owner");
        expect_role(
            &mut tx,
            project_owner,
            project_a,
            Some(ProjectRole::Owner),
            "project owner / A",
        )
        .await;
        expect_role(&mut tx, project_owner, project_b, None, "project owner / B").await;

        // A guest added to project A as a member: Member on A, nothing on B.
        let project_member = insert_user(&mut tx, "proj-member").await;
        add_org_member(&mut tx, org, project_member, "guest", false).await;
        add_member(&mut tx, project_a, project_member, ProjectRole::Member)
            .await
            .expect("add project member");
        expect_role(
            &mut tx,
            project_member,
            project_a,
            Some(ProjectRole::Member),
            "project member / A",
        )
        .await;
        expect_role(
            &mut tx,
            project_member,
            project_b,
            None,
            "project member / B",
        )
        .await;

        // A guest in an org-level team inherits access to every project. An admin
        // team role maps to ProjectRole::Admin.
        let org_team_member = insert_user(&mut tx, "org-team").await;
        add_org_member(&mut tx, org, org_team_member, "guest", false).await;
        add_team_member(&mut tx, org_team, org_team_member, "admin").await;
        expect_role(
            &mut tx,
            org_team_member,
            project_a,
            Some(ProjectRole::Admin),
            "org-team member / A",
        )
        .await;
        expect_role(
            &mut tx,
            org_team_member,
            project_b,
            Some(ProjectRole::Admin),
            "org-team member / B",
        )
        .await;

        // A guest in a project-A-scoped team reaches A only; the team never leaks to B.
        let project_team_member = insert_user(&mut tx, "proj-team").await;
        add_org_member(&mut tx, org, project_team_member, "guest", false).await;
        add_team_member(&mut tx, project_a_team, project_team_member, "member").await;
        expect_role(
            &mut tx,
            project_team_member,
            project_a,
            Some(ProjectRole::Member),
            "project-team member / A",
        )
        .await;
        expect_role(
            &mut tx,
            project_team_member,
            project_b,
            None,
            "project-team member / B",
        )
        .await;

        // An outsider (a member of a different org) reaches neither project.
        let other_org = insert_org(&mut tx, "other").await;
        let outsider = insert_user(&mut tx, "outsider").await;
        add_org_member(&mut tx, other_org, outsider, "owner", false).await;
        expect_role(&mut tx, outsider, project_a, None, "outsider / A").await;
        expect_role(&mut tx, outsider, project_b, None, "outsider / B").await;

        // Tiers combine to the strongest: a full member also made project-A owner is
        // Owner on A, still Member (the cascade floor) on B.
        let combined = insert_user(&mut tx, "combined").await;
        add_org_member(&mut tx, org, combined, "member", false).await;
        add_member(&mut tx, project_a, combined, ProjectRole::Owner)
            .await
            .expect("raise combined to project owner");
        expect_role(
            &mut tx,
            combined,
            project_a,
            Some(ProjectRole::Owner),
            "combined / A",
        )
        .await;
        expect_role(
            &mut tx,
            combined,
            project_b,
            Some(ProjectRole::Member),
            "combined / B",
        )
        .await;

        // Suspension fully cuts access, even for an owner and even where an explicit
        // project grant exists.
        let suspended = insert_user(&mut tx, "suspended").await;
        add_org_member(&mut tx, org, suspended, "owner", true).await;
        add_member(&mut tx, project_a, suspended, ProjectRole::Owner)
            .await
            .expect("grant suspended a project role");
        expect_role(&mut tx, suspended, project_a, None, "suspended owner / A").await;
        expect_role(&mut tx, suspended, project_b, None, "suspended owner / B").await;

        // An unknown project resolves to no access.
        expect_role(
            &mut tx,
            org_owner,
            Uuid::new_v4(),
            None,
            "owner / unknown project",
        )
        .await;

        // The `has_project_access` predicate honors the same resolution and threshold.
        assert!(
            has_project_access(&mut tx, project_owner, project_a, ProjectRole::Owner)
                .await
                .expect("has_project_access owner on A"),
            "project owner meets the Owner threshold on A",
        );
        assert!(
            !has_project_access(&mut tx, project_owner, project_b, ProjectRole::Member)
                .await
                .expect("has_project_access member on B"),
            "project owner does not meet even Member on B",
        );
        assert!(
            has_project_access(&mut tx, full_member, project_b, ProjectRole::Member)
                .await
                .expect("has_project_access member floor"),
            "a full member meets the Member threshold on B",
        );

        tx.rollback().await.expect("rollback transaction");
    }

    /// Connects a pool and applies migrations, for the DB-backed guard tests below.
    async fn connect_test_pool() -> sqlx::PgPool {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this test");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("could not connect to postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");
        pool
    }

    // The delete guard: an org must always keep at least one project, so deleting
    // its last one is refused. Ignored (needs postgres); rolled back at the end.
    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn deleting_the_orgs_last_project_is_refused() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "solo").await;
        let only = insert_project(&mut tx, org, "only").await;

        // The org's only project cannot be deleted.
        let deleted = delete_unless_last(&mut tx, only, org)
            .await
            .expect("guarded delete");
        assert!(!deleted, "the org's last project must not be deletable");
        let remaining =
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM projects WHERE org_id = $1")
                .bind(org)
                .fetch_one(&mut *tx)
                .await
                .expect("count projects");
        assert_eq!(remaining, 1, "the refused project still exists");

        // Once a second project exists, the first is deletable.
        let _second = insert_project(&mut tx, org, "second").await;
        let deleted = delete_unless_last(&mut tx, only, org)
            .await
            .expect("guarded delete");
        assert!(deleted, "a non-last project deletes");

        tx.rollback().await.expect("rollback transaction");
    }

    // Domain uniqueness: a domain maps to exactly one project across the deployment,
    // so attaching one already owned is refused. Ignored (needs postgres).
    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn attaching_a_domain_owned_by_another_project_is_refused() {
        // Brings the nested-transaction (`SAVEPOINT`) `begin` into scope below.
        use sqlx::Acquire;

        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "domains").await;
        let project_a = insert_project(&mut tx, org, "alpha").await;
        let project_b = insert_project(&mut tx, org, "beta").await;

        // The first attach of a free domain succeeds.
        assert!(
            attach_domain(&mut tx, project_a, "navarrocity.com")
                .await
                .expect("attach to A"),
            "a free domain attaches",
        );

        // Each conflicting attempt runs in its own savepoint. In production every
        // request uses a fresh autocommit connection, so a rejected insert is
        // isolated; here a savepoint reproduces that isolation without poisoning the
        // outer transaction the way a bare insert would.
        for (project, who) in [(project_b, "another project"), (project_a, "the owner")] {
            let mut savepoint = tx.begin().await.expect("open savepoint");
            let attached = attach_domain(&mut savepoint, project, "navarrocity.com")
                .await
                .expect("attach attempt");
            assert!(!attached, "an already-owned domain is refused for {who}");
            savepoint.rollback().await.expect("rollback savepoint");
        }

        tx.rollback().await.expect("rollback transaction");
    }

    // Role-gated membership mutation: the membership endpoints gate on
    // ProjectRole::Admin, so a plain project member cannot change roles while an
    // admin can. Ignored (needs postgres).
    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn a_plain_project_member_cannot_meet_the_membership_admin_gate() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "gate").await;
        let project = insert_project(&mut tx, org, "proj").await;

        // A guest scoped to the project as a plain member.
        let member = insert_user(&mut tx, "member").await;
        add_org_member(&mut tx, org, member, "guest", false).await;
        add_member(&mut tx, project, member, ProjectRole::Member)
            .await
            .expect("add project member");

        // A guest scoped to the project as an admin.
        let admin = insert_user(&mut tx, "admin").await;
        add_org_member(&mut tx, org, admin, "guest", false).await;
        add_member(&mut tx, project, admin, ProjectRole::Admin)
            .await
            .expect("add project admin");

        // The set/remove-member endpoints require ProjectRole::Admin. A plain member
        // is below that gate and cannot mutate roles; the admin clears it.
        assert!(
            !has_project_access(&mut tx, member, project, ProjectRole::Admin)
                .await
                .expect("member admin gate"),
            "a project member is below the membership-admin gate",
        );
        assert!(
            has_project_access(&mut tx, admin, project, ProjectRole::Admin)
                .await
                .expect("admin admin gate"),
            "a project admin meets the membership-admin gate",
        );
        // The member still has view access, proving the gate is about mutation, not
        // visibility of the roster.
        assert!(
            has_project_access(&mut tx, member, project, ProjectRole::Member)
                .await
                .expect("member view gate"),
            "a project member can still view the roster",
        );

        tx.rollback().await.expect("rollback transaction");
    }

    /// Reads a project's stored slug.
    async fn slug_of(conn: &mut PgConnection, project: Uuid) -> String {
        sqlx::query_scalar::<_, String>("SELECT slug FROM projects WHERE id = $1")
            .bind(project)
            .fetch_one(conn)
            .await
            .expect("read project slug")
    }

    // Slug generation: a derived slug stays clean when the base is free within the
    // org and only gains a numeric suffix on a real collision, while an explicit
    // slug is honored verbatim and its collision is a conflict, not a retry. Ignored
    // (needs postgres); everything rolls back at the end.
    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn derived_slug_is_clean_when_free_and_suffixed_on_collision() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "slugs").await;
        let owner = insert_user(&mut tx, "owner").await;
        add_org_member(&mut tx, org, owner, "owner", false).await;

        // A first project derived from its name gets the clean base slug, no suffix.
        let first = create_project(&mut tx, org, "Jalapeno Labs", "", owner)
            .await
            .expect("create first project")
            .expect("the clean slug was free");
        assert_eq!(
            slug_of(&mut tx, first).await,
            "jalapeno-labs",
            "a free base slug is used verbatim",
        );
        // The owner became the project OWNER, mirroring create_default.
        assert_eq!(
            explicit_project_role(&mut *tx, owner, first)
                .await
                .expect("owner role"),
            Some(ProjectRole::Owner),
            "the named owner owns the project",
        );

        // A second project with the same name collides on the clean slug, so it keeps
        // the base and gains a numeric suffix.
        let second = create_project(&mut tx, org, "Jalapeno Labs", "", owner)
            .await
            .expect("create second project")
            .expect("the collision resolved with a suffix");
        let second_slug = slug_of(&mut tx, second).await;
        assert_ne!(
            second_slug, "jalapeno-labs",
            "the collision forced a new slug"
        );
        let suffix = second_slug
            .strip_prefix("jalapeno-labs-")
            .expect("the suffixed slug keeps the clean base");
        assert!(
            !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()),
            "the appended suffix is numeric: {second_slug}",
        );

        // An explicit slug is honored verbatim when free.
        let explicit = create_project(&mut tx, org, "Anything At All", "custom-slug", owner)
            .await
            .expect("create explicit-slug project")
            .expect("the explicit slug was free");
        assert_eq!(slug_of(&mut tx, explicit).await, "custom-slug");

        // An explicit slug that collides is a conflict (None), never silently suffixed.
        let conflict = create_project(&mut tx, org, "Anything At All", "custom-slug", owner)
            .await
            .expect("explicit-slug conflict resolves without a db error");
        assert!(
            conflict.is_none(),
            "an explicit slug collision is surfaced as a conflict, not retried",
        );

        tx.rollback().await.expect("rollback transaction");
    }
}
