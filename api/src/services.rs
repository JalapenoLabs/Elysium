// Copyright © 2026 Jalapeno Labs

//! What the API and the engine are both built from.
//!
//! [`Foundation::connect`] is the start both processes share: configuration, the encryption
//! key, the schema check, store connections (with retry), and the event bus's Redis
//! subscriptions. The `build_*` functions assemble the services that both hold, each with the
//! clients it needs; which of them a process starts is up to the process (`crate::server`,
//! `crate::engine`).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use bollard::{API_DEFAULT_VERSION, Docker};
use redis::aio::ConnectionManager;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use crate::action_items::links::Links;
use crate::auth::kratos::Kratos;
use crate::auth::{Auth, hook_key};
use crate::backups::dump::{Dumper, POSTGRES_CONTAINER};
use crate::browser::Browsers;
use crate::browser::container::BrowserContainers;
use crate::browser::neko::Neko;
use crate::config::Config;
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::database::migrations;
use crate::github::Github;
use crate::jira::Jira;
use crate::mail::Mail;
use crate::mail::broker::Broker;
use crate::mail::dns::DnsChecker;
use crate::mail::hosting::Hosting;
use crate::mail::stalwart::Stalwart;
use crate::mail::stalwart::container::StalwartContainer;
use crate::models::secret_copy;
use crate::oauth::hydra::Hydra;
use crate::passwords::{self, Passwords, PasswordsDeps};
use crate::realtime::EventBus;
use crate::runner::workspace::WorkspaceContainer;
use crate::storage::Storage;
use crate::version::VersionInfo;
use crate::whisper::Whisper;
use crate::{connections, shutdown};

/// How long one Docker call may take. Pulling the mail server's image is one call, and
/// can take minutes on a slow connection.
const DOCKER_TIMEOUT_SECONDS: u64 = 600;

/// How long one Docker call may take for a backup. A dump is one attached exec stream that
/// stays open until `pg_dump` has written every table, which on a large database takes a while.
const BACKUP_DOCKER_TIMEOUT_SECONDS: u64 = 2 * 60 * 60;

/// How long one Docker call may take in the workspace. A turn's output is one attached exec
/// stream that stays open for the whole turn, and the client's timeout covers a request end
/// to end, so it must outlast the longest turn. Hung turns are stopped by the runner's own
/// heartbeat long before this.
const WORKSPACE_DOCKER_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;

/// The connections and clients every service of either process shares.
#[derive(Clone)]
pub struct Foundation {
    pub config: Arc<Config>,
    pub version: Arc<VersionInfo>,
    pub cipher: Arc<Cipher>,
    pub database: Pool,
    /// A multiplexed connection for what the API keeps in Redis, such as sessions.
    pub redis: ConnectionManager,
    /// One HTTP client for every outbound call: the OAuth broker, Stalwart, GitHub, Jira, and
    /// storage providers share its connection pool.
    pub http: reqwest::Client,
    pub events: EventBus,
    pub storage: Storage,
    pub github: Github,
    pub jira: Jira,
    pub links: Links,
    /// Cancelled when shutdown begins; every loop, stream, and turn ends on it.
    pub shutdown: CancellationToken,
}

impl std::fmt::Debug for Foundation {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Foundation")
            .field("version", &self.version.version)
            .finish_non_exhaustive()
    }
}

impl Foundation {
    /// Reads the configuration and connects everything, for the process named `process`.
    ///
    /// # Errors
    /// Fails on invalid configuration, an invalid encryption key, pending migrations, or
    /// unreachable stores.
    pub async fn connect(process: &'static str) -> Result<Self> {
        let config = Config::from_env().context("configuration is invalid")?;
        let version = Arc::new(VersionInfo::from_build()?);
        event!(
            name: "process.startup.begin",
            Level::INFO,
            service.name = process,
            service.version = version.version,
            vcs.ref.head.revision = version.git.commit.as_ref().map_or("", |commit| commit.short_hash.as_str()),
            "starting",
        );

        let cipher = Arc::new(
            Cipher::from_base64_key(&config.encryption_key)
                .context("ELYSIUM_ENCRYPTION_KEY is invalid")?,
        );
        let database =
            connections::connect_postgres(&config.database_url, config.database_max_connections)
                .await?;
        migrations::ensure_up_to_date(config.database_url.clone()).await?;
        reseal_copied_secrets(&database, &cipher).await?;
        let redis = connections::connect_redis(&config.redis_url).await?;
        // Subscribed before anything can publish or any stream can open, so nothing published
        // from here on is missed by this process's listeners.
        let events = EventBus::connect(&config.redis_url).await?;

        let http = reqwest::Client::builder()
            .user_agent(concat!("elysium-api/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("cannot build the HTTP client")?;
        Ok(Self::assemble(
            config, version, cipher, database, redis, http, events,
        ))
    }

    /// The clients built on top of the connections. Tests assemble a foundation of their own
    /// connections through this too.
    pub fn assemble(
        config: Config,
        version: Arc<VersionInfo>,
        cipher: Arc<Cipher>,
        database: Pool,
        redis: ConnectionManager,
        http: reqwest::Client,
        events: EventBus,
    ) -> Self {
        let storage = Storage::new(http.clone());
        let github = Github::new(http.clone());
        let jira = Jira::new(http.clone());
        let links = Links::new(
            database.clone(),
            Arc::clone(&cipher),
            &jira,
            &github,
            events.clone(),
        );
        Self {
            config: Arc::new(config),
            version,
            cipher,
            database,
            redis,
            http,
            events,
            storage,
            github,
            jira,
            links,
            shutdown: CancellationToken::new(),
        }
    }

    /// Resolves when a shutdown signal arrives, and cancels [`Self::shutdown`].
    pub async fn shutdown_signal(&self) {
        shutdown::signal().await;
        self.shutdown.cancel();
    }

    /// Closes the stores, last of all: the event bus flushes what was published on the way
    /// out, the pool drops its idle connections, and the Redis manager, a multiplexed socket,
    /// closes once its last handle drops.
    pub async fn close(self) {
        self.events.close().await;
        self.database.close();
        event!(name: "connection.close.success", Level::INFO, db.system.name = "postgres", "store connection closed");
        drop(self.redis);
        event!(name: "connection.close.success", Level::INFO, db.system.name = "redis", "store connection closed");
    }
}

/// Re-seals the secrets a migration copied for their own rows, before anything reads one. Both
/// processes run it at startup; each copy is re-sealed once, by whichever gets to it first.
async fn reseal_copied_secrets(database: &Pool, cipher: &Cipher) -> Result<()> {
    let mut connection = database
        .get()
        .await
        .context("no database connection to re-seal copied secrets")?;
    let resealed = secret_copy::reseal_all(&mut connection, cipher)
        .await
        .context("cannot read the secrets migrations copied")?;
    if resealed > 0 {
        event!(
            name: "secrets.copies.resealed",
            Level::INFO,
            secret.count = resealed,
            "re-sealed {{secret.count}} copied secrets for their own rows",
        );
    }
    Ok(())
}

/// Kratos, and what requests are checked against. See `crate::auth`.
pub fn build_auth(config: &Config, http: reqwest::Client) -> Auth {
    Auth {
        kratos: Kratos::new(
            http.clone(),
            config.kratos_public_url.clone(),
            config.kratos_admin_url.clone(),
        ),
        hydra: Hydra::new(http, config.hydra_admin_url.clone()),
        hook_key: hook_key::derive(&config.encryption_key),
        public_url: config.public_url.clone(),
    }
}

/// The filtered Docker API, with calls allowed `timeout_seconds`. Connecting is lazy: nothing
/// reaches Docker until it is first needed.
///
/// # Errors
/// Fails when `DOCKER_URL` is not a URL Docker can be reached at.
fn build_docker(config: &Config, timeout_seconds: u64) -> Result<Docker> {
    Docker::connect_with_http(
        config.docker.as_str().trim_end_matches('/'),
        timeout_seconds,
        API_DEFAULT_VERSION,
    )
    .context("DOCKER_URL is invalid")
}

/// The agent workspace container, through Docker. Only the engine creates or replaces it; the
/// API works in it once it runs.
///
/// # Errors
/// Fails when `DOCKER_URL` is not a URL Docker can be reached at.
pub fn build_workspace(config: &Config) -> Result<WorkspaceContainer> {
    Ok(WorkspaceContainer::new(
        build_docker(config, WORKSPACE_DOCKER_TIMEOUT_SECONDS)?,
        config.workspace_image.clone(),
    ))
}

/// Whisper, which transcribes recordings in containers of its own. Its client allows a whole
/// day per call, like the workspace's: a transcription's input and output are one attached
/// stream that stays open until it ends, which its own timeout bounds far sooner.
///
/// # Errors
/// Fails when `DOCKER_URL` is not a URL Docker can be reached at.
pub fn build_whisper(config: &Config) -> Result<Whisper> {
    Ok(Whisper::new(
        build_docker(config, WORKSPACE_DOCKER_TIMEOUT_SECONDS)?,
        config.whisper_image.clone(),
    ))
}

/// What dumps the databases for backups: `pg_dump` in the Postgres container `compose.yml`
/// names, through Docker.
///
/// # Errors
/// Fails when `DOCKER_URL` is not a URL Docker can be reached at.
pub fn build_dumper(config: &Config) -> Result<Dumper> {
    Ok(Dumper::Docker {
        docker: build_docker(config, BACKUP_DOCKER_TIMEOUT_SECONDS)?,
        container: POSTGRES_CONTAINER.to_owned(),
    })
}

/// The mail services. The broker and Stalwart share `http`.
///
/// # Errors
/// Fails when `DOCKER_URL` is not a URL, or the resolver configuration cannot be read.
pub fn build_mail(
    config: &Config,
    http: reqwest::Client,
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
) -> Result<Mail> {
    let mail_config = &config.mail;

    let broker = mail_config.oauth_broker.clone().map(|public_url| {
        let internal_url = mail_config
            .oauth_broker_internal
            .clone()
            .unwrap_or_else(|| public_url.clone());
        Broker::new(http.clone(), public_url, internal_url)
    });
    let stalwart = Stalwart::new(http);

    let hosting = Hosting::new(
        database,
        cipher,
        events,
        stalwart.clone(),
        StalwartContainer::new(build_docker(config, DOCKER_TIMEOUT_SECONDS)?),
        mail_config.ingress_address,
    );
    let dns = DnsChecker::from_system().context("cannot read the resolver configuration")?;

    event!(
        name: "mail.services.configured",
        Level::INFO,
        mail.broker = broker.is_some(),
        mail.docker.url = %config.docker,
        mail.ingress.address = ?mail_config.ingress_address,
        "mail services configured",
    );
    Ok(Mail {
        broker,
        stalwart,
        hosting,
        dns,
    })
}

/// The password vault, run through the CLI the image carries, on its own loopback. Only the
/// engine runs it; the API reaches it through `crate::passwords::remote`.
pub fn build_passwords(database: Pool, cipher: Arc<Cipher>, events: EventBus) -> Passwords {
    Passwords::new(PasswordsDeps {
        database,
        cipher,
        events,
        binary: PathBuf::from(passwords::CLI_BINARY),
        directory: PathBuf::from(passwords::CLI_DIRECTORY),
        serve_port: passwords::cli::SERVE_PORT,
    })
}

/// The browsers the host streams. They share `http` with every other outbound call.
///
/// # Errors
/// Fails when `DOCKER_URL` is not a URL Docker can be reached at.
pub fn build_browsers(
    config: &Config,
    http: reqwest::Client,
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
) -> Result<Browsers> {
    Ok(Browsers::new(
        database,
        cipher,
        events,
        BrowserContainers::new(
            build_docker(config, DOCKER_TIMEOUT_SECONDS)?,
            &config.encryption_key,
        ),
        Neko::new(http),
        config.public_url.clone(),
    ))
}
