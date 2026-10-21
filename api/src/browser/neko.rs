// Copyright © 2026 Jalapeno Labs

//! The API's side of a browser: Neko's HTTP API and signaling socket, and Chromium's
//! `DevTools` endpoint, all reached on the browser's own network by its [`host_name`].
//!
//! Neko 3.1.6's API, as its server routes it (`server/internal/api/router.go`):
//! `POST /api/login` with `{username, password}` answers a session whose `token` is in the
//! body when session cookies are off, and `GET /api/ws?token=...` is the signaling socket.
//!
//! Chromium refuses a `DevTools` request whose `Host` header is not an IP address or
//! `localhost`, so every `DevTools` call sends `Host: localhost` whatever name it dials.

use std::time::Duration;

use reqwest::header::HOST;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

use super::container::{DEVTOOLS_PORT, NEKO_HTTP_PORT, host_name};

/// The signaling socket to one browser, as the API holds it for a viewer.
pub type Upstream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long one call to a browser may take. Everything is on the host's own network, so
/// anything slower is a browser that is not answering.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a new browser may take before its `DevTools` port answers. Chromium starts in a
/// few seconds once the image is pulled; the rest covers a busy host.
const DEVTOOLS_READY_TIMEOUT: Duration = Duration::from_secs(90);
const DEVTOOLS_READY_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The name the API signs in under. Neko shows it to other viewers of the same browser.
const MEMBER_NAME: &str = "elysium";

/// A browser refused or did not answer.
#[derive(Debug, thiserror::Error)]
pub enum NekoError {
    #[error("the browser did not answer: {0}")]
    Http(String),
    #[error("the browser refused the sign-in ({0})")]
    SignIn(reqwest::StatusCode),
    #[error("the browser's signaling socket refused the connection: {0}")]
    Socket(String),
    #[error("the browser's DevTools port did not answer within {0:?}")]
    DevtoolsNotReady(Duration),
}

#[derive(Debug, Clone)]
pub struct Neko {
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct SignedIn {
    token: String,
}

impl Neko {
    pub const fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Signs in as the browser's member and opens its signaling socket.
    ///
    /// # Errors
    /// [`NekoError`] when the browser does not answer, refuses the password, or refuses the
    /// socket.
    pub async fn connect(&self, port: u16, password: &SecretString) -> Result<Upstream, NekoError> {
        let token = self.sign_in(port, password).await?;
        let mut url = Url::parse(&format!("ws://{}:{NEKO_HTTP_PORT}/api/ws", host_name(port)))
            .expect("a browser's socket URL parses");
        url.query_pairs_mut()
            .append_pair("token", token.expose_secret());

        let (socket, _response) =
            tokio::time::timeout(CALL_TIMEOUT, tokio_tungstenite::connect_async(url.as_str()))
                .await
                .map_err(|_elapsed| NekoError::Socket("timed out".to_owned()))?
                .map_err(|error| NekoError::Socket(error.to_string()))?;
        Ok(socket)
    }

    async fn sign_in(&self, port: u16, password: &SecretString) -> Result<SecretString, NekoError> {
        let url = format!("http://{}:{NEKO_HTTP_PORT}/api/login", host_name(port));
        let response = self
            .http
            .post(url)
            .timeout(CALL_TIMEOUT)
            .json(&json!({ "username": MEMBER_NAME, "password": password.expose_secret() }))
            .send()
            .await
            .map_err(|error| NekoError::Http(error.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(NekoError::SignIn(response.status()));
        }
        let signed_in: SignedIn = response
            .json()
            .await
            .map_err(|error| NekoError::Http(error.without_url().to_string()))?;
        Ok(SecretString::from(signed_in.token))
    }

    /// Waits for a new browser's `DevTools` port, then opens `start_url` in a new tab.
    ///
    /// # Errors
    /// [`NekoError::DevtoolsNotReady`] when the port never answers, or [`NekoError::Http`]
    /// when Chromium refuses the tab.
    pub async fn open_start_page(&self, port: u16, start_url: &Url) -> Result<(), NekoError> {
        self.wait_for_devtools(port).await?;

        // Chromium reads the page to open from the whole query string.
        let mut url = devtools_url(port, "json/new");
        url.set_query(Some(start_url.as_str()));
        let response = self
            .http
            .put(url)
            .header(HOST, "localhost")
            .timeout(CALL_TIMEOUT)
            .send()
            .await
            .map_err(|error| NekoError::Http(error.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(NekoError::Http(format!(
                "Chromium refused to open the start page ({})",
                response.status()
            )));
        }
        Ok(())
    }

    async fn wait_for_devtools(&self, port: u16) -> Result<(), NekoError> {
        let deadline = tokio::time::Instant::now() + DEVTOOLS_READY_TIMEOUT;
        loop {
            let answered = self
                .http
                .get(devtools_url(port, "json/version"))
                .header(HOST, "localhost")
                .timeout(CALL_TIMEOUT)
                .send()
                .await
                .is_ok_and(|response| response.status().is_success());
            if answered {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(NekoError::DevtoolsNotReady(DEVTOOLS_READY_TIMEOUT));
            }
            tokio::time::sleep(DEVTOOLS_READY_POLL_INTERVAL).await;
        }
    }
}

fn devtools_url(port: u16, path: &str) -> Url {
    Url::parse(&format!(
        "http://{}:{DEVTOOLS_PORT}/{path}",
        host_name(port)
    ))
    .expect("a browser's DevTools URL parses")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_page_is_the_whole_query_of_the_new_tab_request() {
        let start = Url::parse("https://outlook.office.com/mail/?folder=inbox&view=list")
            .expect("a static URL parses");
        let mut url = devtools_url(59002, "json/new");
        url.set_query(Some(start.as_str()));
        assert_eq!(
            url.as_str(),
            "http://elysium-browser-59002:9223/json/new?https://outlook.office.com/mail/?folder=inbox&view=list"
        );
    }
}
