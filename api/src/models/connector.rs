// Copyright © 2026 Jalapeno Labs

//! Connectors: a service read as its person, through an account the connector owns. See
//! `crate::connectors` and `docs/connectors.md`.
//!
//! A connector signs in with one account of its own, which its kind decides
//! (`crate::connectors::kinds`): a browser, a token account, an Atlassian account
//! (`jira_credentials`), or a GitHub account (`github_credentials`). The
//! `connectors_one_sign_in` constraint holds that exactly one is set, and a unique index on
//! each that no two connectors share one. The one exception is a mail or chat connector that
//! shared its browser before each connector owned its own: it waits without one until its
//! person starts its browser. Removing the account removes its connector.
//!
//! What a connector reads is its content: Jira's allowlist on its Atlassian account, a
//! meeting source's storage location in `storage_location_id`, and anything else in
//! `content`, in the kind's own terms. Its reading state (sign-in, cursor, last read, last
//! error) is what the hub shows; `is_active` false means paused.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::connectors::ConnectorKind;
use crate::connectors::kinds::{self, Provides};
use crate::database::schema::{browser_sessions, connectors};
use crate::models::action_item_link::LinkCredential;

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = connectors, check_for_backend(diesel::pg::Pg))]
pub struct Connector {
    pub id: Uuid,
    /// Which kind reads it; see `crate::connectors::ConnectorKind`.
    pub kind: String,
    /// The browser it reads through, for a kind that signs in through one.
    pub browser_session_id: Option<Uuid>,
    /// `signed_in`, `signed_out`, or `unknown`, as last read.
    pub sign_in_state: String,
    /// Where the last read left off, in the kind's own terms.
    pub cursor: Value,
    pub last_polled_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    /// False while it is paused.
    pub is_active: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Where a kind that keeps files saves them, such as meeting recordings.
    pub storage_location_id: Option<Uuid>,
    /// The token account it signs in with, for a kind whose sign-in is one API key.
    pub token_account_id: Option<Uuid>,
    /// The Atlassian account it reads through, for Jira.
    pub jira_credential_id: Option<Uuid>,
    /// The GitHub account it reads through.
    pub github_credential_id: Option<Uuid>,
    /// What it reads, in its kind's own terms, for a kind whose selection has no table of its own.
    pub content: Value,
    /// Set while a read someone asked for, or a meeting's processing, waits or runs.
    pub syncing_since: Option<DateTime<Utc>>,
}

/// The account a connector reads through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountRef {
    Browser(Uuid),
    Token(Uuid),
    Atlassian(Uuid),
    Github(Uuid),
}

impl AccountRef {
    pub const fn id(self) -> Uuid {
        match self {
            Self::Browser(id) | Self::Token(id) | Self::Atlassian(id) | Self::Github(id) => id,
        }
    }
}

impl Connector {
    /// The account it signs in with, or `None` for one read through a browser while it waits
    /// for its browser. The `connectors_one_sign_in` constraint holds that at most one is set.
    pub fn account(&self) -> Option<AccountRef> {
        self.browser_session_id
            .map(AccountRef::Browser)
            .or(self.token_account_id.map(AccountRef::Token))
            .or(self.jira_credential_id.map(AccountRef::Atlassian))
            .or(self.github_credential_id.map(AccountRef::Github))
    }
}

#[derive(Debug, Insertable)]
#[diesel(table_name = connectors)]
pub struct NewConnector {
    pub id: Uuid,
    pub kind: String,
    pub browser_session_id: Option<Uuid>,
    pub token_account_id: Option<Uuid>,
    pub jira_credential_id: Option<Uuid>,
    pub github_credential_id: Option<Uuid>,
    pub storage_location_id: Option<Uuid>,
    pub content: Value,
    pub created_by: Uuid,
}

impl NewConnector {
    /// A connector of `kind` reading through `account`, with nothing else chosen yet.
    pub fn new(kind: &str, account: AccountRef, created_by: Uuid) -> Self {
        let mut connector = Self {
            id: Uuid::now_v7(),
            kind: kind.to_owned(),
            browser_session_id: None,
            token_account_id: None,
            jira_credential_id: None,
            github_credential_id: None,
            storage_location_id: None,
            content: Value::Object(serde_json::Map::new()),
            created_by,
        };
        match account {
            AccountRef::Browser(id) => connector.browser_session_id = Some(id),
            AccountRef::Token(id) => connector.token_account_id = Some(id),
            AccountRef::Atlassian(id) => connector.jira_credential_id = Some(id),
            AccountRef::Github(id) => connector.github_credential_id = Some(id),
        }
        connector
    }
}

/// What one read of a connector recorded about it.
#[derive(Debug)]
pub struct PollOutcome {
    pub sign_in_state: String,
    /// The keys of the cursor this read owns, merged over the stored cursor rather than in
    /// place of it ([`record_poll`]).
    pub cursor: Value,
    pub last_polled_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// Every active connector of one of `kinds`, oldest first, for the loops that read them.
///
/// # Errors
/// Propagates any database error.
pub async fn list_active(
    connection: &mut AsyncPgConnection,
    kinds: &[&str],
) -> QueryResult<Vec<Connector>> {
    connectors::table
        .filter(connectors::is_active.eq(true))
        .filter(connectors::kind.eq_any(kinds))
        .select(Connector::as_select())
        .order(connectors::created_at.asc())
        .load(connection)
        .await
}

/// The connectors `person` sees: every connector of the workspace's kinds, and the ones of
/// a person's own kinds that `person` created. Oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_visible(
    connection: &mut AsyncPgConnection,
    person: Uuid,
    workspace_kinds: &[&str],
) -> QueryResult<Vec<Connector>> {
    connectors::table
        .filter(
            connectors::kind
                .eq_any(workspace_kinds)
                .or(connectors::created_by.eq(person)),
        )
        .select(Connector::as_select())
        .order(connectors::created_at.asc())
        .load(connection)
        .await
}

/// Marks connector `id` syncing, unless it already is. Answers whether this marked it.
///
/// # Errors
/// Propagates any database error.
pub async fn mark_syncing(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<bool> {
    let marked = diesel::update(
        connectors::table
            .find(id)
            .filter(connectors::syncing_since.is_null()),
    )
    .set(connectors::syncing_since.eq(diesel::dsl::now))
    .execute(connection)
    .await?;
    Ok(marked > 0)
}

/// Marks connector `id` done syncing.
///
/// # Errors
/// Propagates any database error.
pub async fn clear_syncing(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    diesel::update(connectors::table.find(id))
        .set(connectors::syncing_since.eq(None::<DateTime<Utc>>))
        .execute(connection)
        .await
        .map(drop)
}

/// Every connector marked syncing, oldest mark first: reads someone asked for that the engine
/// has not run yet, or that an engine which stopped left marked.
///
/// # Errors
/// Propagates any database error.
pub async fn list_syncing(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Connector>> {
    connectors::table
        .filter(connectors::syncing_since.is_not_null())
        .select(Connector::as_select())
        .order(connectors::syncing_since.asc())
        .load(connection)
        .await
}

/// The connector `id`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such row.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Connector> {
    connectors::table
        .find(id)
        .select(Connector::as_select())
        .first(connection)
        .await
}

/// The connector that owns `account`, if one does.
///
/// # Errors
/// Propagates any database error.
pub async fn find_by_account(
    connection: &mut AsyncPgConnection,
    account: AccountRef,
) -> QueryResult<Option<Connector>> {
    let query = connectors::table
        .select(Connector::as_select())
        .into_boxed();
    let query = match account {
        AccountRef::Browser(id) => query.filter(connectors::browser_session_id.eq(id)),
        AccountRef::Token(id) => query.filter(connectors::token_account_id.eq(id)),
        AccountRef::Atlassian(id) => query.filter(connectors::jira_credential_id.eq(id)),
        AccountRef::Github(id) => query.filter(connectors::github_credential_id.eq(id)),
    };
    query.first(connection).await.optional()
}

/// The accounts of every paused connector the link watcher reads through, which it skips: those
/// of the kinds that provide issues (Jira, GitHub, YouTrack). Only those kinds count: a
/// Confluence connector's Atlassian account is its own, and the watcher never reads through it.
///
/// # Errors
/// Propagates any database error.
pub async fn paused_link_accounts(
    connection: &mut AsyncPgConnection,
) -> QueryResult<Vec<LinkCredential>> {
    let issue_kinds: Vec<&str> = kinds::providing(Provides::Issues)
        .into_iter()
        .map(ConnectorKind::as_str)
        .collect();
    let paused: Vec<Connector> = connectors::table
        .filter(connectors::is_active.eq(false))
        .filter(connectors::kind.eq_any(issue_kinds))
        .select(Connector::as_select())
        .load(connection)
        .await?;
    Ok(paused.iter().filter_map(kinds::issue_credential).collect())
}

/// Stores a new connector. One that reads through a browser needs its creator to own that
/// browser.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the browser does not exist or belongs to someone
/// else; a unique violation when another connector already owns the account; a foreign key
/// violation for an account or storage location that does not exist.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_connector: NewConnector,
) -> QueryResult<Connector> {
    if let Some(browser_session_id) = new_connector.browser_session_id {
        let owns_browser: bool = diesel::select(diesel::dsl::exists(
            browser_sessions::table
                .filter(browser_sessions::id.eq(browser_session_id))
                .filter(browser_sessions::created_by.eq(new_connector.created_by)),
        ))
        .get_result(connection)
        .await?;
        if !owns_browser {
            return Err(diesel::result::Error::NotFound);
        }
    }

    diesel::insert_into(connectors::table)
        .values(new_connector)
        .returning(Connector::as_returning())
        .get_result(connection)
        .await
}

/// Records what a read found.
///
/// The outcome's cursor is merged over the stored one, key by key, in the same statement
/// (`cursor || $1`), so two loops that read one connector each keep their own keys: a Teams
/// connector's chat reader keeps `baselined` and `reader`, and the meetings loop keeps `since`.
/// A read that wrote the whole cursor it read at its start would put back what the other loop
/// had replaced meanwhile.
///
/// # Errors
/// Propagates any database error.
pub async fn record_poll(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    outcome: PollOutcome,
) -> QueryResult<Connector> {
    diesel::update(connectors::table.find(id))
        .set((
            connectors::sign_in_state.eq(outcome.sign_in_state),
            connectors::cursor.eq(connectors::cursor.concat(outcome.cursor)),
            connectors::last_polled_at.eq(outcome.last_polled_at),
            connectors::last_error.eq(outcome.last_error),
        ))
        .returning(Connector::as_returning())
        .get_result(connection)
        .await
}

/// Gives the connector `id`, one waiting for its browser, the browser `browser_session_id`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such row or it already has an account.
pub async fn attach_browser(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    browser_session_id: Uuid,
) -> QueryResult<Connector> {
    diesel::update(
        connectors::table
            .find(id)
            .filter(connectors::browser_session_id.is_null())
            .filter(connectors::token_account_id.is_null())
            .filter(connectors::jira_credential_id.is_null())
            .filter(connectors::github_credential_id.is_null()),
    )
    .set(connectors::browser_session_id.eq(browser_session_id))
    .returning(Connector::as_returning())
    .get_result(connection)
    .await
}

/// Pauses or resumes the connector `id`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such row.
pub async fn set_active(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    is_active: bool,
) -> QueryResult<Connector> {
    diesel::update(connectors::table.find(id))
        .set(connectors::is_active.eq(is_active))
        .returning(Connector::as_returning())
        .get_result(connection)
        .await
}

/// Replaces what the connector `id` reads: its `content`, and its storage location.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such row; a foreign key violation for
/// a storage location that does not exist.
pub async fn set_content(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    content: Value,
    storage_location_id: Option<Uuid>,
) -> QueryResult<Connector> {
    diesel::update(connectors::table.find(id))
        .set((
            connectors::content.eq(content),
            connectors::storage_location_id.eq(storage_location_id),
        ))
        .returning(Connector::as_returning())
        .get_result(connection)
        .await
}

/// Removes the connector `id`. Its records stay on the timeline. The account it owned is the
/// caller's to remove; removing the account first removes the connector with it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such row.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(connectors::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}
