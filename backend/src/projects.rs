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

use sqlx::PgConnection;
use uuid::Uuid;

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
// `Member` and `Admin` complete the role contract but are not constructed in
// production until project authz (#49) and the management API (#50) resolve and
// assign roles. `allow` (not `expect`) because the test module already exercises
// them, so the lint fires only on the binary target.
#[allow(dead_code)]
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
    ///
    /// The production read path lands with project authz (#49); the test module
    /// exercises it today, so `allow` fires only on the binary target.
    #[allow(dead_code)]
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
}
