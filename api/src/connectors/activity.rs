// Copyright © 2026 Jalapeno Labs

//! Where every loop that reads a connector reports its syncs, so the hub can show each
//! connector's status and the recent activity across all of them.
//!
//! A sync that recorded something, or failed, is stored as a run (`crate::models::connector_run`);
//! one that found nothing new is not, since a mailbox is read every minute. A connector is
//! `syncing` while a sync someone asked for runs, or while one of its meetings is processed,
//! which is kept in memory: it is only ever true while this process is working on it.
//!
//! Every change publishes `connector.changed` with the connector's id alone. A connector's
//! view names whose mailbox or meetings it reads, and every client hears every event, so
//! clients load what changed by id, which answers its owner alone.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use tracing::{Level, event};
use uuid::Uuid;

use crate::database::Pool;
use crate::models::connector_run::{self, ConnectorRun};
use crate::realtime::{EventBus, ServerEvent};

/// Cheap to clone; clones share one record of what is syncing.
#[derive(Clone)]
pub struct Activity {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    syncing: Mutex<HashSet<Uuid>>,
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
            inner: Arc::new(Inner {
                database,
                events,
                syncing: Mutex::new(HashSet::new()),
            }),
        }
    }

    /// Whether a sync of `connector_id` is running now.
    pub fn is_syncing(&self, connector_id: Uuid) -> bool {
        self.syncing().contains(&connector_id)
    }

    /// Marks `connector_id` syncing until the returned guard is dropped. Answers `None` when it
    /// already is, so two syncs of one connector never run at once.
    pub fn begin(&self, connector_id: Uuid) -> Option<Syncing> {
        if !self.syncing().insert(connector_id) {
            return None;
        }
        self.changed(connector_id);
        Some(Syncing {
            activity: self.clone(),
            connector_id,
        })
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

    fn syncing(&self) -> std::sync::MutexGuard<'_, HashSet<Uuid>> {
        self.inner
            .syncing
            .lock()
            .expect("no thread panics while holding the syncing set")
    }
}

/// A connector marked syncing; dropping it marks it done.
#[derive(Debug)]
pub struct Syncing {
    activity: Activity,
    connector_id: Uuid,
}

impl Drop for Syncing {
    fn drop(&mut self) {
        self.activity.syncing().remove(&self.connector_id);
        self.activity.changed(self.connector_id);
    }
}
