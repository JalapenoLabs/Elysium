// Copyright © 2026 Jalapeno Labs

//! Issues assigned to an issue connector's person come in as inbox items, each linked to its
//! issue. See `docs/action-items.md`, Assigned issues.
//!
//! # A read
//!
//! On every watcher pass, each active connector of a kind that provides issues
//! (`kinds::providing(Provides::Issues)`) is asked through its link provider for every open issue
//! assigned to its person, bounded by its content: Jira by its account's allowlist, GitHub by the
//! repositories it chose, YouTrack by the projects it chose
//! ([`crate::action_items::links::Provider::assigned`]). Each one the connector has not brought
//! in before raises one inbox item, linked to the issue as its primary, recorded as the watcher of
//! its service (`watcher:jira`), as ingest raises items. From then on the link is followed like
//! any other: the issue closing resolves the item.
//!
//! The first read brings in everything already assigned. A mailbox's first read only baselines,
//! since old mail is read; an assigned issue is owed work however old it is.
//!
//! Every pass reads the whole of what is assigned rather than what changed since a cursor, so a
//! connector whose content widens, or an issue assigned in a project just picked, comes in on the
//! next pass with no cursor to reset, and nothing is missed while a provider's clock and Elysium's
//! disagree. A read is a page of a hundred for most people, and stops at a thousand issues, most
//! recently updated first; past that, the rest come in once they are next updated. Items are made
//! oldest first, so the inbox lists them in the order they were last worked on.
//!
//! # Never twice
//!
//! `assigned_issues` keeps one row per connector and issue, written in the same transaction as
//! the item and its link, and the issue's link through the connector's account is unique too. So
//! an issue never raises a second item: not on the next pass, nor after a restart, nor when two
//! engines read at once (the second's transaction is refused and changes nothing), nor once its
//! item was dismissed, deleted, or unlinked. One already linked by hand through the same account
//! raises nothing; it is recorded as brought in, naming its item.
//!
//! # Nothing starts
//!
//! An issue arriving makes an item and its link, and nothing else. The item belongs to no
//! project, so no Project Manager hears of it, and Elysia's intake leaves the items and links a
//! watcher makes alone (`crate::elysia::intake`). It waits in the inbox for a person.
//!
//! What a connector records on the timeline is apart from this. YouTrack's reader records every
//! new issue in its chosen projects, assigned or not, and that record reaches Elysia as every
//! ingested record does; it is news of the project, not of the assignment.

use anyhow::Context;
use chrono::{DateTime, Utc};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::{AsyncConnection, AsyncPgConnection};
use tracing::{Level, event};
use uuid::Uuid;

use crate::action_items::links::Remote;
use crate::action_items::watcher::WatchContext;
use crate::action_items::{Actor, WorkError};
use crate::connectors::ConnectorKind;
use crate::models::action_item::{ActionItemState, NewActionItem, Owner};
use crate::models::action_item_link::{self, LinkCredential, LinkKind, LinkState, Linked, NewLink};
use crate::models::assigned_issue::{self, NewAssignedIssue};
use crate::models::connector::Connector;
use crate::routes::v1::action_items::{publish_item_links, publish_item_write};

/// Brings in what is assigned to `connector`'s person, read through `credential`, its account.
/// Answers how many items it raised.
///
/// # Errors
/// Propagates a read the provider refused or could not answer, and database failures. Issues
/// already brought in stay brought in; the next pass reads them all again.
pub async fn bring_in(
    context: &WatchContext,
    connector: &Connector,
    credential: LinkCredential,
) -> anyhow::Result<usize> {
    let kind = ConnectorKind::parse(&connector.kind)
        .with_context(|| format!("a connector of unknown kind {:?}", connector.kind))?;
    let provider = context.links.provider(credential.provider);
    let assigned = provider.assigned(credential.id, &connector.content).await?;
    if assigned.truncated {
        event!(
            name: "links.assigned.truncated",
            Level::INFO,
            connector.id = %connector.id,
            links.assigned.read = assigned.items.len(),
            "more issues are assigned than one read takes; the rest come in as they are updated",
        );
    }

    let mut connection = context
        .database
        .get()
        .await
        .context("no database connection available")?;
    let brought_in = assigned_issue::external_ids(&mut connection, connector.id).await?;
    let mut raised = 0;
    // An issue that cannot be stored is passed over, so it never holds back the ones after it,
    // and the pass then fails naming it, so the connector shows why until it comes in.
    let mut refused: Vec<(String, anyhow::Error)> = Vec::new();
    // The provider lists the most recently updated first; the oldest is made first, so the inbox
    // reads in the order they were last worked on.
    for remote in assigned.items.iter().rev() {
        if brought_in.contains(&remote.external_id) {
            continue;
        }
        // The provider's query asked for open issues assigned to this account; each is checked
        // again on the way back, so a provider that answers loosely brings nothing in.
        if remote.owner != Owner::User || remote.state != LinkState::Open {
            event!(
                name: "links.assigned.skipped",
                Level::DEBUG,
                connector.id = %connector.id,
                links.key = %remote.key,
                "an issue the provider listed as assigned is not the person's open issue",
            );
            continue;
        }
        let arrival = Arrival {
            connector,
            kind,
            credential,
            remote,
        };
        match arrive(context, &mut connection, arrival, Utc::now()).await {
            Ok(true) => raised += 1,
            Ok(false) => {}
            Err(error) => {
                event!(
                    name: "links.assigned.unstored",
                    Level::WARN,
                    connector.id = %connector.id,
                    links.key = %remote.key,
                    error.message = %format!("{error:#}"),
                    "an assigned issue could not be brought in; it is tried again next pass",
                );
                refused.push((remote.key.clone(), error));
            }
        }
    }
    if let Some((key, error)) = refused.first() {
        anyhow::bail!(
            "{} of the issues assigned could not be brought in, {key} among them: {error:#}",
            refused.len()
        );
    }
    Ok(raised)
}

/// One assigned issue coming in through one connector.
#[derive(Clone, Copy)]
struct Arrival<'read> {
    connector: &'read Connector,
    kind: ConnectorKind,
    credential: LinkCredential,
    remote: &'read Remote,
}

/// Brings one issue in, in one transaction: its item and link, unless the connector's account
/// already links it, and the row that says the connector brought it in. Answers whether it raised
/// an item.
async fn arrive(
    context: &WatchContext,
    connection: &mut AsyncPgConnection,
    arrival: Arrival<'_>,
    now: DateTime<Utc>,
) -> anyhow::Result<bool> {
    let actor = Actor::Connector(arrival.kind);
    let remote = arrival.remote;
    let new_item = NewActionItem {
        title: remote.item_title(),
        notes: String::new(),
        // Nobody accepted it yet: it arrived.
        state: ActionItemState::Inbox,
        priority: remote.starting_priority(),
        due_at: remote.starting_due_at(),
        owner: remote.owner.clone(),
        // Which projects it belongs to is decided after it arrives, by attribution or a person.
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    };
    let new_link = NewLink {
        credential: arrival.credential,
        kind: LinkKind::Issue,
        external_id: remote.external_id.clone(),
        observation: remote.observation(),
    };
    let connector_id = arrival.connector.id;
    let external_id = remote.external_id.clone();

    let written = connection
        .transaction(async move |connection| {
            let existing = action_item_link::find_by_external(
                connection,
                new_link.credential,
                LinkKind::Issue,
                &new_link.external_id,
            )
            .await?;
            let (action_item_id, raised) = if let Some(link) = existing {
                (link.action_item_id, None)
            } else {
                let linked = action_item_link::create_linked_item(
                    connection, new_item, new_link, actor, now,
                )
                .await?;
                (linked.item.record.id, Some(linked))
            };
            let recorded = assigned_issue::record(
                connection,
                NewAssignedIssue {
                    id: Uuid::now_v7(),
                    connector_id,
                    external_id,
                    action_item_id,
                    created_by: actor.user_id(),
                    created_at: now,
                },
            )
            .await?;
            if !recorded {
                // Another pass brought it in a moment ago; the item made here goes back.
                return Err(WorkError::Conflict(
                    "the connector brought that issue in already",
                ));
            }
            Ok::<Option<Linked>, WorkError>(raised)
        })
        .await;

    let raised = match written {
        Ok(raised) => raised,
        Err(error) if is_already_in(&error) => {
            event!(
                name: "links.assigned.already_in",
                Level::DEBUG,
                connector.id = %connector_id,
                links.key = %remote.key,
                "an assigned issue was brought in by another pass at the same moment",
            );
            return Ok(false);
        }
        Err(other) => return Err(other.into()),
    };
    let Some(linked) = raised else {
        event!(
            name: "links.assigned.linked_already",
            Level::DEBUG,
            connector.id = %connector_id,
            links.key = %remote.key,
            "an assigned issue was already linked to an item, which it joins without a second",
        );
        return Ok(false);
    };

    // The item is in once its transaction commits. Announcing it reads it back to publish, and
    // a read that fails then is said for what it is, never as the issue not coming in: the
    // next pass finds it brought in and leaves it be.
    let action_item_id = linked.item.record.id;
    let announced = async {
        publish_item_write(&context.events, connection, linked.item, &[], now).await?;
        publish_item_links(&context.events, connection, action_item_id).await
    };
    if let Err(error) = announced.await {
        event!(
            name: "links.assigned.unannounced",
            Level::WARN,
            connector.id = %connector_id,
            links.key = %remote.key,
            error.message = %format!("{error:#}"),
            "an assigned issue came in, but its item could not be announced; pages show it once reloaded",
        );
    }
    Ok(true)
}

/// Whether a refused arrival means the issue is in already, through a pass that wrote it while
/// this one ran: its link or its row was taken first.
fn is_already_in(error: &WorkError) -> bool {
    matches!(
        error,
        WorkError::Conflict(_)
            | WorkError::Database(DieselError::DatabaseError(
                DatabaseErrorKind::UniqueViolation,
                _
            ))
    )
}

#[cfg(test)]
mod tests;
