// Copyright © 2026 Jalapeno Labs

//! The accounts connectors own, read for the hub's view of each connector. See
//! `docs/connectors.md`.
//!
//! Accounts live in one store per way of signing in, each with the columns and constraints its
//! provider needs: Atlassian accounts in `jira_credentials`, GitHub accounts in
//! `github_credentials`, browsers in `browser_sessions`, and API keys in `token_accounts`. Each
//! is owned by exactly one connector. This module names the account beside its connector, and
//! reads what the link watcher records per account for the Jira and GitHub connectors.

use std::collections::HashMap;

use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use uuid::Uuid;

use super::{AccountKind, ConnectorKind};
use crate::database::schema::{action_item_links, link_watch_cursors, timeline_records};
use crate::models::connector::{AccountRef, Connector};
use crate::models::link_watch_cursor::LinkWatchCursor;
use crate::models::{browser_session, github_credential, jira_credential, token_account};

/// An account's name and kind, beside the connector that owns it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: Uuid,
    pub kind: AccountKind,
    pub name: String,
}

/// The name of every account `connectors` own, by account id.
///
/// # Errors
/// Propagates any database error.
pub async fn summaries(
    connection: &mut AsyncPgConnection,
    connectors: &[Connector],
) -> QueryResult<HashMap<Uuid, AccountSummary>> {
    let mut summaries: HashMap<Uuid, AccountSummary> = HashMap::new();
    for connector in connectors {
        // A connector waiting for its browser has no account to name yet.
        let Some(account) = connector.account() else {
            continue;
        };
        if summaries.contains_key(&account.id()) {
            continue;
        }
        let (kind, name) = match account {
            AccountRef::Atlassian(id) => (
                AccountKind::Atlassian,
                jira_credential::find(connection, id).await?.name,
            ),
            AccountRef::Github(id) => (
                AccountKind::Github,
                github_credential::find(connection, id).await?.name,
            ),
            AccountRef::Browser(id) => (
                AccountKind::Browser,
                browser_session::find(connection, id)
                    .await?
                    .map(|browser| browser.name)
                    .unwrap_or_default(),
            ),
            // A token account names its provider itself; one this build does not know is
            // left out, as `list` leaves it out.
            AccountRef::Token(id) => {
                let token = token_account::find(connection, id).await?;
                let Some(kind) = AccountKind::parse(&token.kind) else {
                    continue;
                };
                (kind, token.name)
            }
        };
        summaries.insert(
            account.id(),
            AccountSummary {
                id: account.id(),
                kind,
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

    // Linked issues belong to the Jira and GitHub connectors, never to Confluence, whose
    // account is an Atlassian one too.
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

/// The connectors among `connectors` that the link watcher reads: Jira and GitHub. Confluence's
/// account is an Atlassian one too, so the watcher's counts and passes are matched by kind as
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
