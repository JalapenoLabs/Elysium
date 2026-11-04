// Copyright © 2026 Jalapeno Labs

//! Work in progress that a restart of the stack would cut short.
//!
//! Auto-update restarts every container, so it waits until nothing is running that a
//! restart would interrupt: an agent turn on this host, or a backup of the databases
//! (`crate::backups`). Whatever starts such work
//! calls [`Activities::begin`] and holds the returned [`ActivityGuard`] for as long as the
//! work runs. Dropping the guard, on success, failure, cancellation, or a panic that
//! unwinds through it, ends the activity, so a turn can never be left counted by an early
//! return.
//!
//! ```ignore
//! let _activity = state.activity.begin(ActivityKind::AgentTurn, format!("session:{id}"));
//! run_the_turn().await?;
//! // The activity ends here, when `_activity` drops.
//! ```
//!
//! The registry is in-process, and starts empty, since a restart already ended whatever it
//! counted. The work it counts runs in the engine, while the API answers the updater, so a
//! [`mirrored`](Activities::mirrored) registry also keeps what it holds in Postgres
//! (`process_activities`), on a lease it renews: [`running`] reads every process's, and a process
//! that died stops counting once its lease runs out.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::Context as _;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use crate::database::Pool;
use crate::database::schema::process_activities;
use crate::models::user::SYSTEM_USER_ID;

/// How often a mirrored registry renews its lease while it holds anything.
const RENEW_INTERVAL: Duration = Duration::from_secs(15);

/// How long a lease lasts. Several renewals long, so a slow database does not let it lapse
/// under running work; short enough that a process that died stops holding updates back soon.
const LEASE: chrono::TimeDelta = chrono::TimeDelta::seconds(60);

/// What kind of work is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    /// An agent turn running on this host, for a coding session or for Elysia.
    AgentTurn,
    /// A backup of the databases, which a restart would leave half uploaded.
    DatabaseBackup,
}

/// One piece of running work, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub kind: ActivityKind,
    /// Names the work for a person reading Settings, such as `session:42`.
    pub label: String,
    pub started_at: DateTime<Utc>,
}

/// The work running in this process. Cloning is cheap: every clone shares one registry.
#[derive(Debug, Clone, Default)]
pub struct Activities {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Keyed by an id that only grows, so the map lists activities in the order they began.
    running: Mutex<BTreeMap<u64, Activity>>,
    next_id: AtomicU64,
    /// Told of every change, for the mirror; `None` for a registry kept in memory alone.
    changed: Option<watch::Sender<()>>,
}

impl Inner {
    /// The poison is ignored: the map holds plain values that every operation leaves whole,
    /// and a guard must be able to remove its entry while a panic unwinds.
    fn running(&self) -> MutexGuard<'_, BTreeMap<u64, Activity>> {
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Activities {
    /// A registry kept in memory alone, for a process that answers for nobody else.
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry that also keeps what it holds in `process_activities`, for every process to
    /// read with [`running`], until `shutdown`; its row is removed then.
    pub fn mirrored(database: Pool, shutdown: CancellationToken) -> (Self, JoinHandle<()>) {
        let (changed, watching) = watch::channel(());
        let activities = Self {
            inner: Arc::new(Inner {
                changed: Some(changed),
                ..Inner::default()
            }),
        };
        let mirror = Mirror {
            activities: activities.clone(),
            database,
            process_id: Uuid::now_v7(),
        };
        let task = tokio::spawn(mirror.run(watching, shutdown));
        (activities, task)
    }

    /// Records that `kind` of work began. It counts until the returned guard drops.
    #[must_use = "the activity ends as soon as the guard is dropped"]
    pub fn begin(&self, kind: ActivityKind, label: impl Into<String>) -> ActivityGuard {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let activity = Activity {
            kind,
            label: label.into(),
            started_at: Utc::now(),
        };
        self.inner.running().insert(id, activity);
        self.inner.announce();
        ActivityGuard {
            id,
            inner: Arc::clone(&self.inner),
        }
    }

    /// What is running, oldest first.
    pub fn snapshot(&self) -> Vec<Activity> {
        self.inner.running().values().cloned().collect()
    }
}

/// Keeps one activity counted. Dropping it ends the activity.
#[derive(Debug)]
pub struct ActivityGuard {
    id: u64,
    inner: Arc<Inner>,
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.inner.running().remove(&self.id);
        self.inner.announce();
    }
}

impl Inner {
    fn announce(&self) {
        if let Some(changed) = &self.changed {
            // An error only means the mirror stopped, at shutdown.
            let _ = changed.send(());
        }
    }
}

/// Keeps one process's activities in `process_activities`.
struct Mirror {
    activities: Activities,
    database: Pool,
    process_id: Uuid,
}

impl Mirror {
    async fn run(self, mut changed: watch::Receiver<()>, shutdown: CancellationToken) {
        let mut renewals = tokio::time::interval(RENEW_INTERVAL);
        renewals.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                _ = renewals.tick() => {}
                // Never closes while the registry is alive, and this task holds one.
                _ = changed.changed() => {}
            }
            if let Err(error) = self.write(&self.activities.snapshot()).await {
                event!(
                    name: "activity.mirror.failure",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "could not record what this process is running; auto-update may not see it",
                );
            }
        }
        // Whatever still runs is cut short by the stop; nothing is left to wait for.
        if let Err(error) = self.write(&[]).await {
            event!(
                name: "activity.mirror.clear_failure",
                Level::WARN,
                error.message = %format!("{error:#}"),
                "could not clear what this process was running; its lease runs out on its own",
            );
        }
    }

    /// Records `running` under a fresh lease, or removes the row when nothing runs.
    async fn write(&self, running: &[Activity]) -> anyhow::Result<()> {
        let mut connection = self
            .database
            .get()
            .await
            .context("no database connection to record activities")?;
        if running.is_empty() {
            diesel::delete(process_activities::table.find(self.process_id))
                .execute(&mut connection)
                .await?;
            return Ok(());
        }
        let activities = serde_json::to_value(running).expect("activities serialize");
        diesel::insert_into(process_activities::table)
            .values((
                process_activities::process_id.eq(self.process_id),
                process_activities::activities.eq(&activities),
                process_activities::expires_at.eq(Utc::now() + LEASE),
                process_activities::created_by.eq(SYSTEM_USER_ID),
            ))
            .on_conflict(process_activities::process_id)
            .do_update()
            .set((
                process_activities::activities.eq(excluded(process_activities::activities)),
                process_activities::expires_at.eq(excluded(process_activities::expires_at)),
            ))
            .execute(&mut connection)
            .await?;
        Ok(())
    }
}

/// What every live process is running, oldest first: what auto-update waits for.
///
/// # Errors
/// Propagates database errors.
pub async fn running(connection: &mut AsyncPgConnection) -> anyhow::Result<Vec<Activity>> {
    let leases: Vec<serde_json::Value> = process_activities::table
        .filter(process_activities::expires_at.gt(diesel::dsl::now))
        .select(process_activities::activities)
        .load(connection)
        .await
        .context("cannot read what Elysium is running")?;
    let mut running = Vec::new();
    for lease in leases {
        match serde_json::from_value::<Vec<Activity>>(lease) {
            Ok(activities) => running.extend(activities),
            Err(error) => {
                // Unreadable is not idle: an update must not restart the stack under it.
                event!(
                    name: "activity.lease.unreadable",
                    Level::WARN,
                    error.message = %error,
                    "a process's activities could not be read; counting it as busy",
                );
                running.push(Activity {
                    kind: ActivityKind::AgentTurn,
                    label: "unreadable".to_owned(),
                    started_at: Utc::now(),
                });
            }
        }
    }
    running.sort_by_key(|activity| activity.started_at);
    Ok(running)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_activity_counts_until_its_guard_drops() {
        let activities = Activities::new();
        assert!(activities.snapshot().is_empty());

        let guard = activities.begin(ActivityKind::AgentTurn, "session:1");
        assert!(!activities.snapshot().is_empty());

        drop(guard);
        assert!(activities.snapshot().is_empty());
    }

    #[test]
    fn clones_share_one_registry() {
        let activities = Activities::new();
        let _guard = activities
            .clone()
            .begin(ActivityKind::AgentTurn, "session:1");
        assert!(!activities.snapshot().is_empty());
    }

    #[test]
    fn the_snapshot_lists_running_work_oldest_first() {
        let activities = Activities::new();
        let first = activities.begin(ActivityKind::AgentTurn, "session:1");
        let _second = activities.begin(ActivityKind::AgentTurn, "elysia:7");
        let _third = activities.begin(ActivityKind::AgentTurn, "session:2");
        drop(first);

        let labels: Vec<String> = activities
            .snapshot()
            .into_iter()
            .map(|activity| activity.label)
            .collect();
        assert_eq!(labels, ["elysia:7", "session:2"]);
    }

    /// A turn that panics must not stay counted forever, or the deployment would never update.
    #[test]
    fn a_panic_that_unwinds_through_the_guard_ends_the_activity() {
        let activities = Activities::new();
        let registry = activities.clone();
        let outcome = std::panic::catch_unwind(move || {
            let _guard = registry.begin(ActivityKind::AgentTurn, "session:1");
            panic!("the turn failed");
        });

        assert!(outcome.is_err());
        assert!(activities.snapshot().is_empty());
    }

    /// Waits until every live process's activities number `count`, as the mirror writes them.
    async fn wait_for_running(connection: &mut AsyncPgConnection, count: usize) -> Vec<Activity> {
        for _attempt in 0..100 {
            let found = running(connection).await.expect("readable");
            if found.len() == count {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the mirror never recorded {count} activities");
    }

    // The engine holds the work and the API answers the updater, so what one process runs must
    // reach the other through the database, or an update would restart the stack under a turn.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn what_one_process_runs_is_read_by_every_other_until_it_ends() {
        let (url, mut connection) = crate::test_support::migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 2)
            .await
            .expect("Postgres accepts connections");
        let shutdown = CancellationToken::new();
        let (activities, mirror) = Activities::mirrored(database, shutdown.clone());

        let turn = activities.begin(ActivityKind::AgentTurn, "session:1");
        let found = wait_for_running(&mut connection, 1).await;
        assert_eq!(found[0].label, "session:1");

        drop(turn);
        wait_for_running(&mut connection, 0).await;

        let _backup = activities.begin(ActivityKind::DatabaseBackup, "backup:1");
        wait_for_running(&mut connection, 1).await;
        shutdown.cancel();
        mirror.await.expect("the mirror stops");
        assert!(
            running(&mut connection).await.expect("readable").is_empty(),
            "a process that stopped holds nothing"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_process_whose_lease_ran_out_holds_nothing() {
        let (_url, mut connection) = crate::test_support::migrated_database().await;
        let stale = serde_json::json!([{
            "kind": "agentTurn",
            "label": "session:9",
            "startedAt": "2026-11-05T09:00:00Z",
        }]);
        diesel::insert_into(process_activities::table)
            .values((
                process_activities::process_id.eq(Uuid::now_v7()),
                process_activities::activities.eq(&stale),
                process_activities::expires_at.eq(Utc::now() - chrono::TimeDelta::seconds(1)),
                process_activities::created_by.eq(SYSTEM_USER_ID),
            ))
            .execute(&mut connection)
            .await
            .expect("a lease that ran out");
        assert!(running(&mut connection).await.expect("readable").is_empty());
    }
}
