// Copyright © 2026 Jalapeno Labs

//! Services Elysium reads through one of a person's browsers. See `crate::connectors`.
//!
//! An account is a connector (`kind`) bound to a browser the person signed in to the service
//! in. It belongs to whoever owns that browser, and removing the browser removes it.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::{browser_sessions, connector_accounts};

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = connector_accounts, check_for_backend(diesel::pg::Pg))]
pub struct ConnectorAccount {
    pub id: Uuid,
    /// Which connector reads it; see `crate::connectors::ConnectorKind`.
    pub kind: String,
    pub browser_session_id: Uuid,
    /// `signed_in`, `signed_out`, or `unknown`, as last read.
    pub sign_in_state: String,
    /// Where the last read left off, in the connector's own terms.
    pub cursor: Value,
    pub last_polled_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub is_active: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = connector_accounts)]
pub struct NewConnectorAccount {
    pub id: Uuid,
    pub kind: String,
    pub browser_session_id: Uuid,
    pub created_by: Uuid,
}

/// What one read of an account recorded about it.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = connector_accounts, treat_none_as_null = true)]
pub struct PollOutcome {
    pub sign_in_state: String,
    pub cursor: Value,
    pub last_polled_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// Every active account, for the ingest loop.
///
/// # Errors
/// Propagates any database error.
pub async fn list_active(connection: &mut AsyncPgConnection) -> QueryResult<Vec<ConnectorAccount>> {
    connector_accounts::table
        .filter(connector_accounts::is_active.eq(true))
        .select(ConnectorAccount::as_select())
        .order(connector_accounts::created_at.asc())
        .load(connection)
        .await
}

/// The accounts `owner` created, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_owned(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
) -> QueryResult<Vec<ConnectorAccount>> {
    connector_accounts::table
        .filter(connector_accounts::created_by.eq(owner))
        .select(ConnectorAccount::as_select())
        .order(connector_accounts::created_at.asc())
        .load(connection)
        .await
}

/// The account `id`, when `owner` created it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist or belongs to someone else.
pub async fn find_owned(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<ConnectorAccount> {
    connector_accounts::table
        .filter(connector_accounts::id.eq(id))
        .filter(connector_accounts::created_by.eq(owner))
        .select(ConnectorAccount::as_select())
        .first(connection)
        .await
}

/// Stores a new account, when `owner` owns the browser it reads through.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the browser does not exist or belongs to someone
/// else; a unique violation when the browser already has a connector.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_account: NewConnectorAccount,
) -> QueryResult<ConnectorAccount> {
    let owns_browser: bool = diesel::select(diesel::dsl::exists(
        browser_sessions::table
            .filter(browser_sessions::id.eq(new_account.browser_session_id))
            .filter(browser_sessions::created_by.eq(new_account.created_by)),
    ))
    .get_result(connection)
    .await?;
    if !owns_browser {
        return Err(diesel::result::Error::NotFound);
    }

    diesel::insert_into(connector_accounts::table)
        .values(new_account)
        .returning(ConnectorAccount::as_returning())
        .get_result(connection)
        .await
}

/// Records what a read found.
///
/// # Errors
/// Propagates any database error.
pub async fn record_poll(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    outcome: PollOutcome,
) -> QueryResult<ConnectorAccount> {
    diesel::update(connector_accounts::table.find(id))
        .set(outcome)
        .returning(ConnectorAccount::as_returning())
        .get_result(connection)
        .await
}

/// Removes the account `id`. Its records stay on the timeline.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such row.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(connector_accounts::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}
