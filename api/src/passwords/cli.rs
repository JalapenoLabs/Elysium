// Copyright © 2026 Jalapeno Labs

//! The official Bitwarden CLI, `bw`, run by the API.
//!
//! Signing in, signing out, and choosing the server are one-shot commands ([`Cli::run`]). Once
//! signed in, one `bw serve` stays running ([`Cli::serve`]), and everything else goes through
//! its HTTP API ([`Serve`]), which keeps the vault unlocked in that process alone.
//!
//! Every process gets a cleared environment: the CLI's state directory, `BW_NOINTERACTION` so it
//! never waits on a prompt, and the secrets the command needs. Secrets travel only in the
//! environment or a request body, never on a command line another process could read.
//!
//! `bw serve` listens on the API container's own loopback, which nothing outside the container
//! can reach, and only Elysium runs inside it. The CLI also refuses any request whose `Host` is
//! not loopback and any request carrying an `Origin`, so a browser could not reach it either.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio::process::{Child, Command};
use tracing::{Level, event};
use url::Url;
use uuid::Uuid;

use super::items::{
    Collection, Item, List, LockState, ServeResponse, Status, StatusTemplate, Text,
};

/// How long a one-shot command may take. Signing in derives a key from the master password,
/// which Argon2 can make take several seconds on a busy host.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// How long one request to `bw serve` may take. A sync downloads the whole vault.
const SERVE_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// How long `bw serve` may take to start listening.
const SERVE_START_TIMEOUT: Duration = Duration::from_secs(60);

/// The CLI's own default port, on loopback.
pub const SERVE_PORT: u16 = 8087;

/// Why the CLI could not do what it was asked.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CliError {
    /// The CLI answered with a refusal, in its own words, such as a wrong master password.
    #[error("{0}")]
    Refused(String),
    /// The CLI could not be run, or answered something Elysium cannot read.
    #[error("the Bitwarden CLI failed: {0}")]
    Failed(String),
}

/// How to run the CLI: where its binary is and where it keeps its state.
#[derive(Debug, Clone)]
pub struct Cli {
    binary: PathBuf,
    directory: PathBuf,
}

/// A running `bw serve`. Dropping it kills the process.
#[derive(Debug)]
pub struct ServeProcess {
    child: Child,
}

impl ServeProcess {
    /// Whether the process is still running.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Stops the process and waits for it to exit.
    pub async fn stop(mut self) {
        if let Err(error) = self.child.kill().await {
            event!(
                name: "passwords.serve.stop_failure",
                Level::WARN,
                error.message = %error,
                "could not stop bw serve",
            );
        }
    }
}

impl Cli {
    pub const fn new(binary: PathBuf, directory: PathBuf) -> Self {
        Self { binary, directory }
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .args(arguments)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("HOME", &self.directory)
            .env("BITWARDENCLI_APPDATA_DIR", &self.directory)
            .env("BW_NOINTERACTION", "true")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        command
    }

    /// Runs `bw <arguments> --response` with `secrets` in its environment, and reads its
    /// answer.
    ///
    /// # Errors
    /// [`CliError::Refused`] with the CLI's message when it answers with a failure, and
    /// [`CliError::Failed`] when it cannot be run or its answer cannot be read.
    pub async fn run<Data: DeserializeOwned>(
        &self,
        arguments: &[&str],
        secrets: &[(&str, &SecretString)],
    ) -> Result<Option<Data>, CliError> {
        tokio::fs::create_dir_all(&self.directory)
            .await
            .map_err(|error| CliError::Failed(format!("cannot create its directory: {error}")))?;
        let mut command = self.command(arguments);
        command.arg("--response");
        for (key, value) in secrets {
            command.env(key, value.expose_secret());
        }
        let output = tokio::time::timeout(COMMAND_TIMEOUT, command.output())
            .await
            .map_err(|_elapsed| CliError::Failed(format!("`bw {}` timed out", arguments[0])))?
            .map_err(|error| CliError::Failed(format!("cannot run bw: {error}")))?;

        let response: ServeResponse<Data> =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                CliError::Failed(format!(
                    "`bw {}` answered something unreadable ({error}); it printed: {}",
                    arguments[0],
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            })?;
        if !response.success {
            return Err(CliError::Refused(
                response
                    .message
                    .unwrap_or_else(|| format!("`bw {}` failed", arguments[0])),
            ));
        }
        Ok(response.data)
    }

    /// Where the CLI stands: signed out, locked, or unlocked, and with which server.
    ///
    /// # Errors
    /// As [`Cli::run`].
    pub async fn status(&self) -> Result<Status, CliError> {
        let template: Option<StatusTemplate> = self.run(&["status"], &[]).await?;
        template
            .map(|template| template.template)
            .ok_or_else(|| CliError::Failed("`bw status` answered no status".to_owned()))
    }

    /// Signs out of whatever account the CLI holds and signs in to `client_id` on `server`
    /// with its API key. The vault stays locked until `bw serve` unlocks it.
    ///
    /// # Errors
    /// As [`Cli::run`]; a refused key is [`CliError::Refused`].
    pub async fn sign_in(
        &self,
        server: &Url,
        client_id: &str,
        client_secret: &SecretString,
    ) -> Result<(), CliError> {
        self.sign_out().await?;
        self.run::<serde_json::Value>(&["config", "server", server.as_str()], &[])
            .await?;
        let client_id = SecretString::from(client_id.to_owned());
        self.run::<serde_json::Value>(
            &["login", "--apikey"],
            &[
                ("BW_CLIENTID", &client_id),
                ("BW_CLIENTSECRET", client_secret),
            ],
        )
        .await?;
        Ok(())
    }

    /// Signs out, if the CLI holds an account.
    ///
    /// # Errors
    /// As [`Cli::run`].
    pub async fn sign_out(&self) -> Result<(), CliError> {
        if self.status().await?.lock_state == LockState::Unauthenticated {
            return Ok(());
        }
        self.run::<serde_json::Value>(&["logout"], &[]).await?;
        Ok(())
    }

    /// Starts `bw serve` on loopback and waits for it to answer.
    ///
    /// # Errors
    /// [`CliError::Failed`] when it cannot start or does not answer in time.
    pub async fn serve(&self, serve: &Serve) -> Result<ServeProcess, CliError> {
        let port = serve.address.port().to_string();
        let mut command = self.command(&["serve", "--hostname", "127.0.0.1", "--port", &port]);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        let child = command
            .spawn()
            .map_err(|error| CliError::Failed(format!("cannot start bw serve: {error}")))?;
        let mut process = ServeProcess { child };

        let deadline = tokio::time::Instant::now() + SERVE_START_TIMEOUT;
        loop {
            if serve.status().await.is_ok() {
                return Ok(process);
            }
            if !process.is_running() {
                return Err(CliError::Failed("bw serve exited as it started".to_owned()));
            }
            if tokio::time::Instant::now() >= deadline {
                process.stop().await;
                return Err(CliError::Failed(
                    "bw serve did not answer in time".to_owned(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

/// The HTTP API of a running `bw serve`.
#[derive(Debug, Clone)]
pub struct Serve {
    http: reqwest::Client,
    address: SocketAddr,
}

impl Serve {
    /// A client for `bw serve` on loopback at `port`.
    ///
    /// # Panics
    /// Panics if the HTTP client cannot be built, which only a broken TLS backend could cause.
    pub fn new(port: u16) -> Self {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(SERVE_REQUEST_TIMEOUT)
            .build()
            .expect("a plain HTTP client builds");
        Self {
            http,
            address: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    async fn read<Data: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Data, CliError> {
        let response = request
            .send()
            .await
            .map_err(|error| CliError::Failed(format!("bw serve did not answer: {error}")))?;
        let answer: ServeResponse<Data> = response.json().await.map_err(|error| {
            CliError::Failed(format!("bw serve answered something unreadable: {error}"))
        })?;
        if !answer.success {
            return Err(CliError::Refused(
                answer
                    .message
                    .unwrap_or_else(|| "bw serve refused".to_owned()),
            ));
        }
        answer
            .data
            .ok_or_else(|| CliError::Failed("bw serve answered without data".to_owned()))
    }

    /// Where the CLI stands with its account.
    ///
    /// # Errors
    /// As every request: [`CliError::Refused`] with the CLI's message, or [`CliError::Failed`].
    pub async fn status(&self) -> Result<Status, CliError> {
        let template: StatusTemplate = self.read(self.http.get(self.url("/status"))).await?;
        Ok(template.template)
    }

    /// Unlocks the vault in the serving process with the master password.
    ///
    /// # Errors
    /// [`CliError::Refused`] for a wrong master password.
    pub async fn unlock(&self, master_password: &SecretString) -> Result<(), CliError> {
        let body = json!({ "password": master_password.expose_secret() });
        let _session: serde_json::Value = self
            .read(self.http.post(self.url("/unlock")).json(&body))
            .await?;
        Ok(())
    }

    /// Downloads the vault again, so items added since appear.
    ///
    /// # Errors
    /// As every request.
    pub async fn sync(&self) -> Result<(), CliError> {
        let _message: serde_json::Value = self.read(self.http.post(self.url("/sync"))).await?;
        Ok(())
    }

    /// Every collection the account reaches.
    ///
    /// # Errors
    /// As every request.
    pub async fn collections(&self) -> Result<Vec<Collection>, CliError> {
        let list: List<Collection> = self
            .read(self.http.get(self.url("/list/object/collections")))
            .await?;
        Ok(list.data)
    }

    /// The items in `collection` matching `search` (name, username, or URI words) and `url`
    /// (the logins for a site, by Bitwarden's own URI matching).
    ///
    /// # Errors
    /// As every request.
    pub async fn items(
        &self,
        collection: Uuid,
        search: Option<&str>,
        url: Option<&str>,
    ) -> Result<Vec<Item>, CliError> {
        let mut query = vec![("collectionid", collection.to_string())];
        if let Some(search) = search {
            query.push(("search", search.to_owned()));
        }
        if let Some(url) = url {
            query.push(("url", url.to_owned()));
        }
        let list: List<Item> = self
            .read(self.http.get(self.url("/list/object/items")).query(&query))
            .await?;
        Ok(list.data)
    }

    /// One item by id.
    ///
    /// # Errors
    /// As every request; an unknown id is [`CliError::Refused`] ("Not found.").
    pub async fn item(&self, id: Uuid) -> Result<Item, CliError> {
        self.read(self.http.get(self.url(&format!("/object/item/{id}"))))
            .await
    }

    /// The current TOTP code of item `id`.
    ///
    /// # Errors
    /// As every request; an item without a TOTP seed is [`CliError::Refused`].
    pub async fn totp(&self, id: Uuid) -> Result<SecretString, CliError> {
        let text: Text = self
            .read(self.http.get(self.url(&format!("/object/totp/{id}"))))
            .await?;
        Ok(SecretString::from(text.data))
    }
}
