// Copyright © 2026 Jalapeno Labs

//! How the last fetch of the configuration repository went: the commit it checked out and what
//! it left out, or why it failed, and the inbox item raised while work waits on it. One row,
//! written as the engine fetches. See `crate::agent_configuration`.
//!
//! Only an attempt at the repository and token the settings name now is recorded: a slow
//! worker's fetch of a repository an admin since replaced records nothing, so it never
//! overwrites the current repository's status.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::agent_configuration_status;
use crate::models::user::{self, WorkspaceSettings};
use crate::runner::configuration::{Checkout, Source};

/// The longest error kept, as the column allows.
pub const ERROR_MAX_CHARACTERS: usize = 4_000;

/// The configuration repository's last fetch.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = agent_configuration_status, check_for_backend(diesel::pg::Pg))]
pub struct Status {
    /// The repository and token the last attempt read; `None` before any.
    pub repository: Option<String>,
    pub github_credential_id: Option<Uuid>,
    /// The commit the last good fetch of that repository checked out, its subject, and when.
    pub commit_sha: Option<String>,
    pub commit_subject: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
    /// What that checkout left out, as `crate::runner::configuration::adapt::Skipped`s.
    pub skipped: Value,
    /// Set while the last attempt failed, with what it said.
    pub error: Option<String>,
    pub failed_at: Option<DateTime<Utc>>,
    /// The inbox item raised while held-back work waits on the repository.
    pub inbox_item_id: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

impl Status {
    /// Whether it is about `source`: the repository and token the settings name now.
    pub fn describes(&self, source: &Source) -> bool {
        self.repository.as_deref() == Some(source.repository.as_str())
            && self.github_credential_id == source.github_credential_id
    }
}

/// The status. The migration writes its row, so there always is one.
///
/// # Errors
/// Propagates any database error.
pub async fn find(connection: &mut AsyncPgConnection) -> QueryResult<Status> {
    agent_configuration_status::table
        .select(Status::as_select())
        .first(connection)
        .await
}

/// The status, its row locked for the rest of the transaction.
///
/// # Errors
/// Propagates any database error.
pub async fn lock(connection: &mut AsyncPgConnection) -> QueryResult<Status> {
    agent_configuration_status::table
        .select(Status::as_select())
        .for_update()
        .first(connection)
        .await
}

/// The repository and token `settings` name, if they name a repository.
pub fn named_source(settings: &WorkspaceSettings) -> Option<Source> {
    Some(Source {
        repository: settings.agent_configuration_repository.clone()?,
        github_credential_id: settings.agent_configuration_github_credential_id,
    })
}

/// The status locked for the rest of the transaction, and whether `source` is what the settings
/// name now, which is the only attempt worth recording. The settings are locked too, so an
/// admin's change cannot commit between this read and the write that follows: it waits, and the
/// next attempt is at what it names. A settings change locks the settings alone, so the two never
/// wait on each other in a circle.
async fn lock_for(
    connection: &mut AsyncPgConnection,
    source: &Source,
) -> QueryResult<(Status, bool)> {
    let status = lock(connection).await?;
    let settings = user::lock_workspace_settings(connection).await?;
    let is_current = named_source(&settings).as_ref() == Some(source);
    if !is_current {
        tracing::event!(
            name: "agent_configuration.attempt.stale",
            tracing::Level::INFO,
            url.full = %source.repository,
            "a fetch of a repository the settings no longer name is not recorded",
        );
    }
    Ok((status, is_current))
}

/// Records that `source` was fetched into `checkout`, clearing any failure. Nothing is recorded
/// when the settings name another repository or token now.
///
/// # Errors
/// Propagates any database error.
pub async fn record_fetched(
    connection: &mut AsyncPgConnection,
    source: &Source,
    checkout: &Checkout,
) -> QueryResult<Status> {
    let skipped = serde_json::to_value(&checkout.skipped).expect("skipped items serialize");
    connection
        .transaction(async |connection| {
            let (status, is_current) = lock_for(connection, source).await?;
            if !is_current {
                return Ok(status);
            }
            diesel::update(agent_configuration_status::table)
                .set((
                    agent_configuration_status::repository.eq(&source.repository),
                    agent_configuration_status::github_credential_id
                        .eq(source.github_credential_id),
                    agent_configuration_status::commit_sha.eq(&checkout.commit),
                    agent_configuration_status::commit_subject.eq(&checkout.subject),
                    agent_configuration_status::fetched_at.eq(checkout.fetched_at),
                    agent_configuration_status::skipped.eq(skipped),
                    agent_configuration_status::error.eq(None::<String>),
                    agent_configuration_status::failed_at.eq(None::<DateTime<Utc>>),
                ))
                .returning(Status::as_returning())
                .get_result(connection)
                .await
        })
        .await
}

/// Records that fetching `source` failed with `error` at `at`. The last good checkout is kept
/// while it is of the same repository and token, since the workspace still holds it; one of
/// another is forgotten. Nothing is recorded when the settings name another repository or token
/// now.
///
/// # Errors
/// Propagates any database error.
pub async fn record_failed(
    connection: &mut AsyncPgConnection,
    source: &Source,
    error: &str,
    at: DateTime<Utc>,
) -> QueryResult<Status> {
    let error: String = error.trim().chars().take(ERROR_MAX_CHARACTERS).collect();
    connection
        .transaction(async |connection| {
            // Locked, so a fetch recorded meanwhile is never mistaken for the one this failed.
            let (status, is_current) = lock_for(connection, source).await?;
            if !is_current {
                return Ok(status);
            }
            if status.describes(source) {
                return diesel::update(agent_configuration_status::table)
                    .set((
                        agent_configuration_status::error.eq(&error),
                        agent_configuration_status::failed_at.eq(at),
                    ))
                    .returning(Status::as_returning())
                    .get_result(connection)
                    .await;
            }
            diesel::update(agent_configuration_status::table)
                .set((
                    agent_configuration_status::repository.eq(&source.repository),
                    agent_configuration_status::github_credential_id
                        .eq(source.github_credential_id),
                    agent_configuration_status::commit_sha.eq(None::<String>),
                    agent_configuration_status::commit_subject.eq(None::<String>),
                    agent_configuration_status::fetched_at.eq(None::<DateTime<Utc>>),
                    agent_configuration_status::skipped.eq(Value::Array(Vec::new())),
                    agent_configuration_status::error.eq(&error),
                    agent_configuration_status::failed_at.eq(at),
                ))
                .returning(Status::as_returning())
                .get_result(connection)
                .await
        })
        .await
}

/// Remembers `inbox_item_id` as the item raised for held-back work, or forgets it.
///
/// # Errors
/// Propagates any database error.
pub async fn set_inbox_item(
    connection: &mut AsyncPgConnection,
    inbox_item_id: Option<Uuid>,
) -> QueryResult<()> {
    diesel::update(agent_configuration_status::table)
        .set(agent_configuration_status::inbox_item_id.eq(inbox_item_id))
        .execute(connection)
        .await
        .map(drop)
}
