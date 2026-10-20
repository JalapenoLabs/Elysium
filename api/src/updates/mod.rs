// Copyright © 2026 Jalapeno Labs

//! Auto-update: the API decides when the deployment moves to a newer release, and the
//! updater carries it out.
//!
//! The updater (`updater/updater.sh`, a compose service in the `auto-update` profile) holds
//! the Docker socket and can restart the stack, but decides nothing. Every minute it asks
//! the API what to do, over `GET /internal/updater/instruction`, and is told either to wait
//! or to apply one release. It reports how that went over `POST /internal/updater/outcome`.
//! Both routes check a key derived from the encryption key, like Kratos's hooks.
//!
//! The API checks Docker Hub for releases every hour ([`CHECK_INTERVAL`]) and tells the
//! updater to apply the newest one when all of these hold:
//!
//! - this build is a release (`/api/version` names it), not `main` or a development build;
//! - the newest release is newer, and has not rolled back here before, unless an admin
//!   asks for it from Settings;
//! - no update is applying;
//! - nothing is running that a restart would cut short (see [`crate::activity`]).
//!
//! Each update is recorded in `deployment_updates`, and every change is published as
//! `updates.status` for the Settings page. The rules are the pure [`decision::decide`].

pub mod decision;
pub mod registry;

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, TimeDelta, Utc};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use secrecy::{ExposeSecret, SecretString};
use semver::Version;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use crate::activity::{Activities, Activity};
use crate::database::Pool;
use crate::models::deployment_update::{
    self, DeploymentUpdate, DeploymentUpdateStatus, UpdateOutcome,
};
use crate::models::user::SYSTEM_USER_ID;
use crate::realtime::{EventBus, ServerEvent};
use crate::version::VersionInfo;
use decision::{Decision, Situation, UpdateState, decide};
use registry::Registry;

/// How often Docker Hub is checked for a newer release. Releases are cut once a day, so an
/// hour lands each one the same morning without asking Docker Hub more than it needs.
const CHECK_INTERVAL: Duration = Duration::from_hours(1);

/// How long an update may stay applying before it counts as failed. An update pulls images,
/// backs the databases up, and restarts the stack, which takes minutes; an updater that has
/// not reported by this time was interrupted, and the deployment must not wait on it forever.
const APPLY_TIMEOUT: TimeDelta = TimeDelta::minutes(45);

/// How recently the updater must have asked for an instruction to count as connected. It
/// asks every minute, so three missed polls mean it is stopped or cannot reach the API.
const UPDATER_CONNECTED_WITHIN: TimeDelta = TimeDelta::minutes(3);

/// How many past updates Settings lists.
const HISTORY_LENGTH: i64 = 20;

/// The header the updater sends its key in.
pub const UPDATER_KEY_HEADER: &str = "x-elysium-updater-key";

/// The key the updater sends, derived from the encryption key as the hex SHA-256 of
/// `elysium/updater/key:<key>`. `updater/updater.sh` derives the same value, and both pin
/// the test vector below.
pub fn derive_updater_key(encryption_key: &SecretString) -> SecretString {
    let digest = Sha256::digest(format!(
        "elysium/updater/key:{}",
        encryption_key.expose_secret()
    ));
    SecretString::from(hex::encode(digest))
}

/// What the updater is told to do, written as one line of text it reads with `read`:
/// `wait`, or `apply <target> from <running>`. The running release is what it restores if the
/// target does not come up healthy; the deployment's own configuration may name a floating
/// tag such as `latest`, which would move with the update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Wait,
    Apply { target: Version, running: Version },
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wait => f.write_str("wait"),
            Self::Apply { target, running } => write!(f, "apply {target} from {running}"),
        }
    }
}

/// How an update ended, as the updater reports it: `succeeded <version>` or
/// `rolled-back <version> <reason>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub version: Version,
    pub outcome: UpdateOutcome,
}

/// The longest reason kept, matching the column's limit.
const MAX_REASON_CHARACTERS: usize = 2000;

impl Report {
    /// Reads the updater's one-line report.
    ///
    /// # Errors
    /// Returns a message naming what is wrong when the line is not a report.
    pub fn parse(line: &str) -> Result<Self, String> {
        let mut words = line.trim().splitn(3, ' ');
        let outcome = words.next().unwrap_or_default();
        let version = words
            .next()
            .ok_or_else(|| "the report names no version".to_owned())?;
        let version = Version::parse(version)
            .map_err(|error| format!("`{version}` is not a release: {error}"))?;

        let outcome = match outcome {
            "succeeded" => UpdateOutcome::Succeeded,
            "rolled-back" => {
                let reason: String = words
                    .next()
                    .map(str::trim)
                    .unwrap_or_default()
                    .chars()
                    .take(MAX_REASON_CHARACTERS)
                    .collect();
                if reason.is_empty() {
                    return Err("a rollback must say why".to_owned());
                }
                UpdateOutcome::RolledBack(reason)
            }
            other => return Err(format!("`{other}` is not an outcome")),
        };
        Ok(Self { version, outcome })
    }
}

/// Why an admin's request to update cannot be taken.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("this build is not a release, so it does not update itself")]
    NotARelease,
    #[error("no newer release is known; check for one first")]
    NothingNewer,
}

/// Where the deployment stands, for Settings and the `updates.status` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesStatus {
    /// The release running, or `None` for a build that is not a release.
    pub release: Option<String>,
    pub commit: Option<String>,
    pub built_at: &'static str,
    pub state: UpdateState,
    /// The release that is ready or waiting for running work.
    pub target: Option<String>,
    pub newest_release: Option<String>,
    pub last_checked_at: Option<DateTime<Utc>>,
    /// Why the last check failed, when it did.
    pub last_check_error: Option<String>,
    /// Whether the updater asked for an instruction recently. Without it, nothing updates.
    pub updater_connected: bool,
    pub updater_seen_at: Option<DateTime<Utc>>,
    /// What an update is waiting for.
    pub waiting_for: Vec<Activity>,
    /// Newest first.
    pub history: Vec<DeploymentUpdateView>,
}

/// One past or applying update.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentUpdateView {
    pub id: Uuid,
    pub from_version: String,
    pub to_version: String,
    pub status: DeploymentUpdateStatus,
    pub reason: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_by: Uuid,
}

impl From<DeploymentUpdate> for DeploymentUpdateView {
    fn from(update: DeploymentUpdate) -> Self {
        Self {
            id: update.id,
            from_version: update.from_version,
            to_version: update.to_version,
            status: update.status,
            reason: update.reason,
            started_at: update.started_at,
            finished_at: update.finished_at,
            created_by: update.created_by,
        }
    }
}

/// What [`Updates`] needs.
#[derive(Clone)]
pub struct UpdatesDeps {
    pub database: Pool,
    pub events: EventBus,
    pub activity: Activities,
    pub http: reqwest::Client,
    pub version: Arc<VersionInfo>,
    pub encryption_key: SecretString,
}

/// Auto-update's state and decisions. Cloning is cheap: every clone shares one state.
#[derive(Clone)]
pub struct Updates {
    inner: Arc<Inner>,
}

impl fmt::Debug for Updates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Updates")
            .field("running", &self.inner.running)
            .finish_non_exhaustive()
    }
}

struct Inner {
    database: Pool,
    events: EventBus,
    activity: Activities,
    registry: Registry,
    version: Arc<VersionInfo>,
    /// The release this build is, parsed once.
    running: Option<Version>,
    updater_key: SecretString,
    observed: Mutex<Observed>,
}

/// What the API has learned since it started.
#[derive(Debug, Default)]
struct Observed {
    newest: Option<Version>,
    last_checked_at: Option<DateTime<Utc>>,
    last_check_error: Option<String>,
    updater_seen_at: Option<DateTime<Utc>>,
    requested: Option<UpdateRequest>,
    /// What the last instruction poll published, so a poll publishes only a change.
    published: Option<PublishedKey>,
}

#[derive(Debug, Clone)]
struct UpdateRequest {
    version: Version,
    by: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishedKey {
    decision: Decision,
    updater_connected: bool,
    waiting_for: usize,
}

/// What the database says, read together for one decision.
struct Recorded {
    applying: Option<DeploymentUpdate>,
    rolled_back: Vec<Version>,
}

impl Updates {
    pub fn new(deps: UpdatesDeps) -> Self {
        let running = deps.version.release.and_then(parse_release);
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                activity: deps.activity,
                registry: Registry::new(deps.http),
                version: deps.version,
                running,
                updater_key: derive_updater_key(&deps.encryption_key),
                observed: Mutex::default(),
            }),
        }
    }

    pub fn updater_key(&self) -> &SecretString {
        &self.inner.updater_key
    }

    /// Settles what an earlier process left applying, then checks Docker Hub now and every
    /// [`CHECK_INTERVAL`] until `shutdown` is cancelled.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let updates = self.clone();
        tokio::spawn(async move {
            if let Err(error) = updates.settle_after_restart().await {
                event!(
                    name: "updates.settle.failure",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "could not settle the update applying before this start",
                );
            }

            let mut interval = tokio::time::interval(CHECK_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    _ = interval.tick() => updates.check().await,
                }
            }
        })
    }

    /// Asks Docker Hub for the newest release, records the answer, and publishes the status.
    pub async fn check(&self) {
        let result = self.inner.registry.newest_release().await;
        {
            let mut observed = self.observed();
            observed.last_checked_at = Some(Utc::now());
            match &result {
                Ok(newest) => {
                    observed.newest.clone_from(newest);
                    observed.last_check_error = None;
                }
                Err(error) => observed.last_check_error = Some(error.to_string()),
            }
        }

        match result {
            Ok(newest) => event!(
                name: "updates.check.success",
                Level::INFO,
                service.version.newest = newest.as_ref().map(ToString::to_string).unwrap_or_default(),
                "checked Docker Hub for a newer release",
            ),
            Err(error) => event!(
                name: "updates.check.failure",
                Level::WARN,
                error.message = %error,
                "could not check Docker Hub for a newer release",
            ),
        }
        self.publish_status().await;
    }

    /// What the updater should do now. Records that it asked, and begins the update when the
    /// answer is to apply one.
    ///
    /// # Errors
    /// Propagates database errors.
    pub async fn instruction(&self) -> Result<Instruction> {
        self.observed().updater_seen_at = Some(Utc::now());

        let recorded = self.recorded().await?;
        let decision = self.decide(&recorded);
        let key = self.published_key(&decision);

        let instruction = match (&decision.state, &decision.target) {
            (UpdateState::Ready, Some(target)) => self.begin(target).await?,
            _ => Instruction::Wait,
        };

        let changed = {
            let mut observed = self.observed();
            let changed = observed.published.as_ref() != Some(&key);
            observed.published = Some(key);
            changed
        };
        if changed || instruction != Instruction::Wait {
            self.publish_status().await;
        }
        Ok(instruction)
    }

    /// Records how an update ended.
    ///
    /// # Errors
    /// Propagates database errors.
    pub async fn report(&self, report: Report) -> Result<()> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let version = report.version.to_string();
        let finished = deployment_update::finish(&mut connection, &version, report.outcome).await?;
        drop(connection);

        let Some(finished) = finished else {
            event!(
                name: "updates.report.unmatched",
                Level::WARN,
                service.version.target = version,
                "the updater reported on an update that is not applying, so nothing changed",
            );
            return Ok(());
        };

        {
            let mut observed = self.observed();
            if observed
                .requested
                .as_ref()
                .is_some_and(|request| request.version == report.version)
            {
                observed.requested = None;
            }
        }
        event!(
            name: "updates.apply.finished",
            Level::INFO,
            service.version.previous = finished.from_version,
            service.version.target = finished.to_version,
            update.status = ?finished.status,
            update.reason = finished.reason.as_deref().unwrap_or_default(),
            "an update finished",
        );
        self.publish_status().await;
        Ok(())
    }

    /// Asks for the newest release on behalf of `by`, even one that rolled back before. It
    /// applies when nothing is running.
    ///
    /// # Errors
    /// Fails when this build is not a release or no newer release is known.
    pub async fn request(&self, by: Uuid) -> Result<(), RequestError> {
        self.record_request(by)?;
        self.publish_status().await;
        Ok(())
    }

    /// Synchronous, so the lock is never part of an async function's state.
    fn record_request(&self, by: Uuid) -> Result<(), RequestError> {
        let running = self
            .inner
            .running
            .as_ref()
            .ok_or(RequestError::NotARelease)?;
        let mut observed = self.observed();
        let newest = observed
            .newest
            .clone()
            .filter(|newest| newest > running)
            .ok_or(RequestError::NothingNewer)?;
        observed.requested = Some(UpdateRequest {
            version: newest,
            by,
        });
        Ok(())
    }

    /// Where the deployment stands.
    ///
    /// # Errors
    /// Propagates database errors.
    pub async fn status(&self) -> Result<UpdatesStatus> {
        let recorded = self.recorded().await?;
        let decision = self.decide(&recorded);

        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let history = deployment_update::recent(&mut connection, HISTORY_LENGTH).await?;
        drop(connection);

        let observed = self.observed();
        let version = &self.inner.version;
        Ok(UpdatesStatus {
            release: self.inner.running.as_ref().map(ToString::to_string),
            commit: version
                .git
                .commit
                .as_ref()
                .map(|commit| commit.short_hash.clone()),
            built_at: version.built_at,
            state: decision.state,
            target: decision.target.as_ref().map(ToString::to_string),
            newest_release: observed.newest.as_ref().map(ToString::to_string),
            last_checked_at: observed.last_checked_at,
            last_check_error: observed.last_check_error.clone(),
            updater_connected: is_connected(observed.updater_seen_at),
            updater_seen_at: observed.updater_seen_at,
            waiting_for: self.inner.activity.snapshot(),
            history: history
                .into_iter()
                .map(DeploymentUpdateView::from)
                .collect(),
        })
    }

    /// Records the update to `target` and tells the updater to apply it. Another update
    /// beginning first is not an error: the updater is told to wait.
    async fn begin(&self, target: &Version) -> Result<Instruction> {
        let running = self
            .inner
            .running
            .as_ref()
            .expect("a decision to apply is only made for a release");
        let requested_by = self
            .observed()
            .requested
            .as_ref()
            .filter(|request| &request.version == target)
            .map(|request| request.by);

        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let began = deployment_update::begin(
            &mut connection,
            &running.to_string(),
            &target.to_string(),
            requested_by.unwrap_or(SYSTEM_USER_ID),
        )
        .await;

        match began {
            Ok(update) => {
                event!(
                    name: "updates.apply.begin",
                    Level::INFO,
                    service.version.previous = update.from_version,
                    service.version.target = update.to_version,
                    "told the updater to apply a release",
                );
                Ok(Instruction::Apply {
                    target: target.clone(),
                    running: running.clone(),
                })
            }
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                event!(
                    name: "updates.apply.already_applying",
                    Level::INFO,
                    "another update began first, so the updater waits",
                );
                Ok(Instruction::Wait)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Reads what the decision needs from the database. An update applying past
    /// [`APPLY_TIMEOUT`] is closed as rolled back first, so an interrupted updater cannot
    /// hold the deployment forever.
    async fn recorded(&self) -> Result<Recorded> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;

        let mut applying = deployment_update::applying(&mut connection).await?;
        if let Some(update) =
            applying.take_if(|update| Utc::now() - update.started_at > APPLY_TIMEOUT)
        {
            let reason = format!(
                "The updater did not report an outcome within {} minutes.",
                APPLY_TIMEOUT.num_minutes()
            );
            deployment_update::finish(
                &mut connection,
                &update.to_version,
                UpdateOutcome::RolledBack(reason),
            )
            .await?;
            event!(
                name: "updates.apply.timed_out",
                Level::WARN,
                service.version.target = update.to_version,
                "an update never reported back, so it counts as rolled back",
            );
        }

        let rolled_back = deployment_update::rolled_back_versions(&mut connection)
            .await?
            .iter()
            .filter_map(|version| parse_release(version))
            .collect();
        Ok(Recorded {
            applying,
            rolled_back,
        })
    }

    fn decide(&self, recorded: &Recorded) -> Decision {
        let observed = self.observed();
        let requested = observed.requested.as_ref().map(|request| &request.version);
        decide(Situation {
            running: self.inner.running.as_ref(),
            newest: observed.newest.as_ref(),
            requested,
            rolled_back: &recorded.rolled_back,
            applying: recorded.applying.is_some(),
            idle: self.inner.activity.is_idle(),
        })
    }

    fn published_key(&self, decision: &Decision) -> PublishedKey {
        let observed = self.observed();
        PublishedKey {
            decision: decision.clone(),
            updater_connected: is_connected(observed.updater_seen_at),
            waiting_for: self.inner.activity.snapshot().len(),
        }
    }

    /// A release that started while an update to it was applying has succeeded, whatever
    /// became of the updater's report.
    async fn settle_after_restart(&self) -> Result<()> {
        let Some(running) = &self.inner.running else {
            return Ok(());
        };
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let Some(applying) = deployment_update::applying(&mut connection).await? else {
            return Ok(());
        };
        if applying.to_version == running.to_string() {
            deployment_update::finish(
                &mut connection,
                &applying.to_version,
                UpdateOutcome::Succeeded,
            )
            .await?;
            event!(
                name: "updates.apply.settled",
                Level::INFO,
                service.version.target = applying.to_version,
                "started as the release an update was applying, so it succeeded",
            );
        }
        Ok(())
    }

    async fn publish_status(&self) {
        match self.status().await {
            Ok(status) => self
                .inner
                .events
                .publish(&ServerEvent::UpdatesStatus(Box::new(status))),
            Err(error) => event!(
                name: "updates.publish.failure",
                Level::ERROR,
                error.message = %format!("{error:#}"),
                "could not publish the update status",
            ),
        }
    }

    /// The lock is never held across an await. The poison is ignored: every field is a plain
    /// value each operation leaves whole.
    fn observed(&self) -> MutexGuard<'_, Observed> {
        self.inner
            .observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(test)]
    pub fn set_newest(&self, newest: Option<Version>) {
        self.observed().newest = newest;
    }

    /// Deps sharing this instance's database, bus, and activity, as a restarted process
    /// would against the same stack.
    #[cfg(test)]
    pub fn deps_for_test(&self) -> UpdatesDeps {
        UpdatesDeps {
            database: self.inner.database.clone(),
            events: self.inner.events.clone(),
            activity: self.inner.activity.clone(),
            http: reqwest::Client::new(),
            version: Arc::clone(&self.inner.version),
            encryption_key: SecretString::from("testkey"),
        }
    }
}

/// A stable release, or `None` for anything else such as `main`.
fn parse_release(text: &str) -> Option<Version> {
    Version::parse(text)
        .ok()
        .filter(|version| version.pre.is_empty() && version.build.is_empty())
}

fn is_connected(seen_at: Option<DateTime<Utc>>) -> bool {
    seen_at.is_some_and(|seen_at| Utc::now() - seen_at <= UPDATER_CONNECTED_WITHIN)
}

#[cfg(test)]
mod tests;
