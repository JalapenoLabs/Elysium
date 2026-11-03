// Copyright © 2026 Jalapeno Labs

//! Every account Elysium signs in with, as one list. See `docs/connectors.md`.
//!
//! Accounts live in one store per way of signing in, each with the columns and constraints its
//! provider needs: Atlassian accounts in `jira_credentials`, GitHub accounts in
//! `github_credentials`, browsers in `browser_sessions`, and API keys in `token_accounts`. This
//! module reads them together, with the connectors each one feeds, for the hub's Accounts list
//! and for naming the account beside each connector.
//!
//! Atlassian and GitHub accounts are the workspace's, like their connectors; browsers and keys
//! are their creator's.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use uuid::Uuid;

use super::{AccountKind, ConnectorKind};
use crate::database::schema::{action_item_links, link_watch_cursors, timeline_records};
use crate::models::connector::{AccountRef, Connector};
use crate::models::link_watch_cursor::LinkWatchCursor;
use crate::models::{
    browser_session, connector, github_credential, jira_credential, token_account,
};

/// An account as the hub lists it. Never carries a secret.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: Uuid,
    pub kind: AccountKind,
    pub name: String,
    /// Who the provider says it is, such as a login or an address; empty when unknown.
    pub identity: String,
    /// When the provider last confirmed it.
    pub checked_at: Option<DateTime<Utc>>,
    /// The workspace default token coding sessions start with, for a GitHub account.
    pub is_default: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    /// The connectors it feeds.
    pub used_by: Vec<AccountUse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUse {
    pub connector_id: Uuid,
    pub kind: String,
}

/// An account's name and kind, beside the connector it feeds.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: Uuid,
    pub kind: AccountKind,
    pub name: String,
}

/// The kind of account a connector reads through.
pub const fn kind_of(account: AccountRef) -> AccountKind {
    match account {
        AccountRef::Browser(_) => AccountKind::Browser,
        // Every token account today is a Fireflies key; a second provider of keys names its
        // kind from the row, as `list` does.
        AccountRef::Token(_) => AccountKind::Fireflies,
        AccountRef::Atlassian(_) => AccountKind::Atlassian,
        AccountRef::Github(_) => AccountKind::Github,
    }
}

/// Every account `person` sees, with the connectors each feeds: the workspace's Atlassian and
/// GitHub accounts, and `person`'s own browsers and keys.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    person: Uuid,
) -> QueryResult<Vec<AccountView>> {
    let mut accounts: Vec<AccountView> = Vec::new();
    for (credential, _allowed) in jira_credential::list(connection).await? {
        accounts.push(AccountView {
            id: credential.id,
            kind: AccountKind::Atlassian,
            name: credential.name,
            identity: credential.display_name,
            checked_at: Some(credential.checked_at),
            is_default: false,
            created_by: credential.created_by,
            created_at: credential.created_at,
            used_by: Vec::new(),
        });
    }
    for credential in github_credential::list(connection).await? {
        accounts.push(AccountView {
            id: credential.id,
            kind: AccountKind::Github,
            name: credential.name,
            identity: credential.login,
            checked_at: Some(credential.checked_at),
            is_default: credential.is_default,
            created_by: credential.created_by,
            created_at: credential.created_at,
            used_by: Vec::new(),
        });
    }
    for browser in browser_session::list_owned(connection, person).await? {
        accounts.push(AccountView {
            id: browser.id,
            kind: AccountKind::Browser,
            name: browser.name,
            identity: browser.start_url,
            checked_at: None,
            is_default: false,
            created_by: browser.created_by,
            created_at: browser.created_at,
            used_by: Vec::new(),
        });
    }
    for account in token_account::list_owned(connection, person).await? {
        let Some(kind) = AccountKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == account.kind)
        else {
            continue;
        };
        accounts.push(AccountView {
            id: account.id,
            kind,
            name: account.name,
            identity: account.identity,
            checked_at: account.checked_at,
            is_default: false,
            created_by: account.created_by,
            created_at: account.created_at,
            used_by: Vec::new(),
        });
    }

    let connectors =
        connector::list_visible(connection, person, &super::kinds::workspace_kinds()).await?;
    let mut uses: HashMap<Uuid, Vec<AccountUse>> = HashMap::new();
    for connector in connectors {
        uses.entry(connector.account().id())
            .or_default()
            .push(AccountUse {
                connector_id: connector.id,
                kind: connector.kind,
            });
    }
    for account in &mut accounts {
        account.used_by = uses.remove(&account.id).unwrap_or_default();
    }
    Ok(accounts)
}

/// The name of every account `connectors` read through, by account id.
///
/// # Errors
/// Propagates any database error.
pub async fn summaries(
    connection: &mut AsyncPgConnection,
    connectors: &[Connector],
) -> QueryResult<HashMap<Uuid, AccountSummary>> {
    let mut summaries: HashMap<Uuid, AccountSummary> = HashMap::new();
    for connector in connectors {
        let account = connector.account();
        if summaries.contains_key(&account.id()) {
            continue;
        }
        let name = match account {
            AccountRef::Atlassian(id) => jira_credential::find(connection, id).await?.name,
            AccountRef::Github(id) => github_credential::find(connection, id).await?.name,
            AccountRef::Browser(id) => browser_session::find(connection, id)
                .await?
                .map(|browser| browser.name)
                .unwrap_or_default(),
            AccountRef::Token(id) => token_account::find(connection, id).await?.name,
        };
        summaries.insert(
            account.id(),
            AccountSummary {
                id: account.id(),
                kind: kind_of(account),
                name,
            },
        );
    }
    Ok(summaries)
}

/// How many things each connector has recorded: timeline records for those that ingest, linked
/// issues and pull requests for Jira and GitHub. By connector id.
///
/// # Errors
/// Propagates any database error.
pub async fn record_counts(
    connection: &mut AsyncPgConnection,
    connectors: &[Connector],
) -> QueryResult<HashMap<Uuid, i64>> {
    let ids: Vec<Uuid> = connectors.iter().map(|connector| connector.id).collect();
    let ingested: Vec<(Option<Uuid>, i64)> = timeline_records::table
        .filter(timeline_records::connector_id.eq_any(&ids))
        .group_by(timeline_records::connector_id)
        .select((timeline_records::connector_id, count_star()))
        .load(connection)
        .await?;
    let mut counts: HashMap<Uuid, i64> = ingested
        .into_iter()
        .filter_map(|(connector_id, count)| connector_id.map(|id| (id, count)))
        .collect();

    // Linked issues belong to the Jira and GitHub connectors, never to another kind reading
    // through the same account, such as Confluence.
    let link_connectors: Vec<&Connector> = link_connectors(connectors).collect();
    let jira_ids: Vec<Uuid> = link_connectors
        .iter()
        .filter_map(|connector| connector.jira_credential_id)
        .collect();
    let github_ids: Vec<Uuid> = link_connectors
        .iter()
        .filter_map(|connector| connector.github_credential_id)
        .collect();
    let jira_links: Vec<(Option<Uuid>, i64)> = action_item_links::table
        .filter(action_item_links::jira_credential_id.eq_any(&jira_ids))
        .group_by(action_item_links::jira_credential_id)
        .select((action_item_links::jira_credential_id, count_star()))
        .load(connection)
        .await?;
    let github_links: Vec<(Option<Uuid>, i64)> = action_item_links::table
        .filter(action_item_links::github_credential_id.eq_any(&github_ids))
        .group_by(action_item_links::github_credential_id)
        .select((action_item_links::github_credential_id, count_star()))
        .load(connection)
        .await?;
    let links: HashMap<Uuid, i64> = jira_links
        .into_iter()
        .chain(github_links)
        .filter_map(|(credential_id, count)| credential_id.map(|id| (id, count)))
        .collect();
    for connector in link_connectors {
        let credential_id = connector
            .jira_credential_id
            .or(connector.github_credential_id);
        if let Some(count) = credential_id.and_then(|id| links.get(&id)) {
            counts.insert(connector.id, *count);
        }
    }
    Ok(counts)
}

/// The connectors among `connectors` that the link watcher reads: Jira and GitHub. An Atlassian
/// account can feed Confluence too, so the watcher's counts and passes are matched by kind as
/// well as account.
fn link_connectors(connectors: &[Connector]) -> impl Iterator<Item = &Connector> {
    connectors.iter().filter(|connector| {
        ConnectorKind::parse(&connector.kind).is_some_and(ConnectorKind::is_link)
    })
}

/// The link watcher's last pass over each Atlassian and GitHub account `connectors` read
/// through, by connector id. Those connectors are read by the watcher, which records its
/// progress per account rather than on the connector.
///
/// # Errors
/// Propagates any database error.
pub async fn watcher_passes(
    connection: &mut AsyncPgConnection,
    connectors: &[Connector],
) -> QueryResult<HashMap<Uuid, LinkWatchCursor>> {
    let link_connectors: Vec<&Connector> = link_connectors(connectors).collect();
    let jira_ids: Vec<Uuid> = link_connectors
        .iter()
        .filter_map(|connector| connector.jira_credential_id)
        .collect();
    let github_ids: Vec<Uuid> = link_connectors
        .iter()
        .filter_map(|connector| connector.github_credential_id)
        .collect();
    let cursors: Vec<LinkWatchCursor> = link_watch_cursors::table
        .filter(
            link_watch_cursors::jira_credential_id
                .eq_any(&jira_ids)
                .or(link_watch_cursors::github_credential_id.eq_any(&github_ids)),
        )
        .select(LinkWatchCursor::as_select())
        .load(connection)
        .await?;
    let mut passes: HashMap<Uuid, LinkWatchCursor> = HashMap::new();
    for cursor in cursors {
        let found = link_connectors.iter().find(|connector| {
            (cursor.jira_credential_id.is_some()
                && connector.jira_credential_id == cursor.jira_credential_id)
                || (cursor.github_credential_id.is_some()
                    && connector.github_credential_id == cursor.github_credential_id)
        });
        if let Some(connector) = found {
            passes.insert(connector.id, cursor);
        }
    }
    Ok(passes)
}
