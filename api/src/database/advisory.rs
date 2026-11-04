// Copyright © 2026 Jalapeno Labs

//! Locks that every process built from this crate respects: Postgres advisory locks.
//!
//! An [`AdvisoryLock`] holds one session-level lock on a connection of its own, taken out of the
//! pool for as long as the lock is held, so the lock can span any number of awaits that are not
//! database work, such as Docker calls. [`AdvisoryLock::release`] unlocks it and returns the
//! connection; a guard dropped without it, by an early return or a panic, closes its connection
//! instead, which releases the lock just the same, since Postgres ends a closed session's locks.

use anyhow::Context as _;
use diesel::sql_types::Text;
use diesel_async::pooled_connection::deadpool::Object;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use tracing::{Level, event};

use super::Pool;

/// One advisory lock, held until released or dropped.
pub struct AdvisoryLock {
    /// `None` only once released.
    connection: Option<Object<AsyncPgConnection>>,
    key: &'static str,
}

impl std::fmt::Debug for AdvisoryLock {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdvisoryLock")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl AdvisoryLock {
    /// Waits for the lock named `key`, in whichever process holds it, then holds it.
    ///
    /// # Errors
    /// Fails when no connection is available or Postgres refuses the lock.
    pub async fn acquire(database: &Pool, key: &'static str) -> anyhow::Result<Self> {
        let mut connection = database
            .get()
            .await
            .context("no database connection to take a lock on")?;
        diesel::sql_query("SELECT pg_advisory_lock(hashtextextended($1, 0))")
            .bind::<Text, _>(key)
            .execute(&mut connection)
            .await
            .with_context(|| format!("cannot take the {key} lock"))?;
        Ok(Self {
            connection: Some(connection),
            key,
        })
    }

    /// Unlocks, and returns the connection to the pool.
    pub async fn release(mut self) {
        let Some(mut connection) = self.connection.take() else {
            return;
        };
        let unlocked = diesel::sql_query("SELECT pg_advisory_unlock(hashtextextended($1, 0))")
            .bind::<Text, _>(self.key)
            .execute(&mut connection)
            .await;
        if let Err(error) = unlocked {
            event!(
                name: "database.lock.release_failure",
                Level::WARN,
                db.lock.key = self.key,
                error.message = %error,
                "could not release a lock; closing its connection releases it",
            );
            drop(Object::take(connection));
        }
    }
}

impl Drop for AdvisoryLock {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            // Out of the pool, so closing it ends the session, and with it the lock.
            drop(Object::take(connection));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_lock_is_held_until_released_or_dropped_whichever_process_holds_it() {
        let (url, _connection) = crate::test_support::migrated_database().await;
        // Two pools stand in for two processes.
        let first = crate::connections::connect_postgres(&url, 2)
            .await
            .expect("Postgres accepts connections");
        let second = crate::connections::connect_postgres(&url, 2)
            .await
            .expect("Postgres accepts connections");

        let held = AdvisoryLock::acquire(&first, "test:lock")
            .await
            .expect("taken");
        let waiting = tokio::time::timeout(
            Duration::from_millis(300),
            AdvisoryLock::acquire(&second, "test:lock"),
        )
        .await;
        assert!(
            waiting.is_err(),
            "the second waits while the first holds it"
        );

        held.release().await;
        let taken = AdvisoryLock::acquire(&second, "test:lock")
            .await
            .expect("taken once released");

        // Dropped without releasing, as a panic would leave it, it is still let go.
        drop(taken);
        tokio::time::timeout(
            Duration::from_secs(5),
            AdvisoryLock::acquire(&first, "test:lock"),
        )
        .await
        .expect("taken once its holder's connection closed")
        .expect("taken")
        .release()
        .await;
    }
}
