// Copyright © 2026 Jalapeno Labs

//! Browsers the host runs and streams to people, for signing in to their accounts as
//! themselves. See `docs/browser.md`.
//!
//! Each browser is a row in `browser_sessions` and one Neko container ([`container`]), with
//! a media port from [`PORTS`] that nginx publishes. [`Browsers`] creates them, removes
//! them, and keeps them in place: a reconciler checks every browser every
//! [`RECONCILE_INTERVAL`], which also rejoins nginx to the browsers' networks after compose
//! recreates it. Viewers watch through the stream route, which [`relay`]s Neko's signaling
//! over a member session the API signs in to ([`neko`]).
//!
//! What each browser is doing is kept in memory, published as `browserSession.upserted`
//! whenever it changes, and starts over as `starting` when the API restarts: the reconciler
//! learns the truth within one pass.

pub mod address;
pub mod container;
pub mod neko;
pub mod relay;

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL;
use secrecy::SecretString;
use serde::Serialize;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use url::Url;
use uuid::Uuid;

use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::browser_session::{self, BrowserSession, NewBrowserSession};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::browser_sessions::BrowserSessionResponse;
use container::{BrowserContainers, BrowserSpec};
use neko::{Neko, Upstream};

/// The media ports nginx publishes, one browser on each. The same range is in
/// `compose.yml`, `nginx/browser.conf`, and the `browser_sessions.port` check; the four
/// change together.
pub const PORTS: RangeInclusive<u16> = 59000..=59007;

/// How often every browser is checked. Short, because compose drops nginx from the
/// browsers' networks whenever it recreates nginx, and a browser's media is refused until
/// the next pass joins it back. A pass over every browser is a few Docker inspections.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(10);

const LOCK_POISONED: &str = "browser status lock poisoned by an earlier panic";

/// What a browser is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserState {
    /// Its image is being pulled or its container started, or the API has not checked it
    /// since it started.
    Starting,
    Running,
    /// The last attempt to start or check it failed; the reconciler tries again.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatus {
    pub state: BrowserState,
    /// Why it failed, when it did.
    pub error: Option<String>,
}

impl BrowserStatus {
    const STARTING: Self = Self {
        state: BrowserState::Starting,
        error: None,
    };
    const RUNNING: Self = Self {
        state: BrowserState::Running,
        error: None,
    };
}

/// What came of bringing a browser up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BringUp {
    Running,
    /// Its row was removed while the pass waited its turn, so nothing was created.
    Removed,
}

/// Why a browser could not be added.
#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error("all {count} browsers this host streams are in use", count = PORTS.len())]
    PortsExhausted,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[derive(Clone)]
pub struct Browsers {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
    containers: BrowserContainers,
    neko: Neko,
    public_url: Url,
    statuses: Mutex<HashMap<Uuid, BrowserStatus>>,
    /// Held across every change to Docker, so a browser being created, checked, and removed
    /// at once never races itself. Eight browsers make this cheap.
    docker: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for Browsers {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Browsers")
            .field("public_url", &self.inner.public_url.as_str())
            .finish_non_exhaustive()
    }
}

impl Browsers {
    pub fn new(
        database: Pool,
        cipher: Arc<Cipher>,
        events: EventBus,
        containers: BrowserContainers,
        neko: Neko,
        public_url: Url,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                database,
                cipher,
                events,
                containers,
                neko,
                public_url,
                statuses: Mutex::new(HashMap::new()),
                docker: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// Starts the reconciler, which runs one pass at once and then one every
    /// [`RECONCILE_INTERVAL`] until `shutdown` is cancelled.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let browsers = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(RECONCILE_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    _tick = interval.tick() => browsers.reconcile().await,
                }
            }
        })
    }

    /// What the browser `id` is doing.
    pub fn status(&self, id: Uuid) -> BrowserStatus {
        self.inner
            .statuses
            .lock()
            .expect(LOCK_POISONED)
            .get(&id)
            .cloned()
            .unwrap_or(BrowserStatus::STARTING)
    }

    /// Stores a new browser on the lowest free port and starts it in the background, then
    /// opens `start_url` in it once Chromium answers.
    ///
    /// # Errors
    /// [`CreateError::PortsExhausted`] when every port is taken, or a database error.
    pub async fn create(
        &self,
        owner: Uuid,
        name: String,
        start_url: Url,
    ) -> Result<BrowserSession, CreateError> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let in_use = browser_session::ports_in_use(&mut connection)
            .await
            .context("cannot read which ports browsers hold")?;
        let port = free_port(&in_use).ok_or(CreateError::PortsExhausted)?;

        let session = browser_session::create(
            &mut connection,
            &self.inner.cipher,
            NewBrowserSession {
                id: Uuid::now_v7(),
                name,
                start_url: start_url.to_string(),
                port: i32::from(port),
                member_password: generate_password(),
                created_by: owner,
            },
        )
        .await
        // Two browsers added at the same instant can pick the same port. The table's unique
        // port refuses the second, which can simply be added again.
        .context("cannot store the browser")?;
        drop(connection);

        self.set_status(&session, BrowserStatus::STARTING);
        let browsers = self.clone();
        let created = session.clone();
        tokio::spawn(async move {
            browsers.bring_up(&created, Some(&start_url)).await;
        });
        Ok(session)
    }

    /// Removes the browser's container, network, and profile, then its row.
    ///
    /// The removal runs as its own task, so a viewer who gives up on the request does not
    /// stop it halfway; awaiting this only waits for its outcome.
    ///
    /// # Errors
    /// Fails when Docker refuses, leaving the row so removal can be tried again.
    pub async fn delete(&self, session: &BrowserSession) -> anyhow::Result<()> {
        let browsers = self.clone();
        let session = session.clone();
        tokio::spawn(async move { browsers.remove(&session).await })
            .await
            .context("the browser's removal ended abnormally")?
    }

    async fn remove(&self, session: &BrowserSession) -> anyhow::Result<()> {
        let docker = self.inner.docker.lock().await;
        self.inner
            .containers
            .remove(session.id)
            .await
            .context("Docker refused to remove the browser")?;
        drop(docker);

        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        browser_session::delete(&mut connection, session.id).await?;
        self.inner
            .statuses
            .lock()
            .expect(LOCK_POISONED)
            .remove(&session.id);
        self.inner
            .events
            .publish(&ServerEvent::BrowserSessionDeleted { id: session.id });
        Ok(())
    }

    /// Signs in to the browser as its member and opens its signaling socket, for a viewer.
    ///
    /// # Errors
    /// Fails when the stored password does not open, or the browser does not answer.
    pub async fn connect(&self, session: &BrowserSession) -> anyhow::Result<Upstream> {
        let password = session
            .member_password(&self.inner.cipher)
            .context("the browser's stored password does not open with this encryption key")?;
        let upstream = self.inner.neko.connect(port_of(session), &password).await?;
        Ok(upstream)
    }

    /// One pass over every browser.
    async fn reconcile(&self) {
        let sessions = match self.list_all().await {
            Ok(sessions) => sessions,
            Err(error) => {
                event!(
                    name: "browser.reconcile.failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "cannot list the browsers to check them",
                );
                return;
            }
        };
        for session in &sessions {
            self.bring_up(session, None).await;
        }
    }

    async fn list_all(&self) -> anyhow::Result<Vec<BrowserSession>> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let sessions = browser_session::list_all(&mut connection).await?;
        Ok(sessions)
    }

    /// Makes the browser's container match its row and run, then opens `start_url` in it
    /// when one is given. Records what came of it.
    async fn bring_up(&self, session: &BrowserSession, start_url: Option<&Url>) {
        let outcome = self.try_bring_up(session, start_url).await;
        match outcome {
            Ok(BringUp::Running) => self.set_status(session, BrowserStatus::RUNNING),
            Ok(BringUp::Removed) => {
                event!(
                    name: "browser.start.skipped",
                    Level::DEBUG,
                    browser.session.id = %session.id,
                    "skipped bringing up a browser that was removed meanwhile",
                );
            }
            Err(error) => {
                let message = format!("{error:#}");
                event!(
                    name: "browser.start.failed",
                    Level::WARN,
                    browser.session.id = %session.id,
                    error.message = %message,
                    "a browser failed to start",
                );
                self.set_status(
                    session,
                    BrowserStatus {
                        state: BrowserState::Failed,
                        error: Some(message),
                    },
                );
            }
        }
    }

    async fn try_bring_up(
        &self,
        session: &BrowserSession,
        start_url: Option<&Url>,
    ) -> anyhow::Result<BringUp> {
        let member_password = session
            .member_password(&self.inner.cipher)
            .context("the browser's stored password does not open with this encryption key")?;
        let public_address = address::public_address(&self.inner.public_url).await?;
        let spec = BrowserSpec {
            session_id: session.id,
            port: port_of(session),
            public_address,
            member_password,
        };

        let docker = self.inner.docker.lock().await;
        // A removal may have run while this waited for the lock; bringing the browser back
        // then would leave a container no row owns.
        if !self.still_stored(session.id).await? {
            return Ok(BringUp::Removed);
        }
        self.inner.containers.ensure_image().await?;
        self.inner.containers.ensure(&spec).await?;
        drop(docker);

        if let Some(start_url) = start_url {
            self.inner
                .neko
                .open_start_page(spec.port, start_url)
                .await?;
        }
        Ok(BringUp::Running)
    }

    async fn still_stored(&self, id: Uuid) -> anyhow::Result<bool> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let stored = browser_session::exists(&mut connection, id).await?;
        Ok(stored)
    }

    /// Records the browser's status and publishes it when it changed.
    fn set_status(&self, session: &BrowserSession, status: BrowserStatus) {
        let previous = self
            .inner
            .statuses
            .lock()
            .expect(LOCK_POISONED)
            .insert(session.id, status.clone());
        if previous.as_ref() == Some(&status) {
            return;
        }
        self.inner
            .events
            .publish(&ServerEvent::BrowserSessionUpserted(
                BrowserSessionResponse::new(session.clone(), status),
            ));
    }
}

/// The browser's media port. The table's check keeps it inside [`PORTS`].
fn port_of(session: &BrowserSession) -> u16 {
    u16::try_from(session.port).expect("browser_sessions.port is checked to be a media port")
}

/// The lowest port in [`PORTS`] no browser holds.
fn free_port(in_use: &[i32]) -> Option<u16> {
    PORTS
        .clone()
        .find(|port| !in_use.contains(&i32::from(*port)))
}

/// A fresh 256-bit password in unpadded base64url.
///
/// # Panics
/// Panics if the operating system cannot supply random bytes.
pub fn generate_password() -> SecretString {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random number generator is available");
    SecretString::from(BASE64_URL.encode(bytes))
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    #[test]
    fn browsers_take_the_lowest_free_port_until_none_is_left() {
        assert_eq!(free_port(&[]), Some(59000));
        assert_eq!(free_port(&[59000, 59002]), Some(59001));
        let every_port: Vec<i32> = PORTS.map(i32::from).collect();
        assert_eq!(free_port(&every_port), None);
    }

    #[test]
    fn passwords_are_long_and_never_repeat() {
        let first = generate_password();
        let second = generate_password();
        assert_eq!(first.expose_secret().len(), 43);
        assert_ne!(first.expose_secret(), second.expose_secret());
    }
}
