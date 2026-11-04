// Copyright © 2026 Jalapeno Labs

//! Schema migrations, embedded into the binary at compile time.
//!
//! Each migration is a directory under `api/migrations/` holding `up.sql` and
//! `down.sql`. Diesel applies each one inside its own transaction and records it in
//! `__diesel_schema_migrations`, so a failed migration leaves no partial schema.
//!
//! Two processes migrating at once would race on that bookkeeping table, so every
//! command here first takes a Postgres advisory lock. A second migrator simply
//! waits, then finds nothing pending.
//!
//! Applying migrations also creates the databases Ory Kratos and Ory Hydra keep their data in,
//! on the same server, when they are missing. Each migrates its own schema, but cannot create
//! its database, and Postgres's init scripts only run for a new data volume. See
//! `docs/auth.md` and `docs/mcp.md`.
//!
//! Before applying anything, it rebuilds the indexes of each of those databases whose text
//! collation changed under it, which a new Postgres image can do to an existing data volume. See
//! [`rebuild_for_changed_collation`].

use anyhow::{Context, Result, anyhow};
use diesel::Connection;
use diesel::sql_types::{BigInt, Bool, Nullable, Text};
use diesel::{QueryableByName, RunQueryDsl};
use diesel_async::AsyncPgConnection;
use diesel_async::async_connection_wrapper::AsyncConnectionWrapper;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use secrecy::{ExposeSecret, SecretString};
use tracing::{Level, event};
use url::Url;

/// Every migration in `api/migrations`, compiled in.
pub(super) const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Advisory lock id shared by every Elysium migrator. The value is arbitrary but
/// must stay constant across releases; it spells "Elysium" in ASCII.
const MIGRATION_LOCK_ID: i64 = 0x0045_6c79_7369_756d;

/// The databases Ory Kratos (identities) and Ory Hydra (OAuth clients and grants) keep their
/// data in. `compose.yml` names each in its service's DSN.
const SERVICE_DATABASES: [&str; 2] = ["kratos", "hydra"];

/// A blocking Diesel connection driven by the async Postgres driver, which is what
/// Diesel's migration harness requires.
type MigrationConnection = AsyncConnectionWrapper<AsyncPgConnection>;

/// What to do with the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationCommand {
    /// Apply every pending migration.
    Run,
    /// Roll back the most recent `count` migrations.
    Revert { count: u32 },
    /// Roll back every migration.
    RevertAll,
    /// Roll back the latest migration and apply it again, proving `down.sql` works.
    Redo,
    /// List applied and pending migrations.
    Status,
}

/// Executes `command` against the database at `database_url`.
///
/// # Errors
/// Fails if the database is unreachable, a migration's SQL fails (that migration's
/// transaction is rolled back), or there is nothing to revert.
pub async fn execute(database_url: SecretString, command: MigrationCommand) -> Result<()> {
    // Rebuilding a database's indexes takes a connection to that database.
    let server_url = database_url.clone();
    with_locked_connection(database_url, move |connection| match command {
        MigrationCommand::Run => {
            for database in SERVICE_DATABASES {
                ensure_database(connection, database)?;
            }
            let own_database = current_database(connection)?;
            for database in std::iter::once(own_database.as_str()).chain(SERVICE_DATABASES) {
                rebuild_for_changed_collation(connection, &server_url, database)?;
            }
            run_pending(connection)
        }
        MigrationCommand::Revert { count } => revert(connection, count),
        MigrationCommand::RevertAll => {
            let reverted = connection
                .revert_all_migrations(MIGRATIONS)
                .map_err(|error| anyhow!(error))?;
            log_versions("reverted", &reverted);
            Ok(())
        }
        MigrationCommand::Redo => {
            revert(connection, 1)?;
            run_pending(connection)
        }
        MigrationCommand::Status => print_status(connection),
    })
    .await
}

/// Fails unless every embedded migration has been applied.
///
/// The server calls this before binding so it never serves requests against a
/// schema older than the queries it was compiled with.
///
/// # Errors
/// Fails if the database is unreachable or any migration is pending.
pub async fn ensure_up_to_date(database_url: SecretString) -> Result<()> {
    with_locked_connection(database_url, |connection| {
        let pending = connection
            .pending_migrations(MIGRATIONS)
            .map_err(|error| anyhow!(error))?;
        if pending.is_empty() {
            return Ok(());
        }

        let versions: Vec<String> = pending
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
        Err(anyhow!(
            "{} migration(s) pending ({}); run `elysium-api migrate run` first",
            versions.len(),
            versions.join(", "),
        ))
    })
    .await
}

/// Opens a dedicated connection, holds the migration lock for the duration of
/// `work`, and runs it on a blocking thread.
async fn with_locked_connection<Work>(database_url: SecretString, work: Work) -> Result<()>
where
    Work: FnOnce(&mut MigrationConnection) -> Result<()> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut connection = MigrationConnection::establish(database_url.expose_secret())
            .context("cannot connect to Postgres to migrate")?;

        diesel::sql_query("SELECT pg_advisory_lock($1)")
            .bind::<BigInt, _>(MIGRATION_LOCK_ID)
            .execute(&mut connection)
            .context("cannot acquire the migration lock")?;

        let outcome = work(&mut connection);

        // Closing the session would release the lock too; unlocking explicitly just
        // frees it sooner. A failure here cannot outweigh the migration's own result.
        if let Err(error) = diesel::sql_query("SELECT pg_advisory_unlock($1)")
            .bind::<BigInt, _>(MIGRATION_LOCK_ID)
            .execute(&mut connection)
        {
            event!(
                name: "migration.unlock.failure",
                Level::WARN,
                error.message = %error,
                "failed to release the migration lock; it frees when the session closes",
            );
        }

        outcome
    })
    .await
    .context("migration task panicked")?
}

/// Whether a database exists, as `SELECT EXISTS` answers it.
#[derive(QueryableByName)]
struct DatabaseExists {
    #[diesel(sql_type = Bool)]
    exists: bool,
}

/// Creates `name` unless it already exists. `CREATE DATABASE` cannot run in a transaction,
/// which is why this runs on the migration connection directly, under its lock, rather than as
/// a migration.
fn ensure_database(connection: &mut MigrationConnection, name: &str) -> Result<()> {
    if database_exists(connection, name)? {
        return Ok(());
    }

    // The migration lock is per database, so a migrator for another database on this server
    // can create it first. Finding it afterwards means that happened.
    if let Err(error) = diesel::sql_query(format!("CREATE DATABASE {name}")).execute(connection) {
        if database_exists(connection, name)? {
            return Ok(());
        }
        return Err(error).with_context(|| format!("cannot create the {name} database"));
    }
    event!(
        name: "migration.service_database.created",
        Level::INFO,
        db.namespace = name,
        "created the {{db.namespace}} database",
    );
    Ok(())
}

fn database_exists(connection: &mut MigrationConnection, name: &str) -> Result<bool> {
    let found: DatabaseExists =
        diesel::sql_query("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1) AS exists")
            .bind::<Text, _>(name)
            .get_result(connection)
            .with_context(|| format!("cannot look up the {name} database"))?;
    Ok(found.exists)
}

#[derive(QueryableByName)]
struct CurrentDatabase {
    #[diesel(sql_type = Text)]
    name: String,
}

fn current_database(connection: &mut MigrationConnection) -> Result<String> {
    let current: CurrentDatabase = diesel::sql_query("SELECT current_database() AS name")
        .get_result(connection)
        .context("cannot name the database being migrated")?;
    Ok(current.name)
}

/// The collation version a database's indexes were built under, and the one the server's C
/// library gives now. Either is `None` where the library reports none, as musl and the `C`
/// locale do.
#[derive(QueryableByName)]
struct CollationVersions {
    #[diesel(sql_type = Nullable<Text>)]
    recorded: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    actual: Option<String>,
}

/// Rebuilds every index in `name` when the server's C library no longer sorts text the way it did
/// when they were built, then records the version they now match.
///
/// A text index is kept in the order the collation gave when each key went in. Under a C library
/// that sorts differently, lookups walk the index in the new order and miss keys that are there,
/// so a unique index stops refusing duplicates. Postgres records the collation's version and only
/// warns when it changes; this rebuilds instead. It happens when the server's image moves to
/// another C library or another release of it: the Alpine image's musl, which reports no version,
/// to pgvector's Debian glibc, or one glibc to the next. See `docs/database.md`, Collations.
///
/// `REINDEX DATABASE` cannot run in a transaction and only reindexes the database it is connected
/// to, so this runs on the migration lock rather than as a migration, connected to `name` itself.
/// A rebuild that meets duplicates written while the old indexes were stale fails here, before
/// anything serves the database.
fn rebuild_for_changed_collation(
    connection: &mut MigrationConnection,
    server_url: &SecretString,
    name: &str,
) -> Result<()> {
    let versions: CollationVersions = diesel::sql_query(
        "SELECT datcollversion AS recorded, pg_database_collation_actual_version(oid) AS actual \
         FROM pg_database WHERE datname = $1",
    )
    .bind::<Text, _>(name)
    .get_result(connection)
    .with_context(|| format!("cannot read the {name} database's collation version"))?;
    if versions.recorded == versions.actual {
        return Ok(());
    }

    let mut database_url = Url::parse(server_url.expose_secret())
        .context("the database URL does not parse as a URL")?;
    database_url.set_path(&format!("/{name}"));
    let mut database = MigrationConnection::establish(database_url.as_str())
        .with_context(|| format!("cannot connect to the {name} database to rebuild its indexes"))?;
    let quoted_name = format!("\"{}\"", name.replace('"', "\"\""));
    diesel::sql_query(format!("REINDEX DATABASE {quoted_name}"))
        .execute(&mut database)
        .with_context(|| format!("cannot rebuild the {name} database's indexes"))?;
    // Written to the catalog as `ALTER DATABASE ... REFRESH COLLATION VERSION` writes it, because
    // that command refuses a change from or to no version, which is what moving between musl and
    // glibc is. Updating the catalog takes a superuser, as the extensions the migrations create do.
    diesel::sql_query(
        "UPDATE pg_database SET datcollversion = pg_database_collation_actual_version(oid) \
         WHERE datname = $1",
    )
    .bind::<Text, _>(name)
    .execute(connection)
    .with_context(|| format!("cannot record the {name} database's collation version"))?;

    event!(
        name: "migration.collation.rebuilt",
        Level::WARN,
        db.namespace = name,
        db.collation.recorded = versions.recorded.as_deref().unwrap_or("none"),
        db.collation.actual = versions.actual.as_deref().unwrap_or("none"),
        "rebuilt every index in {{db.namespace}}: its collation changed from \
         {{db.collation.recorded}} to {{db.collation.actual}}",
    );
    Ok(())
}

fn run_pending(connection: &mut MigrationConnection) -> Result<()> {
    let applied = connection
        .run_pending_migrations(MIGRATIONS)
        .map_err(|error| anyhow!(error))?;
    if applied.is_empty() {
        event!(name: "migration.run.noop", Level::INFO, "schema already up to date");
        return Ok(());
    }

    log_versions("applied", &applied);
    Ok(())
}

fn revert(connection: &mut MigrationConnection, count: u32) -> Result<()> {
    for _ in 0..count {
        let version = connection
            .revert_last_migration(MIGRATIONS)
            .map_err(|error| anyhow!(error))?;
        log_versions("reverted", &[version]);
    }
    Ok(())
}

fn print_status(connection: &mut MigrationConnection) -> Result<()> {
    let applied = connection
        .applied_migrations()
        .map_err(|error| anyhow!(error))?;
    let pending = connection
        .pending_migrations(MIGRATIONS)
        .map_err(|error| anyhow!(error))?;

    for version in &applied {
        println!("applied  {version}");
    }
    for migration in &pending {
        println!("pending  {}", migration.name());
    }
    Ok(())
}

fn log_versions(outcome: &'static str, versions: &[diesel::migration::MigrationVersion<'_>]) {
    for version in versions {
        event!(
            name: "migration.change.success",
            Level::INFO,
            db.migration.outcome = outcome,
            db.migration.version = %version,
            "migration changed",
        );
    }
}

#[cfg(test)]
mod tests {
    use diesel_async::{AsyncConnection, RunQueryDsl};

    use super::*;
    use crate::test_support::empty_database;

    #[derive(diesel::QueryableByName)]
    struct Count {
        #[diesel(sql_type = BigInt)]
        count: i64,
    }

    /// How many relations and types named `llms` / `llm_type` exist.
    async fn llm_objects(url: &SecretString) -> i64 {
        let mut connection = AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("connects");
        let sql = "SELECT (SELECT count(*) FROM pg_class WHERE relname = $1) \
                   + (SELECT count(*) FROM pg_type WHERE typname = $2) AS count";
        diesel::sql_query(sql)
            .bind::<Text, _>("llms")
            .bind::<Text, _>("llm_type")
            .get_result::<Count>(&mut connection)
            .await
            .expect("catalog query runs")
            .count
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn every_migration_applies_reverts_and_reapplies() {
        let url = empty_database().await;

        execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("up");
        assert_eq!(llm_objects(&url).await, 2, "table and enum exist after run");
        ensure_up_to_date(url.clone())
            .await
            .expect("nothing pending after run");

        execute(url.clone(), MigrationCommand::RevertAll)
            .await
            .expect("down");
        assert_eq!(
            llm_objects(&url).await,
            0,
            "down.sql removes everything up.sql created"
        );

        execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("up again");
        assert_eq!(llm_objects(&url).await, 2);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn running_migrations_creates_each_service_database_once() {
        let url = empty_database().await;
        execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("first run creates it");
        execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("a second run finds it and carries on");

        let mut connection = AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("connects");
        for database in SERVICE_DATABASES {
            let found: DatabaseExists = diesel::sql_query(
                "SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1) AS exists",
            )
            .bind::<Text, _>(database)
            .get_result(&mut connection)
            .await
            .expect("catalog query runs");
            assert!(found.exists, "{database} exists after migrating");
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn redo_and_single_revert_leave_a_consistent_schema() {
        let url = empty_database().await;
        execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("up");

        execute(url.clone(), MigrationCommand::Redo)
            .await
            .expect("redo");
        ensure_up_to_date(url.clone())
            .await
            .expect("redo ends fully migrated");

        let migrations =
            diesel::migration::MigrationSource::<diesel::pg::Pg>::migrations(&MIGRATIONS)
                .expect("embedded migrations load");
        let newest = migrations
            .last()
            .expect("at least one migration exists")
            .name()
            .to_string();

        execute(url.clone(), MigrationCommand::Revert { count: 1 })
            .await
            .expect("revert one");
        assert_eq!(
            llm_objects(&url).await,
            2,
            "reverting one migration leaves the earlier ones applied"
        );
        let error = ensure_up_to_date(url.clone())
            .await
            .expect_err("a reverted migration is pending");
        assert!(
            error.to_string().contains(&newest),
            "names the pending migration {newest}: {error}"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn server_refuses_an_unmigrated_database() {
        let url = empty_database().await;
        let error = ensure_up_to_date(url)
            .await
            .expect_err("fresh database has pending migrations");
        assert!(error.to_string().contains("migrate run"));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn concurrent_migrators_serialize_on_the_advisory_lock() {
        let url = empty_database().await;

        let (first, second) = tokio::join!(
            execute(url.clone(), MigrationCommand::Run),
            execute(url.clone(), MigrationCommand::Run),
        );

        first.expect("first migrator succeeds");
        second.expect("second migrator waits, then finds nothing pending");
        ensure_up_to_date(url).await.expect("fully migrated");
    }

    #[derive(diesel::QueryableByName)]
    struct IndexFile {
        #[diesel(sql_type = diesel::sql_types::Oid)]
        relfilenode: u32,
    }

    /// The file `users.email`'s unique index lives in, which `REINDEX` replaces.
    async fn email_index_file(connection: &mut AsyncPgConnection) -> u32 {
        diesel::sql_query("SELECT relfilenode FROM pg_class WHERE relname = 'users_email_key'")
            .get_result::<IndexFile>(connection)
            .await
            .expect("the email index exists")
            .relfilenode
    }

    async fn collation_versions(connection: &mut AsyncPgConnection) -> CollationVersions {
        diesel::sql_query(
            "SELECT datcollversion AS recorded, \
             pg_database_collation_actual_version(oid) AS actual \
             FROM pg_database WHERE datname = current_database()",
        )
        .get_result(connection)
        .await
        .expect("catalog query runs")
    }

    /// A data volume first started under the Alpine image records no collation version, since
    /// musl reports none. Under glibc its text indexes are in the wrong order, so the next run
    /// rebuilds them and records glibc's version.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_database_whose_collation_changed_has_its_indexes_rebuilt() {
        let url = empty_database().await;
        execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("up");
        let mut connection = AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("connects");
        let index_before = email_index_file(&mut connection).await;
        diesel::sql_query(
            "UPDATE pg_database SET datcollversion = NULL WHERE datname = current_database()",
        )
        .execute(&mut connection)
        .await
        .expect("the test role is a superuser, as the stack's is");

        execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("the next run rebuilds the indexes");

        let versions = collation_versions(&mut connection).await;
        assert!(
            versions.actual.is_some(),
            "the test server runs glibc, which reports a version"
        );
        assert_eq!(
            versions.recorded, versions.actual,
            "the version is recorded"
        );
        assert_ne!(
            email_index_file(&mut connection).await,
            index_before,
            "the index was rebuilt"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_database_whose_collation_held_keeps_its_indexes() {
        let url = empty_database().await;
        execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("up");
        let mut connection = AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("connects");
        let index_before = email_index_file(&mut connection).await;

        execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("a second run finds nothing to do");

        assert_eq!(email_index_file(&mut connection).await, index_before);
    }
}
