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

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::authz::{self, OrgRole};
use crate::session::Principal;
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
//
// The resolution layer is the interface storage (#21), integrations (#36/#52), and
// mail resolve resource access through; those consumers land in later issues, so in
// the binary target this whole island is currently only exercised by tests. `allow`
// (not `expect`) because the test build *does* use it, where the lint never fires.
#[allow(dead_code)]
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
#[allow(dead_code)] // See `resolve_project_access`: consumers land in later issues.
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
#[allow(dead_code)] // See `resolve_project_access`: consumers land in later issues.
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
#[allow(dead_code)] // See `resolve_project_access`: consumers land in later issues.
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
#[allow(dead_code)] // See `resolve_project_access`: consumers land in later issues.
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
}
