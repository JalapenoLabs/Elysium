// Copyright © 2026 Jalapeno Labs

//! Reads someone asked for: Sync now, on a connector's page.
//!
//! The API marks the connector syncing and signals [`Signal::ConnectorSync`]; this reads it with
//! its kind's reader and clears the mark. At startup it first runs every connector still marked,
//! which is a sync asked for while the engine was down, or one an engine that stopped left
//! marked: reading it again is safe, since every reader picks up where its cursor left off.

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use crate::confluence::reader::Reader as ConfluenceReader;
use crate::connectors::ConnectorKind;
use crate::connectors::activity::Activity;
use crate::connectors::ingest::Ingest;
use crate::database::Pool;
use crate::meetings::Meetings;
use crate::models::connector::{self, Connector};
use crate::realtime::{EventBus, Signal};
use crate::youtrack::reader::YoutrackReader;

/// Every reader a sync may go through.
#[derive(Clone)]
pub struct Readers {
    pub database: Pool,
    pub events: EventBus,
    pub activity: Activity,
    pub ingest: Ingest,
    pub confluence: ConfluenceReader,
    pub youtrack: YoutrackReader,
    pub meetings: Meetings,
}

impl std::fmt::Debug for Readers {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Readers").finish_non_exhaustive()
    }
}

/// Runs every sync asked for, until `shutdown`. A read under way when it is cancelled stops
/// where it is; its mark is left, so the next start reads it again.
pub fn start(readers: Readers, shutdown: CancellationToken) -> JoinHandle<()> {
    let mut signals = readers.events.signals();
    tokio::spawn(async move {
        for owed in still_marked(&readers).await {
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = run(&readers, &owed) => {}
            }
        }
        loop {
            let signal = tokio::select! {
                () = shutdown.cancelled() => return,
                signal = signals.next() => signal,
            };
            match signal {
                Some(Signal::ConnectorSync { connector_id }) => {
                    tokio::select! {
                        () = shutdown.cancelled() => return,
                        () = run_by_id(&readers, connector_id) => {}
                    }
                }
                // Asked for while this listener fell behind or lost its subscription.
                Some(Signal::Resync) => {
                    for owed in still_marked(&readers).await {
                        tokio::select! {
                            () = shutdown.cancelled() => return,
                            () = run(&readers, &owed) => {}
                        }
                    }
                }
                Some(_) => {}
                None => return,
            }
        }
    })
}

/// Every connector marked syncing.
async fn still_marked(readers: &Readers) -> Vec<Connector> {
    let listed = async {
        let mut connection = readers.database.get().await?;
        anyhow::Ok(connector::list_syncing(&mut connection).await?)
    };
    match listed.await {
        Ok(connectors) => connectors,
        Err(error) => {
            event!(
                name: "connectors.sync.list_failure",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "cannot read which connectors wait for a sync; they wait for the next start",
            );
            Vec::new()
        }
    }
}

async fn run_by_id(readers: &Readers, connector_id: Uuid) {
    let found = async {
        let mut connection = readers.database.get().await?;
        anyhow::Ok(connector::find(&mut connection, connector_id).await?)
    };
    match found.await {
        Ok(connector) => run(readers, &connector).await,
        Err(error) => event!(
            name: "connectors.sync.unreadable",
            Level::WARN,
            connector.id = %connector_id,
            error.message = %format!("{error:#}"),
            "cannot read a connector someone asked to sync; it may have been removed",
        ),
    }
}

/// Reads `connector` with its kind's reader, then clears its mark.
async fn run(readers: &Readers, connector: &Connector) {
    // Dropping it clears the mark, however the read ends.
    let _syncing = readers.activity.syncing(connector.id);
    match ConnectorKind::parse(&connector.kind) {
        Some(ConnectorKind::Outlook | ConnectorKind::Gmail) => {
            readers.ingest.read_now(connector).await;
        }
        // Teams and Slack read messages through ingest and meetings (Teams' recordings, Slack's
        // huddles) through the meetings loop, which skips a connector whose meetings are off.
        Some(ConnectorKind::Teams | ConnectorKind::Slack) => {
            readers.ingest.read_now(connector).await;
            readers.meetings.sync(connector).await;
        }
        Some(ConnectorKind::Confluence) => readers.confluence.read_now(connector).await,
        Some(ConnectorKind::Youtrack) => readers.youtrack.read_now(connector).await,
        Some(ConnectorKind::Fireflies) => readers.meetings.sync(connector).await,
        // The link watcher reads these; the API wakes it without marking them.
        Some(ConnectorKind::Jira | ConnectorKind::Github) => {
            readers.events.signal(&Signal::LinkWatcher);
        }
        None => event!(
            name: "connectors.sync.unknown_kind",
            Level::WARN,
            connector.id = %connector.id,
            connector.kind = %connector.kind,
            "a connector of a kind this build does not know was asked to sync",
        ),
    }
}
