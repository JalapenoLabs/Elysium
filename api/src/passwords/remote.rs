// Copyright © 2026 Jalapeno Labs

//! The password vault across processes: the engine runs the CLI, and the API asks it.
//!
//! The vault is one CLI signed in as one device, with its state on the engine's data volume and
//! `bw serve` on the engine's own loopback, which nothing outside its container can reach. The
//! engine therefore holds the one [`Passwords`]. The API, whose routes and agent tools need the
//! vault while a request waits, holds a [`RemoteVault`] with the same operations, answered by
//! the engine over the bus:
//!
//! - A request is a [`Signal::VaultRequest`] with a fresh id; its answer is a
//!   [`Signal::VaultReply`] with the same id, which only the process that asked is waiting for.
//! - Both are sealed with the encryption key, bound to their id and direction, since they carry
//!   the vault's credentials and the logins agents are handed. Redis never holds a secret, and a
//!   sealed message cannot be replayed as another.
//! - A request the engine does not answer in time fails with [`VaultError::Unreachable`]. Signals
//!   are at most once, so a lost one ends the same way; nothing is retried, since the person or
//!   agent asking can ask again.
//!
//! What the engine hands out for a login or a code it also remembers for scrubbing, in its own
//! process, where every turn's output is scrubbed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use super::cli::CliError;
use super::items::{Collection, ItemSummary, LoginDetails};
use super::{Connected, Credentials, Passwords, Server, VaultError, VaultStatus};
use crate::crypto::Cipher;
use crate::models::password_vault::PasswordServer;
use crate::realtime::{EventBus, Signal};

/// How long the API waits for the engine to connect a vault: it signs in, unlocks, and syncs,
/// which takes Bitwarden a while.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);

/// How long the API waits for the vault's status, which the engine holds in memory and answers at
/// once. Every load of the Passwords page asks it, so an engine that is down costs the page this.
const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

/// How long the API waits for any other answer. The CLI answers a search or a login in moments,
/// so an engine that takes longer is not answering, and whoever waits should hear so.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// One operation on the vault.
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Request {
    Status,
    Connect {
        server: PasswordServer,
        server_url: Option<String>,
        client_id: String,
        client_secret: String,
        master_password: String,
    },
    Disconnect,
    Sync,
    Collections,
    Find {
        collection: Uuid,
        search: Option<String>,
        url: Option<String>,
    },
    Login {
        collection: Uuid,
        id: Uuid,
    },
    Totp {
        collection: Uuid,
        id: Uuid,
    },
}

/// What the vault answered. Not `Debug`, since it may hold a login's password.
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "answer",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Answer {
    Status(VaultStatus),
    Connected {
        account_email: String,
        collections: Vec<Collection>,
    },
    Done,
    Collections(Vec<Collection>),
    Logins(Vec<ItemSummary>),
    Login(LoginDetails),
    Code(String),
    Refused(Refusal),
}

/// A [`VaultError`] as it travels.
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Refusal {
    NotConfigured,
    NotReady,
    NoCollection,
    NotInCollection { id: Uuid },
    CliRefused { message: String },
    CliFailed { message: String },
    Internal { message: String },
}

impl Request {
    /// How long its answer may take.
    const fn timeout(&self) -> Duration {
        match self {
            Self::Status => STATUS_TIMEOUT,
            Self::Connect { .. } => CONNECT_TIMEOUT,
            _ => ANSWER_TIMEOUT,
        }
    }
}

impl From<VaultError> for Refusal {
    fn from(error: VaultError) -> Self {
        match error {
            VaultError::NotConfigured => Self::NotConfigured,
            VaultError::NotReady => Self::NotReady,
            VaultError::NoCollection => Self::NoCollection,
            VaultError::NotInCollection(id) => Self::NotInCollection { id },
            VaultError::Cli(CliError::Refused(message)) => Self::CliRefused { message },
            VaultError::Cli(CliError::Failed(message)) => Self::CliFailed { message },
            // Only the API ever fails to reach the engine.
            VaultError::Unreachable => Self::Internal {
                message: error.to_string(),
            },
            VaultError::Internal(error) => Self::Internal {
                message: format!("{error:#}"),
            },
        }
    }
}

impl From<Refusal> for VaultError {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::NotConfigured => Self::NotConfigured,
            Refusal::NotReady => Self::NotReady,
            Refusal::NoCollection => Self::NoCollection,
            Refusal::NotInCollection { id } => Self::NotInCollection(id),
            Refusal::CliRefused { message } => Self::Cli(CliError::Refused(message)),
            Refusal::CliFailed { message } => Self::Cli(CliError::Failed(message)),
            Refusal::Internal { message } => Self::Internal(anyhow::anyhow!(message)),
        }
    }
}

/// The vault as the API reaches it. Cheap to clone; clones share the requests waiting.
#[derive(Clone)]
pub struct RemoteVault {
    events: EventBus,
    cipher: Arc<Cipher>,
    waiting: Arc<Mutex<HashMap<Uuid, oneshot::Sender<Answer>>>>,
}

impl std::fmt::Debug for RemoteVault {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteVault")
            .finish_non_exhaustive()
    }
}

impl RemoteVault {
    /// A vault reached over `events`, and the task that hands each answer to its request, until
    /// `shutdown`.
    pub fn start(
        events: EventBus,
        cipher: Arc<Cipher>,
        shutdown: CancellationToken,
    ) -> (Self, JoinHandle<()>) {
        let vault = Self {
            events,
            cipher,
            waiting: Arc::default(),
        };
        let mut signals = vault.events.signals();
        let receiving = vault.clone();
        let task = tokio::spawn(async move {
            loop {
                let signal = tokio::select! {
                    () = shutdown.cancelled() => return,
                    signal = signals.next() => signal,
                };
                match signal {
                    Some(Signal::VaultReply { id, sealed }) => receiving.receive(id, &sealed),
                    Some(_) => {}
                    None => return,
                }
            }
        });
        (vault, task)
    }

    /// What the vault is doing. An engine that does not answer reads as a failed vault, saying
    /// so, since nothing can use the vault then.
    pub async fn status(&self) -> VaultStatus {
        match self.ask(Request::Status).await {
            Ok(Answer::Status(status)) => status,
            Ok(other) => failed_status(&refused(other)),
            Err(error) => failed_status(&error),
        }
    }

    /// See [`Passwords::connect`].
    ///
    /// # Errors
    /// As [`Passwords::connect`], and [`VaultError::Unreachable`].
    pub async fn connect(&self, credentials: &Credentials) -> Result<Connected, VaultError> {
        let (server, server_url) = credentials.server.stored();
        let request = Request::Connect {
            server,
            server_url,
            client_id: credentials.client_id.clone(),
            client_secret: credentials.client_secret.expose_secret().to_owned(),
            master_password: credentials.master_password.expose_secret().to_owned(),
        };
        match self.ask(request).await? {
            Answer::Connected {
                account_email,
                collections,
            } => Ok(Connected {
                account_email,
                collections,
            }),
            other => Err(refused(other)),
        }
    }

    /// See [`Passwords::disconnect`]. An engine that does not answer is logged: the row is
    /// removed either way, and an engine that starts without one signs the CLI out.
    pub async fn disconnect(&self) {
        if let Err(error) = self.ask(Request::Disconnect).await {
            event!(
                name: "passwords.remote.disconnect_failure",
                Level::WARN,
                error.message = %error,
                "the engine did not stop the vault's CLI; it stops when the engine next starts",
            );
        }
    }

    /// See [`Passwords::sync`].
    ///
    /// # Errors
    /// As [`Passwords::sync`], and [`VaultError::Unreachable`].
    pub async fn sync(&self) -> Result<(), VaultError> {
        match self.ask(Request::Sync).await? {
            Answer::Done => Ok(()),
            other => Err(refused(other)),
        }
    }

    /// See [`Passwords::collections`].
    ///
    /// # Errors
    /// As [`Passwords::collections`], and [`VaultError::Unreachable`].
    pub async fn collections(&self) -> Result<Vec<Collection>, VaultError> {
        match self.ask(Request::Collections).await? {
            Answer::Collections(collections) => Ok(collections),
            other => Err(refused(other)),
        }
    }

    /// See [`Passwords::find`].
    ///
    /// # Errors
    /// As [`Passwords::find`], and [`VaultError::Unreachable`].
    pub async fn find(
        &self,
        collection: Uuid,
        search: Option<&str>,
        url: Option<&str>,
    ) -> Result<Vec<ItemSummary>, VaultError> {
        let request = Request::Find {
            collection,
            search: search.map(ToOwned::to_owned),
            url: url.map(ToOwned::to_owned),
        };
        match self.ask(request).await? {
            Answer::Logins(logins) => Ok(logins),
            other => Err(refused(other)),
        }
    }

    /// See [`Passwords::login`]; the engine remembers the password for scrubbing.
    ///
    /// # Errors
    /// As [`Passwords::login`], and [`VaultError::Unreachable`].
    pub async fn login(&self, collection: Uuid, id: Uuid) -> Result<LoginDetails, VaultError> {
        match self.ask(Request::Login { collection, id }).await? {
            Answer::Login(details) => Ok(details),
            other => Err(refused(other)),
        }
    }

    /// See [`Passwords::totp`]; the engine remembers the code for scrubbing.
    ///
    /// # Errors
    /// As [`Passwords::totp`], and [`VaultError::Unreachable`].
    pub async fn totp(&self, collection: Uuid, id: Uuid) -> Result<SecretString, VaultError> {
        match self.ask(Request::Totp { collection, id }).await? {
            Answer::Code(code) => Ok(SecretString::from(code)),
            other => Err(refused(other)),
        }
    }

    /// Sends `request` to the engine and waits for its answer.
    async fn ask(&self, request: Request) -> Result<Answer, VaultError> {
        let id = Uuid::now_v7();
        let (sender, receiver) = oneshot::channel();
        lock(&self.waiting).insert(id, sender);

        let timeout = request.timeout();
        let plaintext = serde_json::to_vec(&request).expect("a vault request serializes");
        let sealed = BASE64.encode(self.cipher.seal(&plaintext, &context(REQUEST, id)));
        self.events.signal(&Signal::VaultRequest { id, sealed });

        let answered = tokio::time::timeout(timeout, receiver).await;
        lock(&self.waiting).remove(&id);
        if let Ok(Ok(answer)) = answered {
            return Ok(answer);
        }
        event!(
            name: "passwords.remote.unanswered",
            Level::WARN,
            vault.request.id = %id,
            "the engine did not answer a request to the password vault",
        );
        Err(VaultError::Unreachable)
    }

    /// Hands the answer sealed in `sealed` to request `id`, if this process asked it.
    fn receive(&self, id: Uuid, sealed: &str) {
        let Some(sender) = lock(&self.waiting).remove(&id) else {
            // Another process asked, or this one gave up waiting.
            return;
        };
        match open::<Answer>(&self.cipher, REPLY, id, sealed) {
            Ok(answer) => {
                // The request may have stopped waiting a moment ago; nothing is lost.
                let _ = sender.send(answer);
            }
            Err(error) => event!(
                name: "passwords.remote.unreadable_reply",
                Level::ERROR,
                vault.request.id = %id,
                error.message = %format!("{error:#}"),
                "an answer from the password vault could not be opened",
            ),
        }
    }
}

/// Answers every vault request on the bus with `passwords`, until `shutdown`. The engine's half
/// of [`RemoteVault`].
pub fn serve(
    passwords: Passwords,
    events: EventBus,
    cipher: Arc<Cipher>,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    let mut signals = events.signals();
    tokio::spawn(async move {
        loop {
            let signal = tokio::select! {
                () = shutdown.cancelled() => return,
                signal = signals.next() => signal,
            };
            let Some(signal) = signal else {
                return;
            };
            let Signal::VaultRequest { id, sealed } = signal else {
                continue;
            };
            let request = match open::<Request>(&cipher, REQUEST, id, &sealed) {
                Ok(request) => request,
                Err(error) => {
                    event!(
                        name: "passwords.remote.unreadable_request",
                        Level::ERROR,
                        vault.request.id = %id,
                        error.message = %format!("{error:#}"),
                        "a request to the password vault could not be opened",
                    );
                    continue;
                }
            };
            // Each on its own task: a slow sign-in must not hold up an agent's search.
            let passwords = passwords.clone();
            let events = events.clone();
            let cipher = Arc::clone(&cipher);
            tokio::spawn(async move {
                let answer = answer(&passwords, request).await;
                let plaintext = serde_json::to_vec(&answer).expect("a vault answer serializes");
                let sealed = BASE64.encode(cipher.seal(&plaintext, &context(REPLY, id)));
                events.signal(&Signal::VaultReply { id, sealed });
            });
        }
    })
}

/// Carries `request` out on the engine's vault.
async fn answer(passwords: &Passwords, request: Request) -> Answer {
    let answered = match request {
        Request::Status => Ok(Answer::Status(passwords.status())),
        Request::Connect {
            server,
            server_url,
            client_id,
            client_secret,
            master_password,
        } => match Server::from_stored(server, server_url.as_deref()) {
            Ok(server) => {
                let credentials = Credentials {
                    server,
                    client_id,
                    client_secret: SecretString::from(client_secret),
                    master_password: SecretString::from(master_password),
                };
                passwords
                    .connect(&credentials)
                    .await
                    .map(|connected| Answer::Connected {
                        account_email: connected.account_email,
                        collections: connected.collections,
                    })
            }
            Err(error) => Err(VaultError::Internal(error)),
        },
        Request::Disconnect => {
            passwords.disconnect().await;
            Ok(Answer::Done)
        }
        Request::Sync => passwords.sync().await.map(|()| Answer::Done),
        Request::Collections => passwords.collections().await.map(Answer::Collections),
        Request::Find {
            collection,
            search,
            url,
        } => passwords
            .find(collection, search.as_deref(), url.as_deref())
            .await
            .map(Answer::Logins),
        Request::Login { collection, id } => {
            passwords.login(collection, id).await.map(Answer::Login)
        }
        Request::Totp { collection, id } => passwords
            .totp(collection, id)
            .await
            .map(|code| Answer::Code(code.expose_secret().to_owned())),
    };
    answered.unwrap_or_else(|error| Answer::Refused(Refusal::from(error)))
}

/// Which way a sealed message travels, part of what it is bound to.
const REQUEST: &str = "request";
const REPLY: &str = "reply";

/// What a sealed message is bound to: its direction and its request's id.
fn context(direction: &str, id: Uuid) -> Vec<u8> {
    let mut context = format!("elysium/vault/{direction}:").into_bytes();
    context.extend_from_slice(id.as_bytes());
    context
}

fn open<Message: serde::de::DeserializeOwned>(
    cipher: &Cipher,
    direction: &str,
    id: Uuid,
    sealed: &str,
) -> anyhow::Result<Message> {
    let envelope = BASE64.decode(sealed)?;
    let plaintext = cipher.open(&envelope, &context(direction, id))?;
    Ok(serde_json::from_slice(plaintext.expose_secret())?)
}

/// The error an answer carries. Any other answer does not fit its request, which only builds of
/// the API and the engine that disagree could send; it is never printed, since it may hold a
/// login.
fn refused(answer: Answer) -> VaultError {
    match answer {
        Answer::Refused(refusal) => refusal.into(),
        _ => VaultError::Internal(anyhow::anyhow!(
            "the engine answered the password vault with an answer of another kind"
        )),
    }
}

fn failed_status(error: &VaultError) -> VaultStatus {
    VaultStatus {
        state: super::VaultState::Failed,
        error: Some(error.to_string()),
        last_synced_at: None,
    }
}

/// Every critical section leaves the map whole, so a poisoned lock holds nothing half done.
fn lock<Value>(mutex: &Mutex<Value>) -> std::sync::MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_request_reaches_the_engine_and_its_answer_comes_back_to_who_asked() {
        let events = EventBus::in_process();
        let cipher = Arc::new(crate::test_support::cipher());
        let database = crate::database::Pool::builder(
            diesel_async::pooled_connection::AsyncDieselConnectionManager::new(
                "postgres://unused@127.0.0.1:1/unused",
            ),
        )
        .build()
        .expect("a lazy pool");
        let passwords =
            crate::services::build_passwords(database, Arc::clone(&cipher), events.clone());
        let shutdown = CancellationToken::new();
        let server = serve(
            passwords,
            events.clone(),
            Arc::clone(&cipher),
            shutdown.clone(),
        );
        let (vault, receiver) = RemoteVault::start(events, cipher, shutdown.clone());

        // A vault that was never set up answers as unconfigured, through the engine.
        let status = vault.status().await;
        assert_eq!(status.state, super::super::VaultState::Unconfigured);
        assert!(matches!(
            vault.collections().await,
            Err(VaultError::NotConfigured)
        ));

        // Both halves stop with the shutdown token, whatever still holds the bus.
        shutdown.cancel();
        server.await.expect("the engine's half stops");
        receiver.await.expect("the API's half stops");
    }

    #[tokio::test(start_paused = true)]
    async fn a_vault_nobody_answers_reads_as_unreachable_without_waiting_long() {
        let events = EventBus::in_process();
        let (vault, _receiver) = RemoteVault::start(
            events,
            Arc::new(crate::test_support::cipher()),
            CancellationToken::new(),
        );
        let started = tokio::time::Instant::now();
        let status = vault.status().await;
        assert_eq!(status.state, super::super::VaultState::Failed);
        assert_eq!(
            started.elapsed(),
            STATUS_TIMEOUT,
            "a page waits seconds, not minutes"
        );
        let started = tokio::time::Instant::now();
        assert!(matches!(vault.sync().await, Err(VaultError::Unreachable)));
        assert_eq!(started.elapsed(), ANSWER_TIMEOUT);
    }

    #[test]
    fn a_sealed_message_opens_only_as_what_it_was_sealed_for() {
        let cipher = crate::test_support::cipher();
        let id = Uuid::now_v7();
        let plaintext = serde_json::to_vec(&Request::Sync).expect("serializes");
        let sealed = BASE64.encode(cipher.seal(&plaintext, &context(REQUEST, id)));

        open::<Request>(&cipher, REQUEST, id, &sealed).expect("opens as what it was sealed for");
        assert!(
            open::<Request>(&cipher, REPLY, id, &sealed).is_err(),
            "a request cannot be replayed as a reply"
        );
        assert!(
            open::<Request>(&cipher, REQUEST, Uuid::now_v7(), &sealed).is_err(),
            "nor as another request"
        );
    }
}
