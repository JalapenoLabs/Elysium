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

use anyhow::{Context, Result, anyhow};
use diesel::Connection;
use diesel::RunQueryDsl;
use diesel::sql_types::BigInt;
use diesel_async::AsyncPgConnection;
use diesel_async::async_connection_wrapper::AsyncConnectionWrapper;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use secrecy::{ExposeSecret, SecretString};
use tracing::{Level, event};

/// Every migration in `api/migrations`, compiled in.
const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Advisory lock id shared by every Elysium migrator. The value is arbitrary but
/// must stay constant across releases; it spells "Elysium" in ASCII.
const MIGRATION_LOCK_ID: i64 = 0x0045_6c79_7369_756d;

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
    with_locked_connection(database_url, move |connection| match command {
        MigrationCommand::Run => run_pending(connection),
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
    use diesel::sql_types::Text;
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

        execute(url.clone(), MigrationCommand::Revert { count: 1 })
            .await
            .expect("revert one");
        assert_eq!(llm_objects(&url).await, 0);
        let error = ensure_up_to_date(url.clone())
            .await
            .expect_err("a reverted migration is pending");
        assert!(
            error.to_string().contains("create_llms"),
            "names the pending migration: {error}"
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
}
