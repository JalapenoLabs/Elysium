// Copyright © 2026 Jalapeno Labs

//! The password vault agents sign in to things with. See `docs/passwords.md`.
//!
//! Elysium holds one Bitwarden or Vaultwarden account (`password_vault`), reached through the
//! official CLI ([`cli`]) run inside the API's own container, never the agents'. The account's
//! API key signs the CLI in, its master password unlocks one long-running `bw serve`, and
//! agents reach the vault only through Elysium's `elysium_passwords` tools
//! (`crate::tools::passwords`), which read one collection and nothing else ([`items`]).
//!
//! [`Passwords`] keeps the CLI running: it signs in and unlocks at startup, restarts `bw serve`
//! if it exits, and syncs the vault every [`SYNC_INTERVAL`] so items added in Bitwarden appear.
//! What it is doing is kept in memory and published as `passwordVault.status`.
//!
//! Every secret it hands an agent is remembered for a while ([`Revealed`]), and every turn's
//! recorded output is scrubbed of them, as it is of secret environment variables.

pub mod cli;
pub mod items;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Context;
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use url::Url;
use uuid::Uuid;

use self::cli::{Cli, CliError, Serve, ServeProcess};
use self::items::{Collection, ItemSummary, LockState, LoginDetails, in_collection, summaries};
use crate::crypto::Cipher;
use crate::database::Pool;
use crate::models::password_vault::{self, PasswordServer, PasswordVault};
use crate::realtime::{EventBus, ServerEvent};

/// Bitwarden's servers in the United States.
pub const BITWARDEN_US: &str = "https://vault.bitwarden.com";
/// Bitwarden's servers in the European Union.
pub const BITWARDEN_EU: &str = "https://vault.bitwarden.eu";

/// Where the CLI is installed in the API image.
pub const CLI_BINARY: &str = "/usr/local/bin/bw";
/// Where the CLI keeps its state: the API's data volume, so a restart keeps the CLI signed in
/// as the same device instead of registering a new one with Bitwarden every time.
pub const CLI_DIRECTORY: &str = "/var/lib/elysium/bitwarden";

/// How often the vault is synced, so items added in Bitwarden reach agents. Bitwarden's own
/// clients sync on a similar schedule; more often only costs Bitwarden requests.
const SYNC_INTERVAL: Duration = Duration::from_mins(5);

/// How often the CLI is checked, and restarted if `bw serve` exited.
const MAINTAIN_INTERVAL: Duration = Duration::from_secs(60);

/// How long a secret handed to an agent is scrubbed from what turns print. A turn rarely runs
/// longer, and a password stays scrubbed for as long as Elysium remembers it.
const REVEALED_RETENTION: Duration = Duration::from_hours(24);

/// The most revealed secrets remembered at once; the oldest go first.
const MAX_REVEALED: usize = 1_000;

/// Where the account lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Server {
    BitwardenUs,
    BitwardenEu,
    /// A server the operator runs, at this https origin.
    SelfHosted(Url),
}

impl Server {
    /// The origin the CLI is pointed at.
    ///
    /// # Panics
    /// Never: Bitwarden's origins are constants that parse.
    pub fn origin(&self) -> Url {
        match self {
            Self::BitwardenUs => Url::parse(BITWARDEN_US).expect("a constant URL"),
            Self::BitwardenEu => Url::parse(BITWARDEN_EU).expect("a constant URL"),
            Self::SelfHosted(origin) => origin.clone(),
        }
    }

    /// The server as stored: its kind, and a self-hosted origin.
    pub fn stored(&self) -> (PasswordServer, Option<String>) {
        match self {
            Self::BitwardenUs => (PasswordServer::BitwardenUs, None),
            Self::BitwardenEu => (PasswordServer::BitwardenEu, None),
            Self::SelfHosted(origin) => (
                PasswordServer::SelfHosted,
                Some(origin.as_str().trim_end_matches('/').to_owned()),
            ),
        }
    }

    /// The server a stored row names.
    ///
    /// # Errors
    /// When a self-hosted row's origin does not parse, which the column's check prevents.
    pub fn from_stored(kind: PasswordServer, url: Option<&str>) -> anyhow::Result<Self> {
        match (kind, url) {
            (PasswordServer::BitwardenUs, _) => Ok(Self::BitwardenUs),
            (PasswordServer::BitwardenEu, _) => Ok(Self::BitwardenEu),
            (PasswordServer::SelfHosted, Some(url)) => Ok(Self::SelfHosted(
                Url::parse(url).context("the stored server URL does not parse")?,
            )),
            (PasswordServer::SelfHosted, None) => {
                anyhow::bail!("a self-hosted vault is stored without its URL")
            }
        }
    }
}

/// What the CLI signs in and unlocks with.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub server: Server,
    pub client_id: String,
    pub client_secret: SecretString,
    pub master_password: SecretString,
}

/// What a vault that signed in and unlocked reaches.
#[derive(Debug, Clone)]
pub struct Connected {
    pub account_email: String,
    pub collections: Vec<Collection>,
}

/// What the vault is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VaultState {
    /// No vault is set up.
    Unconfigured,
    /// The CLI is signing in, unlocking, or starting.
    Starting,
    Ready,
    /// The last attempt failed; `error` says why. It is tried again every minute.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub state: VaultState,
    pub error: Option<String>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

impl VaultStatus {
    const UNCONFIGURED: Self = Self {
        state: VaultState::Unconfigured,
        error: None,
        last_synced_at: None,
    };
    const STARTING: Self = Self {
        state: VaultState::Starting,
        error: None,
        last_synced_at: None,
    };
}

/// Why a vault operation could not be carried out. `Display` is what an admin or an agent reads.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("no password vault is set up")]
    NotConfigured,
    #[error(
        "the password vault is not ready yet; it is signing in or failed to, see Settings, Passwords"
    )]
    NotReady,
    #[error("no collection is chosen for agents yet, so there is nothing they may read")]
    NoCollection,
    #[error("item {0} is not a login in the collection agents may read")]
    NotInCollection(Uuid),
    #[error(transparent)]
    Cli(#[from] CliError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// Secrets handed to agents, kept so every turn's output can be scrubbed of them. Cheap to
/// clone; clones share the list.
#[derive(Debug, Clone, Default)]
pub struct Revealed {
    secrets: Arc<Mutex<VecDeque<(Instant, SecretString)>>>,
}

impl Revealed {
    /// Remembers `secret`.
    pub fn add(&self, secret: &SecretString) {
        let mut secrets = lock(&self.secrets);
        if secrets
            .iter()
            .any(|(_, known)| known.expose_secret() == secret.expose_secret())
        {
            return;
        }
        if secrets.len() == MAX_REVEALED {
            secrets.pop_front();
        }
        secrets.push_back((Instant::now(), secret.clone()));
    }

    /// Calls `visit` with every secret still remembered, forgetting those past
    /// [`REVEALED_RETENTION`].
    pub fn each(&self, mut visit: impl FnMut(&str)) {
        let mut secrets = lock(&self.secrets);
        while secrets
            .front()
            .is_some_and(|(revealed_at, _)| revealed_at.elapsed() > REVEALED_RETENTION)
        {
            secrets.pop_front();
        }
        for (_, secret) in secrets.iter() {
            visit(secret.expose_secret());
        }
    }
}

/// What [`Passwords`] runs on.
pub struct PasswordsDeps {
    pub database: Pool,
    pub cipher: Arc<Cipher>,
    pub events: EventBus,
    pub binary: PathBuf,
    pub directory: PathBuf,
    pub serve_port: u16,
}

impl std::fmt::Debug for PasswordsDeps {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The pool and cipher have nothing useful to print.
        formatter
            .debug_struct("PasswordsDeps")
            .field("binary", &self.binary)
            .field("directory", &self.directory)
            .field("serve_port", &self.serve_port)
            .finish_non_exhaustive()
    }
}

/// The vault. Cheap to clone; clones share the CLI.
#[derive(Clone)]
pub struct Passwords {
    inner: Arc<Inner>,
}

struct Inner {
    cli: Cli,
    serve: Serve,
    database: Pool,
    cipher: Arc<Cipher>,
    events: EventBus,
    status: Mutex<VaultStatus>,
    /// The running `bw serve`. Every start, stop, and sign-in holds this lock, so two never
    /// overlap.
    process: tokio::sync::Mutex<Option<ServeProcess>>,
    revealed: Revealed,
}

impl std::fmt::Debug for Passwords {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Passwords")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

impl Passwords {
    pub fn new(deps: PasswordsDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                cli: Cli::new(deps.binary, deps.directory),
                serve: Serve::new(deps.serve_port),
                database: deps.database,
                cipher: deps.cipher,
                events: deps.events,
                status: Mutex::new(VaultStatus::UNCONFIGURED),
                process: tokio::sync::Mutex::new(None),
                revealed: Revealed::default(),
            }),
        }
    }

    /// The secrets handed to agents, for scrubbing turn output.
    pub fn revealed(&self) -> Revealed {
        self.inner.revealed.clone()
    }

    /// What the vault is doing.
    pub fn status(&self) -> VaultStatus {
        lock(&self.inner.status).clone()
    }

    /// Whether agents can use the vault now.
    pub fn is_ready(&self) -> bool {
        self.status().state == VaultState::Ready
    }

    fn set_status(&self, status: VaultStatus) {
        let changed = {
            let mut current = lock(&self.inner.status);
            let changed = *current != status;
            current.clone_from(&status);
            changed
        };
        if !changed {
            return;
        }
        self.inner
            .events
            .publish(&ServerEvent::PasswordVaultStatus(status));
    }

    fn fail(&self, error: &VaultError) {
        event!(
            name: "passwords.vault.failure",
            Level::WARN,
            error.message = %error,
            "the password vault could not be brought up",
        );
        self.set_status(VaultStatus {
            state: VaultState::Failed,
            error: Some(error.to_string()),
            last_synced_at: None,
        });
    }

    /// Brings the stored vault up, then keeps it up and synced until `shutdown`.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let passwords = self.clone();
        tokio::spawn(async move {
            passwords.restore().await;
            let mut ticks = tokio::time::interval(MAINTAIN_INTERVAL);
            ticks.tick().await;
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    _ = ticks.tick() => passwords.maintain().await,
                }
            }
            if let Some(process) = passwords.inner.process.lock().await.take() {
                process.stop().await;
            }
        })
    }

    /// Signs the CLI in with the stored vault and unlocks it, or marks it unconfigured when
    /// none is stored. A failure is recorded in the status and tried again later.
    pub async fn restore(&self) {
        let mut process = self.inner.process.lock().await;
        match self.stored_credentials().await {
            Ok(Some((credentials, stored))) => {
                if let Err(error) = self
                    .bring_up(&mut process, &credentials, Some(&stored))
                    .await
                {
                    self.fail(&error);
                }
            }
            Ok(None) => {
                if let Some(running) = process.take() {
                    running.stop().await;
                }
                self.set_status(VaultStatus::UNCONFIGURED);
            }
            Err(error) => self.fail(&error),
        }
    }

    async fn stored_credentials(&self) -> Result<Option<(Credentials, PasswordVault)>, VaultError> {
        let mut connection = self
            .inner
            .database
            .get()
            .await
            .context("no database connection available")?;
        let Some(stored) = password_vault::find(&mut connection)
            .await
            .context("cannot read the password vault")?
        else {
            return Ok(None);
        };
        let credentials = Credentials {
            server: Server::from_stored(stored.server, stored.server_url.as_deref())?,
            client_id: stored.client_id.clone(),
            client_secret: stored
                .client_secret(&self.inner.cipher)
                .context("cannot open the stored client secret; did the encryption key change?")?,
            master_password: stored.master_password(&self.inner.cipher).context(
                "cannot open the stored master password; did the encryption key change?",
            )?,
        };
        Ok(Some((credentials, stored)))
    }

    /// Stops `bw serve`, signs in unless the CLI already holds `stored`'s account on the same
    /// server, starts `bw serve`, unlocks it, and syncs. Returns the account's email.
    async fn bring_up(
        &self,
        process: &mut Option<ServeProcess>,
        credentials: &Credentials,
        stored: Option<&PasswordVault>,
    ) -> Result<String, VaultError> {
        if let Some(running) = process.take() {
            running.stop().await;
        }
        self.set_status(VaultStatus::STARTING);

        let cli = &self.inner.cli;
        let origin = credentials.server.origin();
        let signed_in = cli.status().await?;
        let same_account = stored.is_some_and(|stored| {
            signed_in.lock_state != LockState::Unauthenticated
                && signed_in.user_email.as_deref() == Some(stored.account_email.as_str())
                && signed_in.server_url.as_deref().is_some_and(|url| {
                    url.trim_end_matches('/') == origin.as_str().trim_end_matches('/')
                })
        });
        if !same_account {
            cli.sign_in(&origin, &credentials.client_id, &credentials.client_secret)
                .await?;
        }

        let serve = &self.inner.serve;
        let running = cli.serve(serve).await?;
        let unlocked = async {
            serve.unlock(&credentials.master_password).await?;
            serve.sync().await?;
            let status = serve.status().await?;
            status.user_email.ok_or_else(|| {
                VaultError::Cli(CliError::Failed("bw serve reports no account".to_owned()))
            })
        }
        .await;
        match unlocked {
            Ok(email) => {
                *process = Some(running);
                self.set_status(VaultStatus {
                    state: VaultState::Ready,
                    error: None,
                    last_synced_at: Some(Utc::now()),
                });
                Ok(email)
            }
            Err(error) => {
                running.stop().await;
                Err(error)
            }
        }
    }

    /// Signs in and unlocks with `credentials`, replacing whatever the CLI held, and answers
    /// the account and its collections. On failure the stored vault, if any, is brought back.
    ///
    /// # Errors
    /// [`VaultError::Cli`] with the CLI's words for a refused key or master password.
    pub async fn connect(&self, credentials: &Credentials) -> Result<Connected, VaultError> {
        let mut process = self.inner.process.lock().await;
        let connected = async {
            let account_email = self.bring_up(&mut process, credentials, None).await?;
            let collections = self.inner.serve.collections().await?;
            Ok::<_, VaultError>(Connected {
                account_email,
                collections,
            })
        }
        .await;
        if connected.is_err() {
            if let Some(running) = process.take() {
                running.stop().await;
            }
            drop(process);
            // The new credentials failed; whatever is stored keeps working.
            self.restore().await;
        }
        connected
    }

    /// Stops the CLI and signs it out, for a vault being removed.
    pub async fn disconnect(&self) {
        let mut process = self.inner.process.lock().await;
        if let Some(running) = process.take() {
            running.stop().await;
        }
        if let Err(error) = self.inner.cli.sign_out().await {
            event!(
                name: "passwords.vault.sign_out_failure",
                Level::WARN,
                error.message = %error,
                "could not sign the CLI out of the removed vault",
            );
        }
        self.set_status(VaultStatus::UNCONFIGURED);
    }

    /// Restarts `bw serve` if it exited or the vault failed, and syncs when one is due.
    async fn maintain(&self) {
        let status = self.status();
        let exited = {
            let mut process = self.inner.process.lock().await;
            process
                .as_mut()
                .is_some_and(|running| !running.is_running())
        };
        if status.state == VaultState::Failed || exited {
            self.restore().await;
            return;
        }
        let due = status.last_synced_at.is_none_or(|synced| {
            (Utc::now() - synced).to_std().unwrap_or_default() >= SYNC_INTERVAL
        });
        if status.state == VaultState::Ready
            && due
            && let Err(error) = self.sync().await
        {
            event!(
                name: "passwords.vault.sync_failure",
                Level::WARN,
                error.message = %error,
                "the scheduled vault sync failed",
            );
        }
    }

    fn require_ready(&self) -> Result<(), VaultError> {
        match self.status().state {
            VaultState::Ready => Ok(()),
            VaultState::Unconfigured => Err(VaultError::NotConfigured),
            VaultState::Starting | VaultState::Failed => Err(VaultError::NotReady),
        }
    }

    /// Downloads the vault again now.
    ///
    /// # Errors
    /// [`VaultError::NotReady`] unless the vault is up, and the CLI's refusal.
    pub async fn sync(&self) -> Result<(), VaultError> {
        self.require_ready()?;
        self.inner.serve.sync().await?;
        let mut status = self.status();
        status.last_synced_at = Some(Utc::now());
        self.set_status(status);
        Ok(())
    }

    /// Every collection the account reaches, for choosing the one agents may read.
    ///
    /// # Errors
    /// [`VaultError::NotReady`] unless the vault is up, and the CLI's refusal.
    pub async fn collections(&self) -> Result<Vec<Collection>, VaultError> {
        self.require_ready()?;
        Ok(self.inner.serve.collections().await?)
    }

    /// The logins in `collection` matching `search` or `url`, without their secrets.
    ///
    /// # Errors
    /// [`VaultError::NotReady`] unless the vault is up, and the CLI's refusal.
    pub async fn find(
        &self,
        collection: Uuid,
        search: Option<&str>,
        url: Option<&str>,
    ) -> Result<Vec<ItemSummary>, VaultError> {
        self.require_ready()?;
        let items = self.inner.serve.items(collection, search, url).await?;
        Ok(summaries(&items, collection))
    }

    /// Item `id`'s username and password, if it is a login in `collection`. The password is
    /// remembered for scrubbing.
    ///
    /// # Errors
    /// [`VaultError::NotInCollection`] for any other item, and as [`Passwords::find`].
    pub async fn login(&self, collection: Uuid, id: Uuid) -> Result<LoginDetails, VaultError> {
        self.require_ready()?;
        let item = self.inner.serve.item(id).await?;
        if !in_collection(&item, collection) {
            return Err(VaultError::NotInCollection(id));
        }
        let details = LoginDetails::new(&item);
        if let Some(password) = &details.password {
            self.inner
                .revealed
                .add(&SecretString::from(password.clone()));
        }
        Ok(details)
    }

    /// Item `id`'s current TOTP code, if it is a login in `collection`. The code is remembered
    /// for scrubbing.
    ///
    /// # Errors
    /// As [`Passwords::login`]; an item without a TOTP seed is the CLI's refusal.
    pub async fn totp(&self, collection: Uuid, id: Uuid) -> Result<SecretString, VaultError> {
        self.require_ready()?;
        let item = self.inner.serve.item(id).await?;
        if !in_collection(&item, collection) {
            return Err(VaultError::NotInCollection(id));
        }
        let code = self.inner.serve.totp(id).await?;
        self.inner.revealed.add(&code);
        Ok(code)
    }
}

/// Every critical section leaves its value whole, so a poisoned lock holds nothing half done.
fn lock<Value>(mutex: &Mutex<Value>) -> std::sync::MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
