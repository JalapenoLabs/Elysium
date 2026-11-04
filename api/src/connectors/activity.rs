// Copyright © 2026 Jalapeno Labs

//! Where every loop that reads a connector reports its syncs, so the hub can show each
//! connector's status and the recent activity across all of them.
//!
//! A sync that recorded something, or failed, is stored as a run (`crate::models::connector_run`);
//! one that found nothing new is not, since a mailbox is read every minute. A connector is
//! `syncing` while a sync someone asked for waits or runs, or while one of its meetings is
//! processed. That is a mark on its row (`connectors.syncing_since`), so the API shows what the
//! engine is doing, and a sync asked for while the engine is down waits for it.
//!
//! Every change publishes `connector.changed` with the connector's id alone. A connector's
//! view names whose mailbox or meetings it reads, and every client hears every event, so
//! clients load what changed by id, which answers its owner alone.

use std::sync::Arc;

use tracing::{Level, event};
use uuid::Uuid;

use crate::database::Pool;
use crate::models::connector;
use crate::models::connector_run::{self, ConnectorRun};
use crate::realtime::{EventBus, ServerEvent, Signal};

/// Cheap to clone; clones share their connections.
#[derive(Clone)]
pub struct Activity {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
}

impl std::fmt::Debug for Activity {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Activity").finish_non_exhaustive()
    }
}

impl Activity {
    pub fn new(database: Pool, events: EventBus) -> Self {
        Self {
            inner: Arc::new(Inner { database, events }),
        }
    }

    /// Asks the engine to read `connector_id` now: marks it syncing and signals the engine,
    /// which reads it and clears the mark. Answers `false` when it is already syncing, so two
    /// syncs of one connector never run at once.
    ///
    /// # Errors
    /// Propagates database errors.
    pub async fn request_sync(&self, connector_id: Uuid) -> anyhow::Result<bool> {
        let mut connection = self.inner.database.get().await?;
        if !connector::mark_syncing(&mut connection, connector_id).await? {
            return Ok(false);
        }
        drop(connection);
        self.changed(connector_id);
        self.inner
            .events
            .signal(&Signal::ConnectorSync { connector_id });
        Ok(true)
    }

    /// Marks `connector_id` syncing until the returned guard is dropped. Answers `None` when it
    /// already is, or the mark cannot be written, which is logged.
    pub async fn begin(&self, connector_id: Uuid) -> Option<Syncing> {
        let marked = async {
            let mut connection = self.inner.database.get().await?;
            anyhow::Ok(connector::mark_syncing(&mut connection, connector_id).await?)
        };
        match marked.await {
            Ok(true) => {
                self.changed(connector_id);
                Some(self.syncing(connector_id))
            }
            Ok(false) => None,
            Err(error) => {
                event!(
                    name: "connectors.syncing.unmarked",
                    Level::WARN,
                    connector.id = %connector_id,
                    error.message = %format!("{error:#}"),
                    "cannot mark a connector syncing; it syncs without showing it",
                );
                None
            }
        }
    }

    /// The guard for `connector_id`'s mark, which someone else set: a sync the API asked for,
    /// which the engine now runs.
    pub fn syncing(&self, connector_id: Uuid) -> Syncing {
        Syncing {
            activity: self.clone(),
            connector_id,
        }
    }

    /// Stores `run` and tells clients its connector changed. A run that cannot be stored is
    /// logged: the sync itself already happened.
    pub async fn record(&self, run: ConnectorRun) {
        let connector_id = run.connector_id;
        let stored = async {
            let mut connection = self.inner.database.get().await?;
            connector_run::record(&mut connection, run).await?;
            anyhow::Ok(())
        };
        if let Err(error) = stored.await {
            event!(
                name: "connectors.run.unrecorded",
                Level::WARN,
                connector.id = %connector_id,
                error.message = %format!("{error:#}"),
                "a connector's sync happened but could not be recorded",
            );
        }
        self.changed(connector_id);
    }

    /// Tells clients `connector_id` changed, so they load it again.
    pub fn changed(&self, connector_id: Uuid) {
        self.inner
            .events
            .publish(&ServerEvent::ConnectorChanged { id: connector_id });
    }

    async fn clear(&self, connector_id: Uuid) {
        let cleared = async {
            let mut connection = self.inner.database.get().await?;
            anyhow::Ok(connector::clear_syncing(&mut connection, connector_id).await?)
        };
        if let Err(error) = cleared.await {
            event!(
                name: "connectors.syncing.uncleared",
                Level::ERROR,
                connector.id = %connector_id,
                error.message = %format!("{error:#}"),
                "cannot mark a connector done syncing; the engine clears it when it next starts",
            );
        }
        self.changed(connector_id);
    }
}

/// A connector marked syncing; dropping it marks it done, in the background.
#[derive(Debug)]
pub struct Syncing {
    activity: Activity,
    connector_id: Uuid,
}

impl Drop for Syncing {
    fn drop(&mut self) {
        let activity = self.activity.clone();
        let connector_id = self.connector_id;
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move { activity.clear(connector_id).await });
            }
            Err(_no_runtime) => event!(
                name: "connectors.syncing.abandoned",
                Level::WARN,
                connector.id = %connector_id,
                "a sync ended outside the runtime; the engine clears its mark when it next starts",
            ),
        }
    }
}
