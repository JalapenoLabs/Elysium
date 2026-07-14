//! Postgres-backed migration integration tests.
//!
//! Ignored by default because they need a reachable postgres. CI runs them with
//! `cargo test -- --ignored` against a throwaway database; locally:
//!
//! ```text
//! DATABASE_URL=postgres://... cargo test -- --ignored
//! ```
//!
//! These complement the psql-based CI migrations job: that job proves the raw
//! SQL applies on a fresh database, while this test proves the *embedded*
//! migrator the running service uses (`sqlx::migrate!`) agrees with the ledger
//! and is idempotent across restarts.

use sqlx::postgres::PgPoolOptions;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[tokio::test]
#[ignore = "requires a postgres reachable via DATABASE_URL"]
async fn every_checked_in_migration_applies_cleanly() {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this test");

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("could not connect to postgres");

    MIGRATOR
        .run(&pool)
        .await
        .expect("all migrations must apply cleanly");

    // A second run must be a clean no-op, exactly like a service restart
    // against an already-migrated database.
    MIGRATOR
        .run(&pool)
        .await
        .expect("re-running migrations must be a no-op");

    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(&pool)
        .await
        .expect("could not count applied migrations");
    let applied = usize::try_from(applied).expect("a non-negative migration count");
    assert_eq!(
        applied,
        MIGRATOR.migrations.len(),
        "every checked-in migration must be recorded as applied",
    );

    let failed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE NOT success")
        .fetch_one(&pool)
        .await
        .expect("could not count failed migrations");
    assert_eq!(failed, 0, "no migration may be recorded as failed");

    // Spot-check the identity schema the first migration establishes.
    let users_table: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name = 'users'",
    )
    .fetch_one(&pool)
    .await
    .expect("could not inspect the schema");
    assert_eq!(users_table, 1, "the identity migration must create users");
}
