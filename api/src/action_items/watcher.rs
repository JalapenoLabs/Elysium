// Copyright © 2026 Jalapeno Labs

//! The watcher: a poller that keeps links current and lands the writes Elysium owes.
//!
//! Every [`WATCH_INTERVAL`], and at once whenever a write asks for it
//! ([`Links::wake_watcher`]), one pass:
//!
//! 1. Lands every owed provider write: closes for resolved items' issues, and comments for
//!    primary links. A write that fails or waits keeps the provider's answer and is tried
//!    again next pass, until it lands or the user cancels it.
//! 2. For each credential a live link or container goes through, asks the provider for what
//!    changed since the credential's cursor, and applies each change to its item through
//!    [`rules`]. Each container's children are read whole and joined to, or taken out of,
//!    the initiative.
//! 3. Moves the credential's cursor to the moment the pass began, so a restart resumes there.
//!
//! Every change goes out on the event stream as the routes' writes do. Applying is
//! idempotent: a change read twice, or one Elysium made itself, finds the link already
//! recording it and does nothing.
//!
//! The watcher runs until the shutdown token is cancelled. A pass in flight is dropped at
//! that point; its database work rolls back, and a provider write that landed without being
//! recorded is read as already done on the next start, since every write reads the
//! provider's state first.

use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use diesel::result::Error as DieselError;
use diesel_async::pooled_connection::deadpool::Object;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use crate::action_items::links::rules::{self, Reaction};
use crate::action_items::links::{LinkError, Links, Remote, WriteOutcome};
use crate::action_items::{Actor, Transition, WorkError};
use crate::database::Pool;
use crate::models::action_item::{self, ActionItem, ActionItemState, NewActionItem};
use crate::models::action_item_event::{HistoryKind, Recorded};
use crate::models::action_item_link::{
    self, ActionItemLink, LinkCredential, LinkKind, LinkState, NewLink, Observation,
};
use crate::models::action_item_link_write::{self, LinkWrite, LinkWriteKind};
use crate::models::initiative_link::{self, InitiativeLink};
use crate::models::{action_item_comment, initiative, link_watch_cursor};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::action_items::{publish_item_links, publish_item_write};
use crate::routes::v1::initiatives::InitiativeLinkResponse;

/// How often the watcher reads every credential. A change in Jira or GitHub reaches Elysium
/// within about this long; each pass costs a few requests per credential and one per
/// container, well inside both providers' rate limits.
pub const WATCH_INTERVAL: Duration = Duration::from_secs(60);

/// The most owed writes one pass tries. Writes wait in order, so the rest are tried on the
/// next pass; this keeps a long outage's backlog from holding one pass for minutes.
const WRITES_PER_PASS: i64 = 100;

/// The longest title an item created for a container's child starts with, matching the
/// items table.
const ITEM_TITLE_MAX_CHARACTERS: usize = 500;

/// What the watcher works with. Cheap to clone; clones share every connection.
#[derive(Clone)]
pub struct WatchContext {
    pub database: Pool,
    pub events: EventBus,
    pub links: Links,
}

/// Written by hand because the pool has nothing useful to print.
impl std::fmt::Debug for WatchContext {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WatchContext")
            .field("links", &self.links)
            .finish_non_exhaustive()
    }
}

/// The running watcher, awaited at shutdown.
#[derive(Debug)]
pub struct Watcher {
    task: JoinHandle<()>,
}

impl Watcher {
    /// Starts the watcher on its own task. It stops when `shutdown` is cancelled.
    pub fn start(context: WatchContext, shutdown: CancellationToken) -> Self {
        Self {
            task: tokio::spawn(run(context, shutdown)),
        }
    }

    /// Waits for the watcher to finish after the shutdown token was cancelled.
    pub async fn stopped(self) {
        if let Err(error) = self.task.await {
            event!(
                name: "links.watcher.join.failed",
                Level::ERROR,
                error.message = %error,
                "the link watcher ended abnormally",
            );
        }
    }
}

/// Runs passes until `shutdown` is cancelled: one at once, then one every
/// [`WATCH_INTERVAL`] or whenever a write wakes it.
async fn run(context: WatchContext, shutdown: CancellationToken) {
    event!(name: "links.watcher.started", Level::INFO, "link watcher started");
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            () = pass(&context) => {}
        }
        tokio::select! {
            () = shutdown.cancelled() => break,
            () = tokio::time::sleep(WATCH_INTERVAL) => {}
            () = context.links.woken() => {}
        }
    }
    event!(name: "links.watcher.stopped", Level::INFO, "link watcher stopped");
}

/// One pass: lands owed writes, then reads every watched credential. Failures are logged
/// and recorded where the user sees them, never raised: one unreachable provider must not
/// stop the rest.
pub async fn pass(context: &WatchContext) {
    let started = Utc::now();
    if let Err(error) = land_writes(context).await {
        event!(
            name: "links.watcher.writes.failed",
            Level::ERROR,
            error.message = %error,
            error.chain = ?error,
            "the link watcher could not land owed writes",
        );
    }

    let credentials = match credentials(context).await {
        Ok(credentials) => credentials,
        Err(error) => {
            event!(
                name: "links.watcher.pass.failed",
                Level::ERROR,
                error.message = %error,
                "the link watcher could not read which credentials to watch",
            );
            return;
        }
    };
    for credential in credentials {
        if let Err(error) = watch_credential(context, credential, started).await {
            event!(
                name: "links.watcher.credential.failed",
                Level::WARN,
                credential.provider = credential.provider.as_str(),
                credential.id = %credential.id,
                error.message = %error,
                "the link watcher could not read a credential; it tries again next pass",
            );
            record_failure(context, credential, &error).await;
        }
    }
}

async fn credentials(context: &WatchContext) -> anyhow::Result<Vec<LinkCredential>> {
    let mut connection = connect(&context.database).await?;
    Ok(link_watch_cursor::credentials_to_watch(&mut connection).await?)
}

async fn record_failure(context: &WatchContext, credential: LinkCredential, error: &anyhow::Error) {
    let recorded = async {
        let mut connection = connect(&context.database).await?;
        link_watch_cursor::record_failure(&mut connection, credential, &error.to_string()).await?;
        anyhow::Ok(())
    };
    if let Err(record_error) = recorded.await {
        event!(
            name: "links.watcher.cursor.unrecorded",
            Level::ERROR,
            error.message = %record_error,
            "the link watcher could not record a failed pass",
        );
    }
}

/// Reads one credential's changes since its cursor and its containers' children, then
/// moves the cursor to `started`.
async fn watch_credential(
    context: &WatchContext,
    credential: LinkCredential,
    started: DateTime<Utc>,
) -> anyhow::Result<()> {
    let mut connection = connect(&context.database).await?;
    let links = action_item_link::watched(&mut connection, credential).await?;
    let containers = initiative_link::watched(&mut connection, credential).await?;
    let cursor = link_watch_cursor::find(&mut connection, credential).await?;
    let since = match cursor {
        Some(cursor) => Some(cursor.watched_through),
        None => action_item_link::oldest_created_at(&mut connection, credential).await?,
    };
    drop(connection);

    let provider = context.links.provider(credential.provider);
    if !links.is_empty() {
        let remotes = provider.changes(credential.id, &links, since).await?;
        for remote in remotes {
            let found = links
                .iter()
                .find(|link| link.external_id.eq_ignore_ascii_case(&remote.external_id));
            if let Some(link) = found {
                apply_change(context, link, &remote, Utc::now()).await?;
            }
        }
    }
    for container in &containers {
        sync_container(context, container, Utc::now()).await?;
    }

    let mut connection = connect(&context.database).await?;
    link_watch_cursor::advance(&mut connection, credential, started).await?;
    Ok(())
}

/// What a link records, as an [`Observation`] to compare with what the provider reports.
fn recorded_observation(link: &ActionItemLink) -> Observation {
    Observation {
        external_key: link.external_key.clone(),
        url: link.url.clone(),
        title: link.title.clone(),
        state: link.observed_state,
        owner: link.observed_owner(),
    }
}

/// Applies what the provider reports about a linked thing: the item reacts to a change of
/// state and follows a primary link's new assignee, then the link records the report. A
/// report that matches the record changes nothing.
///
/// # Errors
/// Propagates database failures.
pub async fn apply_change(
    context: &WatchContext,
    link: &ActionItemLink,
    remote: &Remote,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let observation = remote.observation();
    if observation == recorded_observation(link) {
        return Ok(());
    }

    let mut connection = connect(&context.database).await?;
    let actor = Actor::Watcher(link.provider);
    let written = connection
        .transaction(async |connection| {
            // Recorded first, so a resolve below owes no close to the issue that just
            // reached done; the rules read the state the link had before, from `link`.
            action_item_link::record_observation(connection, link.id, &observation).await?;
            let item = action_item::find(connection, link.action_item_id).await?;
            let mut written = Recorded {
                record: item,
                history: Vec::new(),
            };
            if written.record.deleted_at.is_none() {
                let reaction =
                    rules::react(link.observed_state, remote.state, written.record.state);
                apply_reaction(connection, link, reaction, actor, now, &mut written).await?;

                let new_owner = rules::owner_change(
                    link.is_primary,
                    &link.observed_owner(),
                    &remote.owner,
                    &written.record.owner(),
                );
                if let Some(owner) = new_owner {
                    let changes = action_item::ActionItemChanges {
                        owner: Some(owner),
                        ..action_item::ActionItemChanges::default()
                    };
                    let updated =
                        action_item::update(connection, link.action_item_id, changes, actor, now)
                            .await?;
                    merge(&mut written, updated);
                }
            }
            Ok::<_, WorkError>(written)
        })
        .await?;

    publish_item_write(&context.events, &mut connection, written, &[], now).await?;
    publish_item_links(&context.events, &mut connection, link.action_item_id).await?;
    if remote.state != link.observed_state {
        // A resolve may have owed closes to the item's other issues.
        context.links.wake_watcher();
    }
    Ok(())
}

/// Carries out one [`Reaction`], adding what it wrote to `written`.
async fn apply_reaction(
    connection: &mut AsyncPgConnection,
    link: &ActionItemLink,
    reaction: Reaction,
    actor: Actor,
    now: DateTime<Utc>,
    written: &mut Recorded<ActionItem>,
) -> Result<(), WorkError> {
    let transition = match reaction {
        Reaction::Nothing => return Ok(()),
        Reaction::RecordClosedUnmerged => {
            let entry = action_item_link::record_on_item(
                connection,
                link.action_item_id,
                HistoryKind::PullRequestClosed,
                actor,
                json!({ "linkId": link.id, "key": link.external_key, "url": link.url }),
                now,
            )
            .await?;
            written.history.push(entry);
            return Ok(());
        }
        Reaction::Resolve => Transition::Resolve,
        Reaction::Dismiss => Transition::Dismiss,
        Reaction::Reopen => Transition::Reopen,
    };
    let moved =
        action_item::transition(connection, link.action_item_id, transition, actor, now).await?;
    merge(written, moved);
    Ok(())
}

/// Folds a later write to the same item into `written`: its record, and its history after.
fn merge(written: &mut Recorded<ActionItem>, later: Recorded<ActionItem>) {
    written.record = later.record;
    written.history.extend(later.history);
}

/// Reads a container's children whole: each joins the initiative, with an item created
/// for it when it has none, and every item the container brought in that it no longer
/// holds leaves. A read cut short takes nobody out.
///
/// A container the provider cannot read records why and is tried again next pass; it does
/// not fail the credential.
///
/// # Errors
/// Propagates database failures.
pub async fn sync_container(
    context: &WatchContext,
    container: &InitiativeLink,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let provider = context.links.provider(container.provider);
    let children = match provider.children(container).await {
        Ok(children) => children,
        Err(error) => {
            event!(
                name: "links.watcher.container.unreadable",
                Level::INFO,
                container.id = %container.id,
                error.message = %error,
                "a container could not be read; it is tried again next pass",
            );
            let mut connection = connect(&context.database).await?;
            let message = describe(&error);
            let recorded =
                initiative_link::record_sync(&mut connection, container.id, Err(&message), now)
                    .await?;
            publish_container(&context.events, recorded);
            return Ok(());
        }
    };

    let mut connection = connect(&context.database).await?;
    let before = action_item::ids_via_container(&mut connection, container.id).await?;
    let project_ids = initiative::project_ids(&mut connection, &[container.initiative_id])
        .await?
        .remove(&container.initiative_id)
        .unwrap_or_default();

    let mut held = Vec::with_capacity(children.items.len());
    for remote in &children.items {
        if let Some(item_id) = adopt_child(
            context,
            &mut connection,
            container,
            remote,
            &project_ids,
            now,
        )
        .await?
        {
            held.push(item_id);
        }
    }

    if !children.truncated {
        for item_id in before.into_iter().filter(|item_id| !held.contains(item_id)) {
            let left = action_item::leave_initiative_via(
                &mut connection,
                item_id,
                container.initiative_id,
                container.id,
                Actor::Watcher(container.provider),
                now,
            )
            .await?;
            publish_item_write(
                &context.events,
                &mut connection,
                left,
                &[container.initiative_id],
                now,
            )
            .await?;
        }
    }

    let recorded = initiative_link::record_sync(
        &mut connection,
        container.id,
        Ok((&children.title, children.truncated)),
        now,
    )
    .await?;
    publish_container(&context.events, recorded);
    Ok(())
}

/// Makes one of a container's children a member of its initiative, creating its item when
/// no item links to it yet. Returns the item, or `None` for a child whose item is deleted,
/// which stays out.
async fn adopt_child(
    context: &WatchContext,
    connection: &mut AsyncPgConnection,
    container: &InitiativeLink,
    remote: &Remote,
    project_ids: &[Uuid],
    now: DateTime<Utc>,
) -> anyhow::Result<Option<Uuid>> {
    let credential = container.credential();
    let actor = Actor::Watcher(container.provider);
    let existing = action_item_link::find_by_external(
        connection,
        credential,
        LinkKind::Issue,
        &remote.external_id,
    )
    .await?;

    let item_id = if let Some(link) = existing {
        let item = action_item::find(connection, link.action_item_id).await?;
        if item.deleted_at.is_some() {
            return Ok(None);
        }
        item.id
    } else {
        let created = create_child(connection, credential, remote, project_ids, actor, now).await?;
        let Some(link) = created.links.first().cloned() else {
            return Ok(None);
        };
        publish_item_write(&context.events, connection, created.item, &[], now).await?;
        publish_item_links(&context.events, connection, link.action_item_id).await?;
        // The child was recorded open, so a child already done resolves now, through the
        // same rules as any change, with the watcher as the actor.
        apply_change(context, &link, remote, now).await?;
        link.action_item_id
    };

    let joined = action_item::join_initiative_via(
        connection,
        item_id,
        container.initiative_id,
        container.id,
        actor,
        now,
    )
    .await?;
    publish_item_write(&context.events, connection, joined, &[], now).await?;
    Ok(Some(item_id))
}

/// Creates the item for a container's child: open, since linking the container accepted
/// it, owned as the provider reports, in the initiative's projects, and starting from the
/// child's priority and due date. Its link records the child as open.
async fn create_child(
    connection: &mut AsyncPgConnection,
    credential: LinkCredential,
    remote: &Remote,
    project_ids: &[Uuid],
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<action_item_link::Linked, WorkError> {
    let title = if remote.title.trim().is_empty() {
        remote.key.clone()
    } else {
        remote
            .title
            .trim()
            .chars()
            .take(ITEM_TITLE_MAX_CHARACTERS)
            .collect()
    };
    let new_item = NewActionItem {
        title,
        notes: String::new(),
        state: ActionItemState::Open,
        priority: remote.starting_priority(),
        due_at: remote.starting_due_at(),
        owner: remote.owner.clone(),
        project_ids: project_ids.to_vec(),
        initiative_ids: Vec::new(),
    };
    let new_link = NewLink {
        credential,
        kind: LinkKind::Issue,
        external_id: remote.external_id.clone(),
        observation: Observation {
            state: LinkState::Open,
            ..remote.observation()
        },
    };
    action_item_link::create_linked_item(connection, new_item, new_link, actor, now).await
}

fn publish_container(events: &EventBus, container: InitiativeLink) {
    events.publish(&ServerEvent::InitiativeLinkUpserted(
        InitiativeLinkResponse::from(container),
    ));
}

/// Tries every owed write once, oldest first.
///
/// # Errors
/// Propagates a failure to read the owed writes; a write that fails is recorded on itself.
pub async fn land_writes(context: &WatchContext) -> anyhow::Result<()> {
    let owed = {
        let mut connection = connect(&context.database).await?;
        action_item_link_write::owed(&mut connection, WRITES_PER_PASS).await?
    };
    for write in owed {
        if let Err(error) = land(context, &write).await {
            event!(
                name: "links.watcher.write.unrecorded",
                Level::ERROR,
                link.write.id = %write.id,
                error.message = %error,
                "the outcome of an owed write could not be recorded; it is tried again",
            );
        }
    }
    Ok(())
}

/// Tries one owed write and records how it went.
async fn land(context: &WatchContext, write: &LinkWrite) -> anyhow::Result<()> {
    let mut connection = connect(&context.database).await?;
    let link = match action_item_link::find(&mut connection, write.link_id).await {
        Ok(link) => link,
        // The link went while the write waited, taking the write with it.
        Err(DieselError::NotFound) => return Ok(()),
        Err(other) => return Err(other.into()),
    };
    let item = action_item::find(&mut connection, link.action_item_id).await?;

    let body = match write.kind {
        LinkWriteKind::Close => {
            // An item reopened or deleted since it resolved no longer wants its issues
            // closed.
            if item.state != ActionItemState::Resolved || item.deleted_at.is_some() {
                event!(
                    name: "links.watcher.write.dropped",
                    Level::INFO,
                    link.id = %link.id,
                    "an owed close was dropped: its item is no longer resolved",
                );
                action_item_link_write::complete(&mut connection, write.id).await?;
                publish_item_links(&context.events, &mut connection, link.action_item_id).await?;
                return Ok(());
            }
            None
        }
        LinkWriteKind::Comment => {
            let comment_id = write
                .comment_id
                .context("a comment write names its comment")?;
            Some(
                action_item_comment::find(&mut connection, comment_id)
                    .await?
                    .body,
            )
        }
    };
    drop(connection);

    let provider = context.links.provider(link.provider);
    let outcome = match &body {
        None => provider.close(&link).await,
        Some(body) => provider
            .comment(&link, body)
            .await
            .map(|()| WriteOutcome::Landed),
    };

    let now = Utc::now();
    let mut connection = connect(&context.database).await?;
    match outcome {
        Ok(WriteOutcome::Landed | WriteOutcome::AlreadyDone) => {
            action_item_link_write::complete(&mut connection, write.id).await?;
            if write.kind == LinkWriteKind::Close {
                action_item_link::record_state(&mut connection, link.id, LinkState::Done).await?;
            }
        }
        Ok(WriteOutcome::Waiting(reason)) => {
            action_item_link_write::record_attempt(&mut connection, write.id, &reason, now).await?;
        }
        Err(error) => {
            event!(
                name: "links.watcher.write.failed",
                Level::INFO,
                link.id = %link.id,
                link.write.kind = write.kind.as_str(),
                error.message = %error,
                "an owed write failed; it is tried again next pass",
            );
            let message = describe(&error);
            action_item_link_write::record_attempt(&mut connection, write.id, &message, now)
                .await?;
        }
    }
    publish_item_links(&context.events, &mut connection, link.action_item_id).await?;
    Ok(())
}

/// A provider failure as the user reads it. A fault inside Elysium is logged in full and
/// described without its internals.
fn describe(error: &LinkError) -> String {
    match error {
        LinkError::Internal(internal) => {
            event!(
                name: "links.watcher.internal_failure",
                Level::ERROR,
                error.message = %internal,
                error.chain = ?internal,
                "a link call failed inside Elysium",
            );
            "Elysium could not complete the call; its logs say why".to_owned()
        }
        other => other.to_string(),
    }
}

async fn connect(database: &Pool) -> anyhow::Result<Object<AsyncPgConnection>> {
    database
        .get()
        .await
        .context("no database connection available")
}

#[cfg(test)]
mod tests;
