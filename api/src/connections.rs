// Copyright © 2026 Jalapeno Labs

//! Startup connections to Postgres and Redis, with bounded retry.
//!
//! Compose only starts the API after both stores report healthy, but a store can
//! still be mid-restart or briefly unreachable, so each connection is retried with
//! exponential backoff before the process gives up.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use deadpool::Runtime;
use diesel_async::RunQueryDsl;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use secrecy::{ExposeSecret, SecretString};
use tracing::{Level, event};

use crate::database::Pool;

/// Attempts before startup is abandoned. With the backoff below this spans roughly
/// a minute, comfortably longer than a Postgres or Redis restart.
const CONNECT_ATTEMPTS: u32 = 8;

/// First retry delay; doubles each attempt up to [`CONNECT_MAX_BACKOFF`].
const CONNECT_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
const CONNECT_MAX_BACKOFF: Duration = Duration::from_secs(10);

/// How long a handler may wait for a pooled Postgres connection before failing the
/// request rather than queueing indefinitely.
const POSTGRES_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Time allowed for Redis to answer a single command.
const REDIS_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Opens the Postgres pool and proves it with a round trip.
///
/// # Errors
/// Returns the last connection error after [`CONNECT_ATTEMPTS`] failures.
pub async fn connect_postgres(database_url: &SecretString, max_connections: u32) -> Result<Pool> {
    let manager = AsyncDieselConnectionManager::new(database_url.expose_secret());
    let pool = Pool::builder(manager)
        .max_size(
            usize::try_from(max_connections)
                .context("DATABASE_MAX_CONNECTIONS does not fit in usize")?,
        )
        .runtime(Runtime::Tokio1)
        .wait_timeout(Some(POSTGRES_ACQUIRE_TIMEOUT))
        .create_timeout(Some(POSTGRES_ACQUIRE_TIMEOUT))
        .build()
        .context("invalid Postgres pool configuration")?;

    // The pool connects lazily, so check a connection out to prove the database is reachable.
    let mut connection = with_retry("postgres", || pool.get()).await?;
    diesel::sql_query("SELECT 1")
        .execute(&mut connection)
        .await
        .context("postgres answered the handshake but not a query")?;
    drop(connection);

    Ok(pool)
}

/// Opens a self-reconnecting Redis connection and proves it with `PING`.
///
/// # Errors
/// Returns an error if the URL is malformed, the server stays unreachable, or
/// `PING` does not answer `PONG`.
pub async fn connect_redis(redis_url: &SecretString) -> Result<ConnectionManager> {
    let client = redis::Client::open(redis_url.expose_secret())
        .context("REDIS_URL is not a valid Redis URL")?;

    // The manager reconnects on its own after startup; these settings only bound
    // how long a single command may stall while it does.
    let manager_config = ConnectionManagerConfig::new()
        .set_response_timeout(Some(REDIS_RESPONSE_TIMEOUT))
        .set_connection_timeout(Some(REDIS_RESPONSE_TIMEOUT));

    let mut connection = with_retry("redis", || {
        ConnectionManager::new_with_config(client.clone(), manager_config.clone())
    })
    .await?;

    let reply: String = redis::cmd("PING")
        .query_async(&mut connection)
        .await
        .context("redis PING failed")?;
    if reply != "PONG" {
        bail!("redis PING answered {reply:?} instead of PONG");
    }

    Ok(connection)
}

/// Runs `connect` until it succeeds or [`CONNECT_ATTEMPTS`] is exhausted.
async fn with_retry<Connection, Error, Attempt, Pending>(
    target: &'static str,
    connect: Attempt,
) -> Result<Connection>
where
    Error: std::error::Error + Send + Sync + 'static,
    Attempt: Fn() -> Pending,
    Pending: Future<Output = Result<Connection, Error>>,
{
    let mut backoff = CONNECT_INITIAL_BACKOFF;

    for attempt in 1..=CONNECT_ATTEMPTS {
        match connect().await {
            Ok(connection) => {
                event!(
                    name: "connection.open.success",
                    Level::INFO,
                    db.system.name = target,
                    attempt,
                    "store connection established",
                );
                return Ok(connection);
            }
            Err(error) if attempt == CONNECT_ATTEMPTS => {
                return Err(error).with_context(|| {
                    format!("{target} unreachable after {CONNECT_ATTEMPTS} attempts")
                });
            }
            Err(error) => {
                event!(
                    name: "connection.open.retry",
                    Level::WARN,
                    db.system.name = target,
                    attempt,
                    error.message = %error,
                    retry.delay_ms = backoff.as_millis(),
                    "store connection failed, retrying",
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(CONNECT_MAX_BACKOFF);
            }
        }
    }

    unreachable!("the loop returns on the final attempt")
}
