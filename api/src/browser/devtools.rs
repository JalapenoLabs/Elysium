// Copyright © 2026 Jalapeno Labs

//! Reads a signed-in page over the Chrome `DevTools` Protocol: finds the browser's tab for a
//! service and evaluates a script in it, as the page itself.
//!
//! Each browser's `DevTools` port is reached on its own network through the forwarder in
//! `neko/cdp-forward.py`. Chromium refuses any `Host` header that is not an IP address or
//! `localhost`, so every request, the WebSocket handshake included, sends `Host: localhost`;
//! the WebSocket address Chromium reports names its own loopback port, so its path is joined
//! to the forwarder's address instead.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reqwest::header::HOST;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use url::Url;

use super::container::{DEVTOOLS_PORT, host_name};

/// How long one evaluation may take, the script's own awaits included.
const EVALUATE_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// The page could not be read.
#[derive(Debug, thiserror::Error)]
pub enum DevtoolsError {
    #[error("the browser's DevTools port did not answer: {0}")]
    Unreachable(String),
    #[error("the browser has no tab open on {0}")]
    NoTab(String),
    #[error("the page's script failed: {0}")]
    Script(String),
    #[error("the page did not answer within {0:?}")]
    TimedOut(Duration),
}

/// A tab, as Chromium lists it at `/json/list`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub web_socket_debugger_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Devtools {
    http: reqwest::Client,
}

impl Devtools {
    pub const fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Every page the browser on `port` has open.
    ///
    /// # Errors
    /// [`DevtoolsError::Unreachable`] when the `DevTools` port does not answer.
    pub async fn tabs(&self, port: u16) -> Result<Vec<Tab>, DevtoolsError> {
        let url = format!("http://{}:{DEVTOOLS_PORT}/json/list", host_name(port));
        let tabs: Vec<Tab> = self
            .http
            .get(url)
            .header(HOST, "localhost")
            .timeout(CALL_TIMEOUT)
            .send()
            .await
            .map_err(|error| DevtoolsError::Unreachable(error.without_url().to_string()))?
            .json()
            .await
            .map_err(|error| DevtoolsError::Unreachable(error.without_url().to_string()))?;
        Ok(tabs.into_iter().filter(|tab| tab.kind == "page").collect())
    }

    /// Evaluates `expression` in the first tab whose address starts with one of `origins`, and
    /// answers its value. A promise is awaited, so the script may `fetch` as the page.
    ///
    /// # Errors
    /// [`DevtoolsError`] when no such tab is open, the port does not answer, or the script
    /// throws or times out.
    pub async fn evaluate(
        &self,
        port: u16,
        origins: &[&str],
        expression: &str,
    ) -> Result<Value, DevtoolsError> {
        let tabs = self.tabs(port).await?;
        let tab = tabs
            .iter()
            .find(|tab| origins.iter().any(|origin| tab.url.starts_with(origin)))
            .ok_or_else(|| DevtoolsError::NoTab(origins.join(" or ")))?;
        let socket_url = forwarded_socket_url(port, tab)?;

        tokio::time::timeout(EVALUATE_TIMEOUT, evaluate_on(&socket_url, expression))
            .await
            .map_err(|_elapsed| DevtoolsError::TimedOut(EVALUATE_TIMEOUT))?
    }
}

/// The tab's WebSocket, at the forwarder's address rather than Chromium's loopback one.
fn forwarded_socket_url(port: u16, tab: &Tab) -> Result<Url, DevtoolsError> {
    let reported = tab
        .web_socket_debugger_url
        .as_deref()
        .and_then(|url| Url::parse(url).ok())
        .ok_or_else(|| DevtoolsError::Unreachable("the tab has no DevTools socket".to_owned()))?;
    let mut url = Url::parse(&format!("ws://{}:{DEVTOOLS_PORT}/", host_name(port)))
        .expect("a browser's DevTools socket URL parses");
    url.set_path(reported.path());
    Ok(url)
}

async fn evaluate_on(socket_url: &Url, expression: &str) -> Result<Value, DevtoolsError> {
    let mut request = socket_url
        .as_str()
        .into_client_request()
        .map_err(|error| DevtoolsError::Unreachable(error.to_string()))?;
    request
        .headers_mut()
        .insert("Host", HeaderValue::from_static("localhost"));
    let (mut socket, _response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|error| DevtoolsError::Unreachable(error.to_string()))?;

    let call = json!({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": {
            "expression": expression,
            "awaitPromise": true,
            "returnByValue": true,
        },
    });
    socket
        .send(Message::text(call.to_string()))
        .await
        .map_err(|error| DevtoolsError::Unreachable(error.to_string()))?;

    while let Some(message) = socket.next().await {
        let message = message.map_err(|error| DevtoolsError::Unreachable(error.to_string()))?;
        let Message::Text(text) = message else {
            continue;
        };
        let answer: Value = serde_json::from_str(text.as_str())
            .map_err(|error| DevtoolsError::Unreachable(error.to_string()))?;
        // Events the page sends meanwhile carry no id.
        if answer["id"] != 1 {
            continue;
        }
        let _ = socket.close(None).await;
        return evaluation_result(&answer);
    }
    Err(DevtoolsError::Unreachable(
        "the page closed its DevTools socket".to_owned(),
    ))
}

/// The value `Runtime.evaluate` answered, or why the script failed.
fn evaluation_result(answer: &Value) -> Result<Value, DevtoolsError> {
    if let Some(error) = answer.get("error") {
        return Err(DevtoolsError::Script(
            error["message"].as_str().unwrap_or("unknown").to_owned(),
        ));
    }
    let result = &answer["result"];
    if let Some(exception) = result.get("exceptionDetails") {
        let description = exception["exception"]["description"]
            .as_str()
            .or_else(|| exception["text"].as_str())
            .unwrap_or("the script threw");
        return Err(DevtoolsError::Script(description.to_owned()));
    }
    Ok(result["result"]["value"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_is_dialed_at_the_forwarder_not_chromiums_loopback() {
        let tab = Tab {
            kind: "page".to_owned(),
            url: "https://outlook.office.com/mail/".to_owned(),
            web_socket_debugger_url: Some("ws://127.0.0.1:9222/devtools/page/7E3C0A2B".to_owned()),
        };
        assert_eq!(
            forwarded_socket_url(59001, &tab)
                .expect("a socket")
                .as_str(),
            "ws://elysium-browser-59001:9223/devtools/page/7E3C0A2B"
        );
    }

    /// Against a live browser: `ELYSIUM_DEVTOOLS_SOCKET` names a page's socket through the
    /// forwarder, such as `ws://172.22.0.2:9223/devtools/page/<id>`. Proves the handshake with
    /// `Host: localhost` and an evaluation that awaits a promise.
    #[tokio::test]
    #[ignore = "needs a running browser; see the doc comment"]
    async fn a_live_page_evaluates_through_the_forwarder() {
        let socket = std::env::var("ELYSIUM_DEVTOOLS_SOCKET").expect("ELYSIUM_DEVTOOLS_SOCKET is set");
        let url = Url::parse(&socket).expect("a socket URL");
        let value = evaluate_on(&url, "Promise.resolve({ title: document.title, sum: 1 + 2 })")
            .await
            .expect("the page answers");
        assert_eq!(value["sum"], 3);
        assert!(value["title"].is_string());
    }

    #[test]
    fn a_returned_value_is_answered_and_a_throw_is_an_error() {
        let value = json!({ "id": 1, "result": { "result": { "type": "object", "value": { "ok": true } } } });
        assert_eq!(
            evaluation_result(&value).expect("a value"),
            json!({ "ok": true })
        );

        let thrown = json!({
            "id": 1,
            "result": {
                "result": { "type": "object" },
                "exceptionDetails": { "text": "Uncaught", "exception": { "description": "TypeError: boom" } },
            },
        });
        assert!(matches!(
            evaluation_result(&thrown),
            Err(DevtoolsError::Script(message)) if message == "TypeError: boom"
        ));
    }
}
