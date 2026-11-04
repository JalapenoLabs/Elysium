// Copyright © 2026 Jalapeno Labs

//! Backups of the stack's databases to a storage location off the host, every night.
//!
//! The databases are the three on the stack's Postgres: Elysium's own, Kratos's, and Hydra's.
//! Each is dumped with the server's own `pg_dump` ([`dump`]) and uploaded to the storage
//! location an admin chose (`workspace_settings.backup_storage_location_id`), under
//! [`files::DIRECTORY`]. After a backup succeeds, backups older than [`files::RETENTION_DAYS`]
//! are removed from it.
//!
//! Backups run in the engine ([`Backups::start`]). One starts three ways:
//!
//! - the nightly schedule, checked every [`CHECK_INTERVAL`] against the pure
//!   [`schedule::decide`] ([`Backups::back_up`]);
//! - an admin's Back up now, on Settings, Backups (`POST /api/v1/backups`);
//! - the deployment's own tooling, before it changes the stack (`POST /internal/backups`).
//!
//! The last two are the API's: [`Backups::begin`] records the backup as running, untaken, and
//! signals the engine ([`Signal::Backups`]), which takes it and runs it. A backup asked for while
//! the engine is down waits for it, rather than being lost.
//!
//! One runs at a time, guarded by a unique index on `database_backups`, and it holds an activity
//! ([`crate::activity`]) while it runs, so auto-update never restarts the stack under it. Every
//! attempt is a `database_backups` row, and every change is published as `backups.status`.
//!
//! Nothing fails silently. Every failure is logged at error level and recorded with its reason;
//! what the attempt uploaded is removed again, so the location only ever holds whole backups. A
//! night whose every scheduled attempt failed raises an inbox item. A night with no location
//! chosen backs up nothing, which Settings shows and the log warns of once a night.

pub mod dump;
pub mod files;
pub mod schedule;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, SubsecRound, Utc};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::pooled_connection::deadpool::Object;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::SecretString;
use serde::Serialize;
use tokio::task::JoinHandle;
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use crate::action_items::Actor;
use crate::activity::{Activities, ActivityKind};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::action_item::{self, ActionItemPriority, ActionItemState, NewActionItem, Owner};
use crate::models::database_backup::{
    self, BackupOutcome, DatabaseBackup, DatabaseBackupStatus, DatabaseBackupTrigger,
};
use crate::models::storage_location::{self, StorageLocation};
use crate::models::user::{self, SYSTEM_USER_ID};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::publish_item_write;
use crate::storage::Storage;
use dump::Dumper;
use schedule::Decision;

/// How often the schedule is checked. The slot is a fixed hour, so a backup starts within ten
/// minutes of it, or of the API starting after it.
const CHECK_INTERVAL: Duration = Duration::from_mins(10);

/// How many past backups Settings lists: three weeks of nights, with room for retries.
const HISTORY_LENGTH: i64 = 30;

/// Where dumps wait between `pg_dump` and their upload: the API's own data volume, since a
/// dump is as large as the databases and the container's own layer is not meant to grow.
pub const STAGING_DIRECTORY: &str = "/var/lib/elysium/backups";

/// Kratos's and Hydra's databases, as `compose.yml` names them in their DSNs. They live on
/// Elysium's Postgres server, which `migrate` creates them on.
const IDENTITY_DATABASES: [&str; 2] = ["kratos", "hydra"];

/// Why a backup that a restart cut short failed.
const INTERRUPTED: &str = "The engine stopped before the backup finished.";

/// The media type every dump is stored with: `pg_dump`'s custom format is binary.
const DUMP_CONTENT_TYPE: &str = "application/octet-stream";

diesel::define_sql_function! {
    /// The database the connection is to: Elysium's own, whatever `DATABASE_URL` names it.
    fn current_database() -> diesel::sql_types::Text;
}

/// Why a backup could not begin.
#[derive(Debug, thiserror::Error)]
pub enum BeginError {
    #[error("a backup is already running")]
    AlreadyRunning,
    #[error("no storage location is chosen for backups; choose one in Settings, Backups")]
    NoDestination,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<DieselError> for BeginError {
    fn from(error: DieselError) -> Self {
        Self::Internal(error.into())
    }
}

/// Where backups stand, for Settings and the `backups.status` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupsStatus {
    /// The location backups go to, or `None` while none is chosen and nothing is backed up.
    pub destination: Option<Destination>,
    /// Elysium's own database first, then Kratos's and Hydra's.
    pub databases: Vec<String>,
    pub retention_days: i64,
    /// When the schedule starts its next attempt; `None` while one runs or no location is
    /// chosen. A time already past means it starts within minutes.
    pub next_attempt_at: Option<DateTime<Utc>>,
    /// Newest first.
    pub history: Vec<DatabaseBackupView>,
}

/// The location backups go to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub id: Uuid,
    pub name: String,
}

/// One backup attempt.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseBackupView {
    pub id: Uuid,
    pub trigger: DatabaseBackupTrigger,
    pub status: DatabaseBackupStatus,
    pub storage_location_id: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub size_bytes: Option<i64>,
    pub error: Option<String>,
    pub created_by: Uuid,
}

impl From<DatabaseBackup> for DatabaseBackupView {
    fn from(backup: DatabaseBackup) -> Self {
        Self {
            id: backup.id,
            trigger: backup.trigger,
            status: backup.status,
            storage_location_id: backup.storage_location_id,
            started_at: backup.started_at,
            finished_at: backup.finished_at,
            size_bytes: backup.size_bytes,
            error: backup.error,
            created_by: backup.created_by,
        }
    }
}

/// What [`Backups`] needs.
#[derive(Clone)]
pub struct BackupsDeps {
    pub database: Pool,
    pub cipher: Arc<Cipher>,
    pub storage: Storage,
    pub events: EventBus,
    pub activity: Activities,
    pub dumper: Dumper,
    /// Normally [`STAGING_DIRECTORY`].
    pub staging_directory: PathBuf,
    /// Cancelled when the engine shuts down; a running backup stops and is recorded as failed.
    pub shutdown: CancellationToken,
}

/// The database backups. Cloning is cheap: every clone shares one state.
#[derive(Clone)]
pub struct Backups {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Backups {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Backups").finish_non_exhaustive()
    }
}

struct Inner {
    deps: BackupsDeps,
    /// The slot the log last warned had no location to back up to, so it warns once a night.
    warned_for_slot: Mutex<Option<DateTime<Utc>>>,
}

/// A backup about to run: its row, where it goes, and what it dumps.
struct Prepared {
    backup: DatabaseBackup,
    location: StorageLocation,
    databases: Vec<String>,
}

impl Backups {
    pub fn new(deps: BackupsDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                deps,
                warned_for_slot: Mutex::new(None),
            }),
        }
    }

    /// Settles what an earlier engine left running, then runs every backup someone asks for,
    /// and checks the schedule now and every [`CHECK_INTERVAL`], until shutdown. The engine
    /// runs this.
    pub fn start(&self) -> JoinHandle<()> {
        let backups = self.clone();
        let mut wake = self
            .inner
            .deps
            .events
            .wake_on(|signal| *signal == Signal::Backups);
        tokio::spawn(async move {
            if let Err(error) = backups.settle_after_restart().await {
                event!(
                    name: "backups.settle.failure",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "could not settle the backup an earlier start left running",
                );
            }

            let shutdown = backups.inner.deps.shutdown.clone();
            let mut interval = tokio::time::interval(CHECK_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    _ = interval.tick() => backups.check_schedule().await,
                    () = wake.woken() => {}
                }
                backups.run_requested().await;
            }
        })
    }

    /// Asks the engine for a backup and answers at once with its row, while the engine runs
    /// it. Its outcome is published as `backups.status` and read back with [`Backups::find`].
    ///
    /// # Errors
    /// [`BeginError::AlreadyRunning`] while another runs, [`BeginError::NoDestination`] when no
    /// location is chosen, and database errors.
    pub async fn begin(
        &self,
        trigger: DatabaseBackupTrigger,
        by: Uuid,
    ) -> Result<DatabaseBackup, BeginError> {
        let prepared = self.prepare(trigger, by, false).await?;
        self.inner.deps.events.signal(&Signal::Backups);
        Ok(prepared.backup)
    }

    /// Backs up and answers once the backup ended, with how it ended. A failed backup is an
    /// `Ok` row whose status is failed: it began, and its failure is recorded and reported.
    ///
    /// # Errors
    /// As [`Backups::begin`].
    pub async fn back_up(
        &self,
        trigger: DatabaseBackupTrigger,
        by: Uuid,
    ) -> Result<DatabaseBackup, BeginError> {
        let prepared = self.prepare(trigger, by, true).await?;
        Ok(self.perform(prepared).await)
    }

    /// Runs the backup someone asked for, if one waits for the engine. One at a time, so at
    /// most one waits.
    async fn run_requested(&self) {
        let claimed = async {
            let mut connection = self.connection().await?;
            let Some(backup) = database_backup::claim_requested(&mut connection).await? else {
                return Ok(None);
            };
            let location_id = backup
                .storage_location_id
                .context("the backup's storage location was deleted before it ran")?;
            let location = storage_location::find(&mut connection, location_id).await?;
            let databases = databases(&mut connection).await?;
            anyhow::Ok(Some(Prepared {
                backup,
                location,
                databases,
            }))
        };
        match claimed.await {
            Ok(Some(prepared)) => {
                event!(
                    name: "backups.run.taken",
                    Level::INFO,
                    backup.id = %prepared.backup.id,
                    backup.trigger = ?prepared.backup.trigger,
                    "backing up the databases, as asked",
                );
                self.perform(prepared).await;
            }
            Ok(None) => {}
            Err(error) => event!(
                name: "backups.run.take_failure",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "could not take the backup someone asked for; trying again shortly",
            ),
        }
    }

    /// One backup, by id.
    ///
    /// # Errors
    /// Propagates database errors.
    pub async fn find(&self, id: Uuid) -> Result<Option<DatabaseBackup>> {
        let mut connection = self.connection().await?;
        Ok(database_backup::find(&mut connection, id).await?)
    }

    /// Where backups stand.
    ///
    /// # Errors
    /// Propagates database errors.
    pub async fn status(&self) -> Result<BackupsStatus> {
        let now = Utc::now();
        let mut connection = self.connection().await?;
        let settings = user::workspace_settings(&mut connection).await?;
        let destination = match settings.backup_storage_location_id {
            Some(id) => {
                let location = storage_location::find(&mut connection, id).await?;
                Some(Destination {
                    id: location.id,
                    name: location.name,
                })
            }
            None => None,
        };
        let tonight = database_backup::since(&mut connection, schedule::latest_slot(now)).await?;
        let history = database_backup::recent(&mut connection, HISTORY_LENGTH).await?;
        let databases = databases(&mut connection).await?;
        drop(connection);

        let next_attempt_at = match schedule::decide(now, &tonight) {
            _ if destination.is_none() => None,
            Decision::Running => None,
            Decision::Due => Some(now),
            Decision::RetryAt(at) => Some(at),
            Decision::Done | Decision::GaveUp => Some(schedule::next_slot(now)),
        };
        Ok(BackupsStatus {
            destination,
            databases,
            retention_days: files::RETENTION_DAYS,
            next_attempt_at,
            history: history.into_iter().map(DatabaseBackupView::from).collect(),
        })
    }

    /// Publishes the status as `backups.status`. A status that cannot be read is logged.
    pub async fn publish_status(&self) {
        match self.status().await {
            Ok(status) => self
                .inner
                .deps
                .events
                .publish(&ServerEvent::BackupsStatus(Box::new(status))),
            Err(error) => event!(
                name: "backups.publish.failure",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "could not publish the backup status",
            ),
        }
    }

    /// Starts tonight's backup when it is due.
    async fn check_schedule(&self) {
        let now = Utc::now();
        let slot = schedule::latest_slot(now);
        let read = async {
            let mut connection = self.connection().await?;
            let settings = user::workspace_settings(&mut connection).await?;
            let tonight = database_backup::since(&mut connection, slot).await?;
            Ok::<_, anyhow::Error>((settings.backup_storage_location_id, tonight))
        };
        let (destination, tonight) = match read.await {
            Ok(read) => read,
            Err(error) => {
                event!(
                    name: "backups.schedule.failure",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "could not read whether tonight's backup is due",
                );
                return;
            }
        };
        if schedule::decide(now, &tonight) != Decision::Due {
            return;
        }
        if destination.is_none() {
            self.warn_no_destination(slot);
            return;
        }

        match self
            .back_up(DatabaseBackupTrigger::Schedule, SYSTEM_USER_ID)
            .await
        {
            // A failed backup has reported itself.
            Ok(_) => {}
            Err(BeginError::AlreadyRunning) => event!(
                name: "backups.schedule.already_running",
                Level::DEBUG,
                "tonight's backup waits for the one running",
            ),
            Err(BeginError::NoDestination) => self.warn_no_destination(slot),
            Err(BeginError::Internal(error)) => event!(
                name: "backups.schedule.failure",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "could not begin tonight's backup",
            ),
        }
    }

    /// Warns, once for `slot`, that nothing is backed up because no location is chosen.
    fn warn_no_destination(&self, slot: DateTime<Utc>) {
        let first_for_slot = self.warned_for_slot().replace(slot) != Some(slot);
        if first_for_slot {
            event!(
                name: "backups.schedule.no_destination",
                Level::WARN,
                "the databases are not backed up: no storage location is chosen in Settings, Backups",
            );
        }
    }

    /// Records a backup beginning: taken by this engine when `claimed`, or waiting for it.
    async fn prepare(
        &self,
        trigger: DatabaseBackupTrigger,
        by: Uuid,
        claimed: bool,
    ) -> Result<Prepared, BeginError> {
        let mut connection = self.connection().await?;
        let settings = user::workspace_settings(&mut connection).await?;
        let location_id = settings
            .backup_storage_location_id
            .ok_or(BeginError::NoDestination)?;
        let location = storage_location::find(&mut connection, location_id).await?;
        let databases = databases(&mut connection).await?;
        // To the second: the start names the backup's files, and the row must name them too.
        let started_at = Utc::now().trunc_subsecs(0);
        let backup = database_backup::begin(
            &mut connection,
            trigger,
            location.id,
            started_at,
            by,
            claimed,
        )
        .await
        .map_err(|error| match error {
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                BeginError::AlreadyRunning
            }
            other => other.into(),
        })?;
        drop(connection);

        event!(
            name: "backups.run.begin",
            Level::INFO,
            backup.id = %backup.id,
            backup.trigger = ?backup.trigger,
            storage.location.id = %location.id,
            "backing up the databases",
        );
        self.publish_status().await;
        Ok(Prepared {
            backup,
            location,
            databases,
        })
    }

    /// Runs a prepared backup to its end, records how it ended, and reports a failure. Answers
    /// the finished row.
    async fn perform(&self, prepared: Prepared) -> DatabaseBackup {
        let Prepared {
            backup,
            location,
            databases,
        } = prepared;
        let _activity = self.inner.deps.activity.begin(
            ActivityKind::DatabaseBackup,
            format!("backup:{}", backup.id),
        );

        let written = tokio::select! {
            written = self.write(&backup, &location, &databases) => written,
            () = self.inner.deps.shutdown.cancelled() => Err(anyhow!(INTERRUPTED)),
        };
        // A dump that failed part way, or one a shutdown cut short, is still staged.
        if let Err(error) = self.clear_staging().await {
            event!(
                name: "backups.staging.failure",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "could not clear the staged dumps; the next backup clears them",
            );
        }
        let outcome = match written {
            Ok(size_bytes) => BackupOutcome::Succeeded { size_bytes },
            Err(error) => {
                self.discard(&location, backup.started_at, &databases).await;
                BackupOutcome::Failed(format!("{error:#}"))
            }
        };

        let finished = match self.finish(&backup, outcome.clone()).await {
            Ok(finished) => finished,
            Err(error) => {
                event!(
                    name: "backups.record.failure",
                    Level::ERROR,
                    backup.id = %backup.id,
                    error.message = %format!("{error:#}"),
                    "could not record how a backup ended; the next start settles it",
                );
                backup.clone()
            }
        };
        match &outcome {
            BackupOutcome::Succeeded { size_bytes } => {
                event!(
                    name: "backups.run.success",
                    Level::INFO,
                    backup.id = %backup.id,
                    backup.size = size_bytes,
                    "backed up the databases",
                );
                self.prune(&location).await;
            }
            BackupOutcome::Failed(error) => {
                event!(
                    name: "backups.run.failure",
                    Level::ERROR,
                    backup.id = %backup.id,
                    backup.trigger = ?backup.trigger,
                    storage.location.id = %location.id,
                    error.message = %error,
                    "the databases could not be backed up",
                );
                self.report_failure(&finished, &location).await;
            }
        }
        self.publish_status().await;
        finished
    }

    /// Dumps and uploads every database, and answers the bytes stored. Each dump is staged on
    /// disk, since providers need its length before its first byte, and removed once uploaded.
    async fn write(
        &self,
        backup: &DatabaseBackup,
        location: &StorageLocation,
        databases: &[String],
    ) -> Result<i64> {
        let access_key = self.access_key(location)?;
        let staging = &self.inner.deps.staging_directory;
        // Whatever a process that stopped mid-backup left here is of no use now.
        self.clear_staging().await?;
        tokio::fs::create_dir_all(staging)
            .await
            .with_context(|| format!("could not create {}", staging.display()))?;

        let mut stored: u64 = 0;
        for database in databases {
            let path = files::dump_path(backup.started_at, database);
            let staged = staging.join(format!("{database}.dump"));
            let size = self
                .inner
                .deps
                .dumper
                .dump(database, &staged)
                .await
                .with_context(|| format!("could not dump {database}"))?;
            let uploaded = self
                .upload(location, &access_key, &path, &staged, size)
                .await;
            // The staged dump is removed whether or not it was stored.
            let removed = tokio::fs::remove_file(&staged).await;
            uploaded.with_context(|| format!("could not upload {path}"))?;
            removed.with_context(|| format!("could not remove {}", staged.display()))?;
            stored += size;
        }
        i64::try_from(stored).context("the backup's size does not fit a count of bytes")
    }

    /// Removes the staging directory with every dump in it.
    async fn clear_staging(&self) -> Result<()> {
        let staging = &self.inner.deps.staging_directory;
        match tokio::fs::remove_dir_all(staging).await {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(error).with_context(|| format!("could not clear {}", staging.display()))
            }
            _ => Ok(()),
        }
    }

    async fn upload(
        &self,
        location: &StorageLocation,
        access_key: &SecretString,
        path: &str,
        staged: &Path,
        size: u64,
    ) -> Result<()> {
        let file = tokio::fs::File::open(staged).await?;
        self.inner
            .deps
            .storage
            .upload(
                location,
                access_key,
                path,
                ReaderStream::new(file),
                size,
                Some(DUMP_CONTENT_TYPE),
            )
            .await?;
        Ok(())
    }

    /// Removes every file of the backup that started at `started_at`, so the location never
    /// holds part of one. Removing a file that was never uploaded succeeds. A file that cannot
    /// be removed is logged; retention removes it with its age.
    async fn discard(
        &self,
        location: &StorageLocation,
        started_at: DateTime<Utc>,
        databases: &[String],
    ) {
        let removed = async {
            let access_key = self.access_key(location)?;
            for database in databases {
                let path = files::dump_path(started_at, database);
                self.inner
                    .deps
                    .storage
                    .delete(location, &access_key, &path)
                    .await
                    .with_context(|| format!("could not remove {path}"))?;
            }
            Ok::<_, anyhow::Error>(())
        };
        if let Err(error) = removed.await {
            event!(
                name: "backups.discard.failure",
                Level::ERROR,
                storage.location.id = %location.id,
                error.message = %format!("{error:#}"),
                "could not remove a failed backup's files; retention removes them in time",
            );
        }
    }

    /// Removes every backup past retention from `location`. A failure is logged and changes
    /// nothing about the backup that just succeeded.
    async fn prune(&self, location: &StorageLocation) {
        let pruned = async {
            let access_key = self.access_key(location)?;
            let storage = &self.inner.deps.storage;
            let mut entries = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let page = storage
                    .list(location, &access_key, files::DIRECTORY, cursor.as_deref())
                    .await
                    .context("could not list the backups")?;
                entries.extend(page.entries);
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }

            let expired = files::expired(&entries, Utc::now());
            for path in &expired {
                storage
                    .delete(location, &access_key, path)
                    .await
                    .with_context(|| format!("could not remove {path}"))?;
            }
            Ok::<_, anyhow::Error>(expired.len())
        };
        match pruned.await {
            Ok(removed) => event!(
                name: "backups.prune.success",
                Level::INFO,
                storage.location.id = %location.id,
                backup.files.removed = removed,
                "removed backups past retention",
            ),
            Err(error) => event!(
                name: "backups.prune.failure",
                Level::ERROR,
                storage.location.id = %location.id,
                error.message = %format!("{error:#}"),
                "could not remove backups past retention; the next backup tries again",
            ),
        }
    }

    /// Raises an inbox item when `failed` was the night's last scheduled attempt. Earlier
    /// attempts are retried, and a backup someone asked for was seen failing by whoever asked.
    async fn report_failure(&self, failed: &DatabaseBackup, location: &StorageLocation) {
        if failed.trigger != DatabaseBackupTrigger::Schedule {
            return;
        }
        let raised = async {
            let mut connection = self.connection().await?;
            let slot = schedule::latest_slot(failed.started_at);
            let tonight = database_backup::since(&mut connection, slot).await?;
            if schedule::decide(Utc::now(), &tonight) != Decision::GaveUp {
                return Ok(false);
            }

            let now = Utc::now();
            let item = failure_item(failed, location, tonight.len());
            let created = action_item::create(&mut connection, item, Actor::System, now)
                .await
                .map_err(|error| anyhow!("cannot raise an inbox item: {error}"))?;
            publish_item_write(&self.inner.deps.events, &mut connection, created, &[], now)
                .await
                .map_err(|error| anyhow!("cannot publish the raised item: {error}"))?;
            Ok::<_, anyhow::Error>(true)
        };
        match raised.await {
            Ok(true) => event!(
                name: "backups.report.raised",
                Level::INFO,
                backup.id = %failed.id,
                "raised an inbox item for tonight's failed backup",
            ),
            Ok(false) => {}
            Err(error) => event!(
                name: "backups.report.failure",
                Level::ERROR,
                backup.id = %failed.id,
                error.message = %format!("{error:#}"),
                "could not raise an inbox item for tonight's failed backup",
            ),
        }
    }

    /// Fails every backup an earlier engine left running, and removes what it uploaded.
    async fn settle_after_restart(&self) -> Result<()> {
        let mut connection = self.connection().await?;
        let interrupted = database_backup::fail_running(&mut connection, INTERRUPTED).await?;
        if interrupted.is_empty() {
            return Ok(());
        }
        let databases = databases(&mut connection).await?;
        for backup in &interrupted {
            event!(
                name: "backups.run.interrupted",
                Level::ERROR,
                backup.id = %backup.id,
                "a backup was cut short when the API stopped",
            );
            let Some(location_id) = backup.storage_location_id else {
                continue;
            };
            match storage_location::find(&mut connection, location_id).await {
                Ok(location) => {
                    self.discard(&location, backup.started_at, &databases).await;
                }
                Err(DieselError::NotFound) => {}
                Err(error) => return Err(error.into()),
            }
        }
        drop(connection);
        self.publish_status().await;
        Ok(())
    }

    async fn finish(
        &self,
        backup: &DatabaseBackup,
        outcome: BackupOutcome,
    ) -> Result<DatabaseBackup> {
        let mut connection = self.connection().await?;
        let finished = database_backup::finish(&mut connection, backup.id, outcome)
            .await?
            .context("the backup was no longer running")?;
        Ok(finished)
    }

    fn access_key(&self, location: &StorageLocation) -> Result<SecretString> {
        location
            .access_key(&self.inner.deps.cipher)
            .context("the location's access key cannot be decrypted")
    }

    async fn connection(&self) -> Result<Object<AsyncPgConnection>> {
        self.inner
            .deps
            .database
            .get()
            .await
            .context("no database connection available")
    }

    /// The lock is never held across an await. The poison is ignored: it guards a plain value.
    fn warned_for_slot(&self) -> MutexGuard<'_, Option<DateTime<Utc>>> {
        self.inner
            .warned_for_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Every database the stack keeps: Elysium's own, whatever `DATABASE_URL` names it, then
/// Kratos's and Hydra's.
async fn databases(connection: &mut AsyncPgConnection) -> Result<Vec<String>> {
    let own: String = diesel::select(current_database())
        .get_result(connection)
        .await
        .context("could not read the database's name")?;
    let mut databases = vec![own];
    databases.extend(IDENTITY_DATABASES.map(str::to_owned));
    Ok(databases)
}

/// The inbox item a night whose every scheduled attempt failed raises.
fn failure_item(
    failed: &DatabaseBackup,
    location: &StorageLocation,
    attempts: usize,
) -> NewActionItem {
    let error = failed.error.as_deref().unwrap_or_default();
    NewActionItem {
        title: "Last night's database backup failed".to_owned(),
        notes: format!(
            "Elysium tried {attempts} times to back up its databases to {location}, and every \
             attempt failed. Nothing was backed up tonight; the next attempt is tomorrow night.\n\n\
             The last attempt failed with: {error}\n\n\
             Settings, Backups lists every attempt, and Back up now tries again at once.",
            location = location.name,
        ),
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::High,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
