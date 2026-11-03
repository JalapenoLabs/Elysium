// Copyright © 2026 Jalapeno Labs

//! Reads every active connector that reads through a browser on an interval and puts what it
//! reads on the timeline.
//!
//! # A pass
//!
//! For each active connector: read its account through its browser, with the reader its kind has
//! (`super::readers`), store each item as a timeline
//! record (an item already recorded for the connector is skipped, by its external id), and for
//! each new item that wants attention raise an inbox action item, owned by the connector's
//! person and recorded as the watcher of that service (`watcher:outlook`). Then store the
//! connector's sign-in state, cursor, and error, and publish what changed. A read that recorded
//! something, or failed, is reported to `super::activity` as a run.
//!
//! # The first read
//!
//! A connector's first signed-in read records what is already in the mailbox without raising
//! anything: connecting a mailbox should not fill the inbox with its backlog. The cursor
//! remembers that it happened (`{"baselined": true}`).
//!
//! # The reader's state
//!
//! A reader that keeps its own place, such as the newest message read in each conversation,
//! answers it as `Reading::state`; the cursor keeps it under `reader` and hands it back on the
//! next read. A read that answers none keeps the state it had.
//!
//! # What is published
//!
//! `timelineRecord.created` carries a record's id, kind, owner, and the item it raised, never
//! its content: every client of the event stream hears it, and a person's mail is theirs. Its
//! owner's pages, and Elysia's dispatcher, load the record by id.

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
use super::readers;
use super::{BrowserReader, ConnectorKind, Read, ReadInput, Reading, SignInState};
use crate::action_items::Actor;
use crate::browser::devtools::Devtools;
use crate::database::Pool;
use crate::models::action_item::{
    self, ActionItem, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::browser_session;
use crate::models::connector::{self, Connector, PollOutcome};
use crate::models::connector_run::ConnectorRun;
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineRecord};
use crate::realtime::{EventBus, ServerEvent};
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
        for connector in connectors {
            if let Err(error) = self.read_connector(&connector).await {
                event!(
                    name: "connectors.connector.failed",
                    Level::WARN,
                    connector.id = %connector.id,
                    error.message = %format!("{error:#}"),
                    "cannot record a connector's read",
                );
            }
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
        Ok(connector::list_active(&mut connection, &kinds).await?)
    }

    /// Reads one connector and records what came of it, a failed read included.
    async fn read_connector(&self, connector: &Connector) -> anyhow::Result<()> {
        let kind = ConnectorKind::parse(&connector.kind)
            .with_context(|| format!("unknown connector kind {:?}", connector.kind))?;
        let reader = readers::for_kind(kind, &self.inner.devtools)
            .with_context(|| format!("{:?} is not read through a browser", connector.kind))?;
        let browser_session_id = connector
            .browser_session_id
            .context("a browser-read connector has no browser")?;
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
                    cursor: next_cursor(connector, baselined, &reading),
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
                    cursor: connector.cursor.clone(),
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
        Ok(())
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
            let raises_item = baselined && read.wants_attention;
            let now = Utc::now();
            let Some((record, raised)) =
                store_read(&mut connection, connector, kind, read, raises_item, now).await?
            else {
                continue;
            };
            recorded += 1;

            let action_item_id = raised.as_ref().map(|item| item.record.id);
            if let Some(raised) = raised {
                publish_item_write(&self.inner.events, &mut connection, raised, &[], now)
                    .await
                    .map_err(|error| anyhow::anyhow!("cannot publish a raised item: {error}"))?;
            }
            self.inner
                .events
                .publish(&ServerEvent::TimelineRecordCreated {
                    id: record.id,
                    source_kind: record.source_kind,
                    created_by: record.created_by,
                    action_item_id,
                });
        }
        Ok(recorded)
    }
}

/// Stores a read as a record and, when `raises_item`, the inbox item it raises, in one
/// transaction: a record never points at an item that was not written, nor an item lack its
/// record. Answers nothing for an item the connector already recorded.
async fn store_read(
    connection: &mut AsyncPgConnection,
    connector: &Connector,
    kind: ConnectorKind,
    read: &Read,
    raises_item: bool,
    now: DateTime<Utc>,
) -> anyhow::Result<Option<(TimelineRecord, Option<Recorded<ActionItem>>)>> {
    let new = new_record(connector, kind, read);
    let stored = connection
        .transaction(async move |connection| {
            let Some(record) = timeline_record::insert_new(connection, new).await? else {
                return Ok(None);
            };
            if !raises_item {
                return Ok(Some((record, None)));
            }
            let created =
                action_item::create(connection, raised_item(read), Actor::Connector(kind), now)
                    .await
                    .map_err(|error| anyhow::anyhow!("cannot raise an inbox item: {error}"))?;
            let record =
                timeline_record::set_action_item(connection, record.id, created.record.id).await?;
            Ok::<_, anyhow::Error>(Some((record, Some(created))))
        })
        .await?;
    Ok(stored)
}

fn is_baselined(connector: &Connector) -> bool {
    connector.cursor["baselined"].as_bool().unwrap_or(false)
}

/// The cursor after a read: baselined once the connector has been read signed in, and the
/// reader's state, or what it had when the read answered none.
fn next_cursor(connector: &Connector, baselined: bool, reading: &Reading) -> serde_json::Value {
    let reader = reading
        .state
        .clone()
        .unwrap_or_else(|| connector.cursor["reader"].clone());
    let mut cursor = json!({ "baselined": baselined || reading.sign_in == SignInState::SignedIn });
    if !reader.is_null() {
        cursor["reader"] = reader;
    }
    cursor
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
        metadata: json!({}),
        created_by: connector.created_by,
    }
}

/// The inbox item a message raises: its subject, who sent it, and its preview.
fn raised_item(read: &Read) -> NewActionItem {
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
        }
    }

    #[test]
    fn the_first_signed_in_read_baselines_the_account() {
        let connector = connector_with_cursor(json!({}));
        assert_eq!(
            next_cursor(&connector, false, &Reading::nothing(SignInState::SignedIn)),
            json!({ "baselined": true })
        );
        assert_eq!(
            next_cursor(&connector, false, &Reading::nothing(SignInState::SignedOut)),
            json!({ "baselined": false })
        );
        assert_eq!(
            next_cursor(&connector, true, &Reading::nothing(SignInState::Unknown)),
            json!({ "baselined": true })
        );
    }

    #[test]
    fn a_readers_state_is_kept_until_a_read_answers_a_new_one() {
        let connector =
            connector_with_cursor(json!({ "baselined": true, "reader": { "C1": "100.1" } }));

        let signed_out = next_cursor(&connector, true, &Reading::nothing(SignInState::SignedOut));
        assert_eq!(signed_out["reader"], json!({ "C1": "100.1" }));

        let moved_on = Reading {
            state: Some(json!({ "C1": "200.2" })),
            ..Reading::nothing(SignInState::SignedIn)
        };
        assert_eq!(
            next_cursor(&connector, true, &moved_on),
            json!({ "baselined": true, "reader": { "C1": "200.2" } })
        );
    }

    fn connector_with_cursor(cursor: serde_json::Value) -> Connector {
        let now = Utc::now();
        Connector {
            id: Uuid::now_v7(),
            kind: "outlook".to_owned(),
            browser_session_id: Some(Uuid::now_v7()),
            sign_in_state: "signed_in".to_owned(),
            cursor,
            last_polled_at: None,
            last_error: None,
            is_active: true,
            created_by: Uuid::now_v7(),
            created_at: now,
            updated_at: now,
            storage_location_id: None,
            token_account_id: None,
            jira_credential_id: None,
            github_credential_id: None,
            content: json!({}),
        }
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
