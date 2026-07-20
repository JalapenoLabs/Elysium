//! Auto-provisioning the per-project default local-FS Drive.
//!
//! A brand-new project must have a working upload target with zero admin config, so
//! every project gets a default local-FS Drive rooted under the deployment's data
//! directory (see `docs/storage.md`, "Default Drive and system files"). This module
//! is the single source of truth for that Drive's shape:
//!
//! - **Where.** [`default_drive_root`] roots the Drive at
//!   `<data_dir>/<org_id>/<project_id>`, keeping each project's bytes isolated by org
//!   then project. `data_dir` is resolved once at boot (see
//!   [`crate::config::data_dir`]) and the resolved root is persisted in the Drive's
//!   config, never re-derived at read time.
//! - **What.** [`provision_default_drive`] inserts a `kind = FS`,
//!   `visibility = ORG_WIDE`, `is_default = true` Drive with no credential. The FS
//!   provider creates the directory on first write, so nothing is created on disk here.
//! - **When.** Project creation calls it in the same transaction that makes the
//!   project (`crate::projects`, `crate::bootstrap`, `crate::orgs`), and
//!   [`ensure_default_drives`] reconciles the invariant on boot for projects that
//!   predate this wiring (the migration-backfilled ones).
//!
//! The insert is guarded by the same per-project single-default invariant the
//! `drives_one_default_per_project` partial unique index enforces, so provisioning is
//! idempotent: a project that already has a default Drive is left untouched.

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::{DriveConfig, DriveKind, DriveRow, DriveVisibility, FsConfig};

/// The name given to every auto-provisioned default Drive.
const DEFAULT_DRIVE_NAME: &str = "Default";

/// The on-disk root for a project's auto-provisioned default Drive.
///
/// Rooted at `<data_dir>/<org_id>/<project_id>`, so each org's projects are isolated
/// by path even before Drive-level access control applies. A trailing slash on
/// `data_dir` is trimmed so the join never doubles it.
pub fn default_drive_root(data_dir: &str, org_id: Uuid, project_id: Uuid) -> String {
    format!("{}/{org_id}/{project_id}", data_dir.trim_end_matches('/'))
}

/// Provisions the default local-FS Drive for a project, unless it already has one.
///
/// Inserts a `kind = FS`, `visibility = ORG_WIDE`, `is_default = true` Drive rooted at
/// [`default_drive_root`], with no provider credential and `created_by` set to the
/// provisioning user (`None` for a system reconcile). The insert only fires when the
/// project has no default Drive yet, so calling this repeatedly is safe and never
/// races the `drives_one_default_per_project` invariant. Returns the new [`DriveRow`],
/// or `None` when a default already existed.
///
/// Runs on the caller's connection so it composes inside the transaction that creates
/// the project, keeping "a project always has a default Drive" atomic with the project
/// itself.
///
/// # Errors
/// Returns an error if the insert fails.
pub async fn provision_default_drive(
    conn: &mut PgConnection,
    org_id: Uuid,
    project_id: Uuid,
    data_dir: &str,
    created_by: Option<Uuid>,
) -> sqlx::Result<Option<DriveRow>> {
    let root = default_drive_root(data_dir, org_id, project_id);
    let config = DriveConfig::Fs(FsConfig { root }).to_json();

    // The WHERE NOT EXISTS guard makes this idempotent: a project that already has a
    // default Drive inserts nothing, so a re-run (or the boot reconcile) is a no-op.
    sqlx::query_as::<_, DriveRow>(
        "INSERT INTO drives
             (project_id, org_id, name, kind, config, secret, visibility, is_default, created_by)
         SELECT $1, $2, $3, $4, $5, NULL, $6, TRUE, $7
          WHERE NOT EXISTS (SELECT 1 FROM drives WHERE project_id = $1 AND is_default)
         RETURNING *",
    )
    .bind(project_id)
    .bind(org_id)
    .bind(DEFAULT_DRIVE_NAME)
    .bind(DriveKind::Fs.as_db())
    .bind(config)
    .bind(DriveVisibility::OrgWide.as_db())
    .bind(created_by)
    .fetch_optional(conn)
    .await
}

/// Ensures every project has a default Drive, provisioning the missing ones.
///
/// The boot-time reconcile: it finds each project with no default Drive (the projects
/// migration 0022 backfilled, which never ran the provisioning path) and provisions a
/// system-owned default FS Drive for it. Idempotent, so a restart provisions nothing
/// once the invariant holds. Returns the number of Drives created.
///
/// # Errors
/// Returns an error if the scan, connection acquire, or any insert fails.
pub async fn ensure_default_drives(pool: &PgPool, data_dir: &str) -> sqlx::Result<u64> {
    let missing = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT p.id, p.org_id
           FROM projects p
          WHERE NOT EXISTS (SELECT 1 FROM drives d WHERE d.project_id = p.id AND d.is_default)
          ORDER BY p.created_at",
    )
    .fetch_all(pool)
    .await?;

    if missing.is_empty() {
        tracing::info!(
            reconciled = 0,
            "storage default-Drive reconcile: every project already has a default Drive"
        );
        return Ok(0);
    }

    let mut conn = pool.acquire().await?;
    let mut created: u64 = 0;
    for (project_id, org_id) in missing {
        if let Some(drive) =
            provision_default_drive(&mut conn, org_id, project_id, data_dir, None).await?
        {
            created += 1;
            tracing::info!(
                drive_id = %drive.id,
                project_id = %project_id,
                org_id = %org_id,
                "provisioned missing default Drive for project"
            );
        }
    }

    tracing::info!(
        reconciled = created,
        "storage default-Drive reconcile complete"
    );
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_joins_data_dir_org_and_project() {
        let org = Uuid::nil();
        let project = Uuid::from_u128(1);
        assert_eq!(
            default_drive_root("/var/lib/elysium", org, project),
            format!("/var/lib/elysium/{org}/{project}"),
        );
    }

    #[test]
    fn root_trims_a_trailing_slash_so_the_join_never_doubles_it() {
        let org = Uuid::from_u128(2);
        let project = Uuid::from_u128(3);
        assert_eq!(
            default_drive_root("/srv/elysium/", org, project),
            format!("/srv/elysium/{org}/{project}"),
        );
    }

    #[test]
    fn root_is_an_absolute_posix_path_the_fs_config_accepts() {
        let root = default_drive_root("/var/lib/elysium", Uuid::from_u128(4), Uuid::from_u128(5));
        // The provisioned root must pass the same FS validation every write goes
        // through, so a default Drive is never born with an invalid config.
        let config = serde_json::json!({ "root": root });
        DriveConfig::parse(DriveKind::Fs, &config).expect("provisioned FS root is valid");
    }

    // --- DB-backed provisioning + reconcile ---------------------------------
    //
    // Ignored by default (needs a reachable postgres), like the other storage tests
    // (`drives.rs`, `files.rs`). Run with `cargo test -- --ignored` and a DATABASE_URL.
    // Everything happens inside a rolled-back transaction, so the database is left
    // untouched.

    async fn connect_test_pool() -> PgPool {
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

    async fn insert_org(conn: &mut PgConnection, tag: &str) -> Uuid {
        let unique = Uuid::new_v4();
        sqlx::query_scalar::<_, Uuid>("INSERT INTO orgs (name, slug) VALUES ($1, $2) RETURNING id")
            .bind(format!("{tag} Org"))
            .bind(format!("{tag}-{unique}"))
            .fetch_one(conn)
            .await
            .expect("insert org")
    }

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

    async fn default_drive_count(conn: &mut PgConnection, project: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM drives WHERE project_id = $1 AND is_default",
        )
        .bind(project)
        .fetch_one(conn)
        .await
        .expect("count default drives")
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn provisioning_creates_exactly_one_default_fs_drive_and_is_idempotent() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "prov").await;
        let project = insert_project(&mut tx, org, "proj").await;

        // The first call provisions a default FS Drive rooted under the data dir.
        let drive = provision_default_drive(&mut tx, org, project, "/var/lib/elysium", None)
            .await
            .expect("provision default drive")
            .expect("a project without a default gets one");
        assert_eq!(drive.kind, DriveKind::Fs);
        assert_eq!(drive.visibility, DriveVisibility::OrgWide);
        assert!(drive.is_default, "the provisioned Drive is the default");
        assert!(drive.secret.is_none(), "an FS Drive has no credential");
        assert_eq!(drive.created_by, None, "a system reconcile has no creator");
        assert_eq!(
            drive.parse_config().expect("valid stored config"),
            DriveConfig::Fs(FsConfig {
                root: default_drive_root("/var/lib/elysium", org, project),
            }),
        );

        // A second call is a no-op: the project already has its default Drive, so the
        // per-project single-default invariant is respected without a second row.
        let again = provision_default_drive(&mut tx, org, project, "/var/lib/elysium", None)
            .await
            .expect("second provision is a no-op");
        assert!(again.is_none(), "a project keeps its single default Drive");
        assert_eq!(
            default_drive_count(&mut tx, project).await,
            1,
            "exactly one default Drive exists after repeated provisioning",
        );

        tx.rollback().await.expect("rollback transaction");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn reconcile_backfills_projects_without_a_default_and_then_stabilizes() {
        // The reconcile scans through the pool (not a transaction), so this test
        // commits under unique ids and deletes the org (cascading its rows) at the end.
        let pool = connect_test_pool().await;
        let mut conn = pool.acquire().await.expect("acquire");

        let org = insert_org(&mut conn, "recon").await;
        let bare = insert_project(&mut conn, org, &format!("bare-{}", Uuid::new_v4())).await;
        let seeded = insert_project(&mut conn, org, &format!("seeded-{}", Uuid::new_v4())).await;

        // Give one project a default Drive up front; the other has none.
        provision_default_drive(&mut conn, org, seeded, "/var/lib/elysium", None)
            .await
            .expect("seed a default drive")
            .expect("the seeded project gets its default");
        assert_eq!(default_drive_count(&mut conn, bare).await, 0);
        drop(conn);

        // The reconcile provisions only the missing one, leaving the seeded untouched.
        let created = ensure_default_drives(&pool, "/var/lib/elysium")
            .await
            .expect("first reconcile");
        assert!(created >= 1, "the bare project was backfilled");

        let mut conn = pool.acquire().await.expect("re-acquire");
        assert_eq!(
            default_drive_count(&mut conn, bare).await,
            1,
            "the bare project now has exactly one default Drive",
        );
        assert_eq!(
            default_drive_count(&mut conn, seeded).await,
            1,
            "the already-seeded project keeps its single default Drive",
        );
        drop(conn);

        // A second reconcile is a no-op for these projects: the invariant holds.
        let created_again = ensure_default_drives(&pool, "/var/lib/elysium")
            .await
            .expect("second reconcile");
        let mut conn = pool.acquire().await.expect("re-acquire");
        assert_eq!(default_drive_count(&mut conn, bare).await, 1);
        assert_eq!(default_drive_count(&mut conn, seeded).await, 1);
        // The second pass created nothing for our projects (other orgs are irrelevant
        // here because both of ours already satisfy the invariant).
        let _ = created_again;

        sqlx::query("DELETE FROM orgs WHERE id = $1")
            .bind(org)
            .execute(&mut *conn)
            .await
            .expect("cleanup org");
    }
}
