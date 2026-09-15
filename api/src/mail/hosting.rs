// Copyright © 2026 Jalapeno Labs

//! Creates the mail server Elysium hosts, and keeps it running.
//!
//! # Creating the server
//!
//! [`Hosting::start`] is called with the server's hostname and its first domain, and
//! returns at once; the work runs in the background because pulling Stalwart's image
//! can take a minute. Each step is published as `mailServer.updated`:
//!
//! 1. `preparing`: create the mail network and attach the API to it.
//! 2. `pulling-image`: pull Stalwart's pinned image unless Docker has it.
//! 3. `starting`: create the container and its volumes, and start it. It comes up in
//!    bootstrap mode and prints a temporary administrator to its log.
//! 4. `configuring`: read that password from the log, finish setup with it, and seal
//!    the permanent administrator Stalwart issues into `mail_servers`. Stalwart shows
//!    that password only once, so it is stored before anything else can fail.
//! 5. `restarting`: restart the container out of bootstrap mode and wait for it to
//!    accept the new administrator.
//! 6. `adding-domain`: record the first domain, which setup created, in `mail_domains`.
//!
//! A failure stops the sequence and is reported as `failed` with the reason, until
//! [`Hosting::start`] is called again. A failure after step 4 leaves a working server,
//! which is reported as such; its first domain can then be added like any other.
//!
//! # Keeping it running
//!
//! [`Hosting::reconcile`] runs at every API start. When a server exists it reattaches
//! the API to the mail network (a recreated API container starts without it) and
//! starts the container, recreating it from its volumes if it was removed.

use std::sync::{Arc, Mutex};

use anyhow::Context;
use secrecy::SecretString;
use serde::Serialize;
use tracing::{Level, event};

use super::stalwart::container::StalwartContainer;
use super::stalwart::{Administrator, Stalwart, StalwartError};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::mail_domain;
use crate::models::mail_server::{self, NewMailServer};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::mail::MailDomainResponse;

const LOCK_POISONED: &str = "mail hosting progress lock poisoned by an earlier panic";

/// Where the creation of the server stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    Preparing,
    PullingImage,
    Starting,
    Configuring,
    Restarting,
    AddingDomain,
}

#[derive(Debug, Clone)]
enum Progress {
    Idle,
    Running(Step),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MailServerState {
    /// No server exists and none is being created.
    NotCreated,
    Creating,
    /// The latest attempt to create one failed.
    Failed,
    Ready,
    /// A server exists but did not answer, or no longer accepts its administrator.
    Unreachable,
}

/// The mail server as clients see it. The administrator is never included.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailServerStatus {
    pub state: MailServerState,
    /// The name the server answers as, once it exists.
    pub hostname: Option<String>,
    /// The step in progress while `creating`.
    pub step: Option<Step>,
    /// Why creation failed, or why the server is unreachable.
    pub error: Option<String>,
}

/// [`Hosting::start`] was refused.
#[derive(Debug, thiserror::Error)]
pub enum StartRefused {
    #[error("a mail server already exists")]
    AlreadyExists,
    #[error("the mail server is already being created")]
    InProgress,
    #[error(transparent)]
    Database(#[from] anyhow::Error),
}

#[derive(Clone)]
pub struct Hosting {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Hosting {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The pool and cipher have nothing useful to print.
        formatter
            .debug_struct("Hosting")
            .field("progress", &self.inner.progress)
            .finish_non_exhaustive()
    }
}

struct Inner {
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
    stalwart: Stalwart,
    container: StalwartContainer,
    progress: Mutex<Progress>,
}

impl Hosting {
    pub fn new(
        database: Pool,
        cipher: Arc<Cipher>,
        events: EventBus,
        stalwart: Stalwart,
        container: StalwartContainer,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                database,
                cipher,
                events,
                stalwart,
                container,
                progress: Mutex::new(Progress::Idle),
            }),
        }
    }

    /// The server's status, asking Stalwart live whether it accepts its administrator.
    ///
    /// # Errors
    /// Fails when the database fails or the stored secret does not decrypt.
    pub async fn status(&self) -> anyhow::Result<MailServerStatus> {
        let progress = self.inner.progress.lock().expect(LOCK_POISONED).clone();
        if let Progress::Running(step) = progress {
            return Ok(MailServerStatus {
                state: MailServerState::Creating,
                hostname: None,
                step: Some(step),
                error: None,
            });
        }

        let Some((hostname, administrator)) = self.stored_server().await? else {
            let (state, error) = match progress {
                Progress::Failed(reason) => (MailServerState::Failed, Some(reason)),
                Progress::Idle | Progress::Running(_) => (MailServerState::NotCreated, None),
            };
            return Ok(MailServerStatus {
                state,
                hostname: None,
                step: None,
                error,
            });
        };

        let (state, error) = match self.inner.stalwart.verify(&administrator).await {
            Ok(()) => (MailServerState::Ready, None),
            Err(StalwartError::Unauthorized) => (
                MailServerState::Unreachable,
                Some(
                    "the mail server no longer accepts Elysium's administrator; its volumes were probably removed"
                        .to_owned(),
                ),
            ),
            Err(error) => (MailServerState::Unreachable, Some(error.to_string())),
        };
        Ok(MailServerStatus {
            state,
            hostname: Some(hostname),
            step: None,
            error,
        })
    }

    /// The administrator of the existing server, or `None` when there is no server.
    ///
    /// # Errors
    /// Fails when the database fails or the stored secret does not decrypt.
    pub async fn administrator(&self) -> anyhow::Result<Option<Administrator>> {
        Ok(self
            .stored_server()
            .await?
            .map(|(_hostname, administrator)| administrator))
    }

    /// Begins creating the server in the background. See the module documentation.
    ///
    /// # Errors
    /// [`StartRefused`] when a server exists or is being created.
    pub async fn start(&self, hostname: String, domain: String) -> Result<(), StartRefused> {
        if self.stored_server().await?.is_some() {
            return Err(StartRefused::AlreadyExists);
        }
        self.claim()?;
        self.publish_status().await;

        let hosting = self.clone();
        tokio::spawn(async move {
            let outcome = hosting.create(&hostname, &domain).await;
            let next = match outcome {
                Ok(()) => {
                    event!(name: "mail.server.created", Level::INFO, mail.server.hostname = %hostname, "created");
                    Progress::Idle
                }
                Err(error) => {
                    event!(
                        name: "mail.server.creation_failed",
                        Level::ERROR,
                        error.message = %format!("{error:#}"),
                        "the mail server could not be created",
                    );
                    Progress::Failed(format!("{error:#}"))
                }
            };
            *hosting.inner.progress.lock().expect(LOCK_POISONED) = next;
            hosting.publish_status().await;
        });
        Ok(())
    }

    /// Brings an existing server back after an API or host restart. See the module
    /// documentation. Failures are logged; the status reports the server unreachable.
    pub async fn reconcile(&self) {
        let outcome = async {
            if self.stored_server().await?.is_none() {
                return anyhow::Ok(());
            }
            let container = &self.inner.container;
            container.attach_api().await?;
            container.ensure_image().await?;
            container.ensure_running().await?;
            anyhow::Ok(())
        }
        .await;

        if let Err(error) = outcome {
            event!(
                name: "mail.server.reconcile_failed",
                Level::WARN,
                error.message = %format!("{error:#}"),
                "the mail server could not be brought up",
            );
        }
    }

    async fn create(&self, hostname: &str, domain: &str) -> anyhow::Result<()> {
        let container = &self.inner.container;
        let stalwart = &self.inner.stalwart;

        container.attach_api().await?;
        self.advance(Step::PullingImage).await;
        container.ensure_image().await?;

        self.advance(Step::Starting).await;
        container.ensure_running().await?;
        let bootstrap_password = container.bootstrap_password().await?;

        self.advance(Step::Configuring).await;
        // Held from before setup: once Stalwart issues the administrator, a database that
        // cannot be reached would lose it for good.
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let administrator = stalwart
            .complete_setup(&bootstrap_password, hostname, domain)
            .await?;
        mail_server::replace(
            &mut connection,
            &self.inner.cipher,
            &NewMailServer {
                hostname: hostname.to_owned(),
                admin_username: administrator.username.clone(),
                admin_secret: administrator.password.clone(),
            },
        )
        .await
        .context("the issued administrator could not be stored; remove the mail server's volumes and start over")?;
        drop(connection);

        self.advance(Step::Restarting).await;
        container.restart().await?;
        stalwart.wait_until_serving(&administrator).await?;

        self.advance(Step::AddingDomain).await;
        let stalwart_id = stalwart.ensure_domain(&administrator, domain).await?;
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let created = mail_domain::create(&mut connection, domain, &stalwart_id, true).await?;
        self.inner
            .events
            .publish(&ServerEvent::MailDomainUpserted(MailDomainResponse::from(
                created,
            )));
        Ok(())
    }

    /// Marks creation as running, unless it already is. Synchronous, so the lock is never
    /// held across an await.
    fn claim(&self) -> Result<(), StartRefused> {
        let mut progress = self.inner.progress.lock().expect(LOCK_POISONED);
        if matches!(*progress, Progress::Running(_)) {
            return Err(StartRefused::InProgress);
        }
        *progress = Progress::Running(Step::Preparing);
        Ok(())
    }

    async fn advance(&self, step: Step) {
        *self.inner.progress.lock().expect(LOCK_POISONED) = Progress::Running(step);
        self.publish_status().await;
    }

    async fn publish_status(&self) {
        match self.status().await {
            Ok(status) => self
                .inner
                .events
                .publish(&ServerEvent::MailServerUpdated(status)),
            Err(error) => event!(
                name: "mail.server.status_failed",
                Level::WARN,
                error.message = %format!("{error:#}"),
                "the mail server status could not be read to publish it",
            ),
        }
    }

    async fn stored_server(&self) -> anyhow::Result<Option<(String, Administrator)>> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let Some(server) = mail_server::find(&mut connection).await? else {
            return Ok(None);
        };

        let password: SecretString = server
            .admin_secret(&self.inner.cipher)
            .context("stored mail server administrator does not decrypt")?;
        let administrator = Administrator {
            username: server.admin_username,
            password,
        };
        Ok(Some((server.hostname, administrator)))
    }
}
