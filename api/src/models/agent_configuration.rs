// Copyright © 2026 Jalapeno Labs

//! How the last fetch of the configuration repository went: the commit it checked out and what
//! it left out, or why it failed, and the inbox item raised while work waits on it. One row,
//! written as the engine fetches. See `crate::agent_configuration`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::agent_configuration_status;
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

/// Records that `source` was fetched into `checkout`, clearing any failure.
///
/// # Errors
/// Propagates any database error.
pub async fn record_fetched(
    connection: &mut AsyncPgConnection,
    source: &Source,
    checkout: &Checkout,
) -> QueryResult<Status> {
    let skipped = serde_json::to_value(&checkout.skipped).expect("skipped items serialize");
    diesel::update(agent_configuration_status::table)
        .set((
            agent_configuration_status::repository.eq(&source.repository),
            agent_configuration_status::github_credential_id.eq(source.github_credential_id),
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
}

/// Records that fetching `source` failed with `error` at `at`. The last good checkout is kept
/// while it is of the same repository and token, since the workspace still holds it; one of
/// another is forgotten.
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
    let keeps_checkout = find(connection).await?.describes(source);
    if keeps_checkout {
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
            agent_configuration_status::github_credential_id.eq(source.github_credential_id),
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
