// Copyright © 2026 Jalapeno Labs

//! Reads every active connector read through a browser on an interval and puts what it reads
//! on the timeline.
//!
//! # A pass
//!
//! For each active connector: read it through its browser, with the reader `readers::for_kind`
//! answers for its kind, from the state its last read left (`cursor.reader`) and over its
//! content. Store each item as a timeline
//! record (an item already recorded for the connector is skipped, by its external id), and for
//! each new item that wants attention raise an inbox action item, owned by the connector's
//! person and recorded as the watcher of that service (`watcher:outlook`). Then store the
//! connector's sign-in state, cursor (`{ baselined, reader }`, the reader's next state), and
//! error, and publish what changed. A read that recorded
//! something, or failed, is reported to `super::activity` as a run.
//!
//! # Refreshing
//!
//! Each pass also refreshes at most one browser, the one most overdue (`super::refresh`): it
//! loads the browser's service again so its session renews, and that connector's read in the
//! same pass then records whether it is still signed in.
//!
//! # The first read
//!
//! A connector's first signed-in read records what is already in the mailbox without raising
//! anything: connecting a mailbox should not fill the inbox with its backlog. The cursor
//! remembers that it happened (`{"baselined": true}`).
//!
//! # What is published
//!
//! `timelineRecord.created` carries a record's id, kind, owner, and the item it raised, never
//! its content: every client of the event stream hears it, and a person's mail is theirs. Its
//! owner's pages, and Elysia's dispatcher, load the record by id.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde_json::json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use super::activity::Activity;
use super::{BrowserReader, ConnectorKind, Read, ReadInput, Reading, SignInState};
use super::{readers, refresh};
use crate::action_items::Actor;
use crate::browser::devtools::Devtools;
use crate::conversations::{self, Gathered};
use crate::database::Pool;
use crate::models::action_item::{
    self, ActionItem, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::browser_session::{self, BrowserSession};
use crate::models::connector::{self, Connector, PollOutcome};
use crate::models::connector_run::ConnectorRun;
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineRecord};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_write;

/// How often every connector is read. A minute keeps new mail prompt without reading a page
/// more often than a person glancing at it would.
const INGEST_INTERVAL: Duration = Duration::from_mins(1);

/// The longest item title raised from a message; the record keeps the whole subject.
const ITEM_TITLE_MAX_CHARACTERS: usize = 200;

#[derive(Clone)]
pub struct Ingest {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    devtools: Devtools,
    activity: Activity,
}

impl std::fmt::Debug for Ingest {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Ingest").finish_non_exhaustive()
    }
}

impl Ingest {
    pub fn new(database: Pool, events: EventBus, devtools: Devtools, activity: Activity) -> Self {
        Self {
            inner: Arc::new(Inner {
                database,
                events,
                devtools,
                activity,
            }),
        }
    }

    /// The `DevTools` client every mailbox is read through, for checks that read one.
    pub fn devtools(&self) -> &Devtools {
        &self.inner.devtools
    }

    /// Reads one connector now, as a pass would, for a sync someone asked for.
    pub async fn read_now(&self, connector: &Connector) {
        if let Err(error) = self.read_connector(connector).await {
            event!(
                name: "connectors.sync.failed",
                Level::WARN,
                connector.id = %connector.id,
                error.message = %format!("{error:#}"),
                "cannot record a connector's read",
            );
        }
    }

    /// Runs a pass every [`INGEST_INTERVAL`] until `shutdown` is cancelled.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let ingest = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(INGEST_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    _tick = interval.tick() => ingest.pass().await,
                }
            }
        })
    }

    async fn pass(&self) {
        let connectors = match self.active_connectors().await {
            Ok(connectors) => connectors,
            Err(error) => {
                event!(
                    name: "connectors.pass.failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "cannot list the connectors to read",
                );
                return;
            }
        };
        let due = self.claim_refresh(&connectors).await;
        for connector in connectors {
            let refreshing = due
                .as_ref()
                .filter(|browser| connector.browser_session_id == Some(browser.id));
            if let Some(browser) = refreshing {
                self.refresh(&connector, browser).await;
            }
            match self.read_connector(&connector).await {
                // The read straight after a refresh is its sign-in check.
                Ok(read) if refreshing.is_some() => event!(
                    name: "browser.refresh.checked",
                    Level::INFO,
                    browser.session.id = ?connector.browser_session_id,
                    connector.kind = %connector.kind,
                    connector.sign_in = %read.sign_in_state,
                    "read a connector after refreshing its browser",
                ),
                Ok(_read) => {}
                Err(error) => event!(
                    name: "connectors.connector.failed",
                    Level::WARN,
                    connector.id = %connector.id,
                    error.message = %format!("{error:#}"),
                    "cannot record a connector's read",
                ),
            }
        }
    }

    /// Picks the browser due a refresh among those `connectors` read through, if one is, and
    /// records the refresh before it is attempted, so one that fails waits its day like any
    /// other rather than being retried every pass.
    async fn claim_refresh(&self, connectors: &[Connector]) -> Option<BrowserSession> {
        let claimed = async {
            let mut connection = self
                .inner
                .database
                .get()
                .await
                .context("no database connection available")?;
            let read_through: HashSet<Uuid> = connectors
                .iter()
                .filter_map(|connector| connector.browser_session_id)
                .collect();
            let mut browsers = browser_session::list_all(&mut connection).await?;
            browsers.retain(|browser| read_through.contains(&browser.id));
            let now = Utc::now();
            let Some(due) = refresh::due(&browsers, now) else {
                return anyhow::Ok(None);
            };
            browser_session::record_refreshed(&mut connection, due.id, now)
                .await
                .context("cannot record the browser's refresh")?;
            Ok(Some(due.clone()))
        };
        match claimed.await {
            Ok(due) => due,
            Err(error) => {
                event!(
                    name: "browser.refresh.unclaimed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "cannot pick the browser due a refresh; trying again next pass",
                );
                None
            }
        }
    }

    /// Loads `connector`'s service again in `browser`, logging what came of it; the read that
    /// follows records whether it is still signed in.
    async fn refresh(&self, connector: &Connector, browser: &BrowserSession) {
        let Some(kind) = ConnectorKind::parse(&connector.kind) else {
            event!(
                name: "browser.refresh.failed",
                Level::WARN,
                browser.session.id = %browser.id,
                connector.kind = %connector.kind,
                "cannot refresh a browser for a connector kind this release does not know",
            );
            return;
        };
        match refresh::refresh(&self.inner.devtools, kind, browser).await {
            Ok(refreshed) => event!(
                name: "browser.refresh.done",
                Level::INFO,
                browser.session.id = %browser.id,
                connector.kind = kind.as_str(),
                browser.refresh.outcome = refreshed.as_str(),
                "refreshed a browser's service so its session renews",
            ),
            Err(error) => event!(
                name: "browser.refresh.failed",
                Level::WARN,
                browser.session.id = %browser.id,
                connector.kind = kind.as_str(),
                error.message = %error,
                "cannot refresh a browser's service; trying again in a day",
            ),
        }
    }

    async fn active_connectors(&self) -> anyhow::Result<Vec<Connector>> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let kinds = ConnectorKind::BROWSER_READ.map(ConnectorKind::as_str);
        let mut connectors = connector::list_active(&mut connection, &kinds).await?;
        // One waiting for its browser has nothing to read until its person starts one.
        connectors.retain(|connector| connector.browser_session_id.is_some());
        Ok(connectors)
    }

    /// Reads one connector and records what came of it, a failed read included. Answers the
    /// connector as recorded.
    async fn read_connector(&self, connector: &Connector) -> anyhow::Result<Connector> {
        let kind = ConnectorKind::parse(&connector.kind)
            .with_context(|| format!("unknown connector kind {:?}", connector.kind))?;
        let reader = readers::for_kind(kind, &self.inner.devtools)
            .with_context(|| format!("{:?} is not read through a browser", connector.kind))?;
        let browser_session_id = connector
            .browser_session_id
            .context("this connector reads through a browser, and has none")?;
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        // Removing a browser removes its connectors, so one missing here was removed mid-pass.
        let browser = browser_session::find(&mut connection, browser_session_id)
            .await?
            .context("the connector's browser was removed while it was read")?;
        drop(connection);

        let started = Utc::now();
        let input = ReadInput {
            state: &connector.cursor["reader"],
            content: &connector.content,
        };
        let mut run = None;
        let outcome = match reader.read(&browser, input).await {
            Ok(reading) => {
                let baselined = is_baselined(connector);
                let recorded = self
                    .record_reading(connector, reader.kind(), &reading, baselined)
                    .await?;
                if recorded > 0 {
                    run = Some(ConnectorRun::succeeded(connector.id, started, recorded));
                }
                PollOutcome {
                    sign_in_state: reading.sign_in.as_str().to_owned(),
                    cursor: next_cursor(
                        baselined,
                        reading.sign_in,
                        reading.state.as_ref().unwrap_or(input.state),
                    ),
                    last_polled_at: Some(Utc::now()),
                    last_error: None,
                }
            }
            Err(error) => {
                run = Some(ConnectorRun::failed(
                    connector.id,
                    started,
                    &error.to_string(),
                ));
                PollOutcome {
                    sign_in_state: SignInState::Unknown.as_str().to_owned(),
                    // Nothing of the cursor changes; it is merged, not replaced.
                    cursor: json!({}),
                    last_polled_at: Some(Utc::now()),
                    last_error: Some(error.to_string()),
                }
            }
        };

        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let updated = connector::record_poll(&mut connection, connector.id, outcome).await?;
        drop(connection);
        let changed = updated.sign_in_state != connector.sign_in_state
            || updated.last_error != connector.last_error;
        match run {
            // Recording a run announces the change too.
            Some(run) => self.inner.activity.record(run).await,
            None if changed => self.inner.activity.changed(connector.id),
            None => {}
        }
        Ok(updated)
    }

    /// Stores each new item, oldest first so the timeline reads in order, and raises an inbox
    /// item for each that wants attention once the connector is baselined. Answers how many
    /// records it stored.
    async fn record_reading(
        &self,
        connector: &Connector,
        kind: ConnectorKind,
        reading: &Reading,
        baselined: bool,
    ) -> anyhow::Result<usize> {
        let mut recorded = 0;
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        for read in reading.items.iter().rev() {
            let raised = (baselined && read.wants_attention).then(|| raised_item(read));
            if record(
                &mut connection,
                &self.inner.events,
                connector,
                kind,
                read,
                raised,
                baselined,
            )
            .await?
            {
                recorded += 1;
            }
        }
        Ok(recorded)
    }
}

/// Stores `read` as a timeline record of `connector`'s, with the inbox item `raised` when one
/// is given, and publishes both. Answers whether it was new: an item the connector already
/// recorded, by its external id, is skipped and nothing is published.
///
/// When `gathers`, a message also joins the conversation it was said in
/// (`crate::conversations::gather`), in the same transaction, so work starts on it once its
/// conversation goes quiet. Only a baselined connector's messages gather, as only they raise
/// items: connecting an account must not start work on its backlog.
///
/// Every connector that ingests records through here, so a record and the item it raises are
/// always written together and announced the same way.
///
/// # Errors
/// Propagates any database error, and a raised item that cannot be published.
pub(crate) async fn record(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    connector: &Connector,
    kind: ConnectorKind,
    read: &Read,
    raised: Option<NewActionItem>,
    gathers: bool,
) -> anyhow::Result<bool> {
    let now = Utc::now();
    let Some(stored) = store_read(connection, connector, kind, read, raised, gathers, now).await?
    else {
        return Ok(false);
    };
    let action_item_id = stored.raised.as_ref().map(|item| item.record.id);
    if let Some(raised) = stored.raised {
        publish_item_write(events, connection, raised, &[], now)
            .await
            .map_err(|error| anyhow::anyhow!("cannot publish a raised item: {error}"))?;
    }
    events.publish(&ServerEvent::TimelineRecordCreated {
        id: stored.record.id,
        source_kind: stored.record.source_kind,
        created_by: stored.record.created_by,
        action_item_id,
        conversation_id: stored.gathered.map(|gathered| gathered.conversation_id),
    });
    // The engine's conversation loop starts its quiet window now rather than at its next look.
    if stored.gathered.is_some_and(|gathered| gathered.wakes) {
        events.signal(&Signal::Conversations);
    }
    Ok(true)
}

/// What [`store_read`] wrote for a read it had not recorded before.
struct Stored {
    record: TimelineRecord,
    raised: Option<Recorded<ActionItem>>,
    gathered: Option<Gathered>,
}

/// Stores a read as a record, with the inbox item `raised` when one is given and its place in
/// its conversation when it `gathers`, in one transaction: a record never points at an item that
/// was not written, nor an item lack its record, and no message waits in a conversation without
/// its record. Answers nothing for an item the connector already recorded.
async fn store_read(
    connection: &mut AsyncPgConnection,
    connector: &Connector,
    kind: ConnectorKind,
    read: &Read,
    raised: Option<NewActionItem>,
    gathers: bool,
    now: DateTime<Utc>,
) -> anyhow::Result<Option<Stored>> {
    let new = new_record(connector, kind, read);
    let conversation = read.conversation.as_ref().filter(|_said_in| gathers);
    let wakes = read.wants_attention;
    let stored = connection
        .transaction(async move |connection| {
            let Some(mut record) = timeline_record::insert_new(connection, new).await? else {
                return Ok(None);
            };
            let gathered = match conversation {
                Some(conversation) => {
                    let message = conversations::Message {
                        record: &record,
                        conversation,
                        wakes,
                    };
                    conversations::gather(connection, connector, message, now).await?
                }
                None => None,
            };
            let raised = match raised {
                Some(raised) => {
                    let created =
                        action_item::create(connection, raised, Actor::Connector(kind), now)
                            .await
                            .map_err(|error| {
                                anyhow::anyhow!("cannot raise an inbox item: {error}")
                            })?;
                    record =
                        timeline_record::set_action_item(connection, record.id, created.record.id)
                            .await?;
                    Some(created)
                }
                None => None,
            };
            Ok::<_, anyhow::Error>(Some(Stored {
                record,
                raised,
                gathered,
            }))
        })
        .await?;
    Ok(stored)
}

fn is_baselined(connector: &Connector) -> bool {
    connector.cursor["baselined"].as_bool().unwrap_or(false)
}

/// Ingest's keys of the cursor after a read: baselined once the connector has been read signed
/// in, and the reader's own state for its next read. `connector::record_poll` merges them over
/// the stored cursor, so the keys another loop keeps there, such as the meetings listing's
/// `since` on a Teams connector, stay.
fn next_cursor(
    baselined: bool,
    sign_in: SignInState,
    reader: &serde_json::Value,
) -> serde_json::Value {
    json!({
        "baselined": baselined || sign_in == SignInState::SignedIn,
        "reader": reader,
    })
}

fn new_record(connector: &Connector, kind: ConnectorKind, read: &Read) -> NewTimelineRecord {
    NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: kind.as_str().to_owned(),
        connector_id: Some(connector.id),
        external_id: Some(read.external_id.clone()),
        occurred_at: read.occurred_at.unwrap_or_else(Utc::now),
        actor: read.actor.clone(),
        title: read.title.clone(),
        body: read.body.clone(),
        url: read.url.clone(),
        media: None,
        metadata: read.metadata.clone(),
        created_by: connector.created_by,
    }
}

/// The inbox item a read raises by default: its title (a message's subject), who it came
/// from, and its body. A kind with its own wording, such as Confluence, builds its own.
pub(crate) fn raised_item(read: &Read) -> NewActionItem {
    let subject = if read.title.is_empty() {
        format!("Mail from {}", read.actor)
    } else {
        read.title.clone()
    };
    NewActionItem {
        title: subject.chars().take(ITEM_TITLE_MAX_CHARACTERS).collect(),
        notes: format!("From {}\n\n{}", read.actor, read.body),
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(title: &str) -> Read {
        Read {
            external_id: "AAQk1".to_owned(),
            occurred_at: None,
            actor: "Ada <ada@example.com>".to_owned(),
            title: title.to_owned(),
            body: "Are we launching Friday?".to_owned(),
            url: None,
            wants_attention: true,
            metadata: json!({}),
            conversation: None,
        }
    }

    #[test]
    fn the_first_signed_in_read_baselines_the_account() {
        let none = serde_json::Value::Null;
        assert_eq!(
            next_cursor(false, SignInState::SignedIn, &none),
            json!({ "baselined": true, "reader": null })
        );
        assert_eq!(
            next_cursor(false, SignInState::SignedOut, &none),
            json!({ "baselined": false, "reader": null })
        );
        assert_eq!(
            next_cursor(true, SignInState::Unknown, &none),
            json!({ "baselined": true, "reader": null })
        );
    }

    #[test]
    fn the_readers_state_is_kept_beside_the_baseline() {
        let state = json!({ "since": 1_730_000_000_000_i64 });
        assert_eq!(
            next_cursor(true, SignInState::SignedIn, &state),
            json!({ "baselined": true, "reader": { "since": 1_730_000_000_000_i64 } })
        );
    }

    #[test]
    fn a_message_raises_an_inbox_item_owned_by_its_person() {
        let item = raised_item(&read("Launch"));
        assert_eq!(item.title, "Launch");
        assert_eq!(item.state, ActionItemState::Inbox);
        assert_eq!(item.owner, Owner::User);
        assert!(item.notes.starts_with("From Ada <ada@example.com>"));
        assert!(item.notes.ends_with("Are we launching Friday?"));
    }

    #[test]
    fn a_message_without_a_subject_is_named_by_its_sender() {
        assert_eq!(
            raised_item(&read("")).title,
            "Mail from Ada <ada@example.com>"
        );
    }
}
