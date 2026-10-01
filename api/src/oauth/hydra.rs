// Copyright © 2026 Jalapeno Labs

//! Ory Hydra's admin API, reached through one [`Hydra`] handle.
//!
//! Hydra is the OAuth 2.1 authorization server MCP clients connect through. It has no sign-in
//! of its own: when a client asks for access, Hydra sends the person to Elysium's
//! `/oauth/login` and `/oauth/consent` pages with a challenge, and the API answers each
//! challenge here, as the person Kratos says is signed in. The API also introspects the access
//! tokens MCP clients present, and lists and revokes people's grants.
//!
//! The admin API is on the compose network alone; browsers and MCP clients never reach it.

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{Level, event};
use url::Url;
use uuid::Uuid;

/// The most clients one listing returns: the largest page Hydra serves. A workspace with more
/// sees the first ones, and the API logs that it cut the list short.
const CLIENT_LIST_LIMIT: usize = 500;

/// How long one call may take. Every call is a single row lookup or write on Hydra's side.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// Hydra refused a call or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum HydraError {
    /// The challenge, client, or grant does not exist, or the challenge was already used.
    #[error("not found")]
    NotFound,
    #[error("Hydra: {0}")]
    Refused(String),
}

/// An OAuth client, as Hydra reports it. Only what Elysium shows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "Hydra's own field names, read straight from its answers"
)]
pub struct Client {
    pub client_id: String,
    #[serde(default)]
    pub client_name: String,
    #[serde(default)]
    pub client_uri: String,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    /// Space-separated, as OAuth writes scopes.
    #[serde(default)]
    pub scope: String,
    /// The audiences tokens issued to it may name. Hydra checks a token's audience against this
    /// when it is refreshed.
    #[serde(default)]
    pub audience: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
}

/// A client asking a signed-in person for access.
#[derive(Debug, Clone, Deserialize)]
pub struct ConsentRequest {
    pub client: Client,
    #[serde(default)]
    pub requested_scope: Vec<String>,
    /// Who signed in: the Elysium user id the login challenge was accepted with.
    #[serde(default)]
    pub subject: String,
    /// The person already consented to this client and these scopes, and asked to be
    /// remembered.
    #[serde(default)]
    pub skip: bool,
}

/// What a person consented to, as Hydra keeps it.
#[derive(Debug, Clone, Deserialize)]
pub struct ConsentSession {
    pub consent_request: ConsentSessionRequest,
    #[serde(default)]
    pub grant_scope: Vec<String>,
    pub handled_at: Option<DateTime<Utc>>,
}

/// The part of a consent session's original request Elysium shows: which client.
#[derive(Debug, Clone, Deserialize)]
pub struct ConsentSessionRequest {
    pub client: Client,
}

/// An access token, as introspection answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Introspection {
    pub active: bool,
    #[serde(default)]
    pub sub: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub aud: Vec<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    /// Seconds since the epoch.
    #[serde(default)]
    pub exp: Option<i64>,
    #[serde(default)]
    pub token_use: Option<String>,
}

/// Why a consent request was refused, as the client is told (RFC 6749 error codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentRefusal {
    /// The person declined: `access_denied`.
    Denied,
    /// The client asked for nothing a person can grant: `invalid_scope`.
    NothingGrantable,
}

/// Where Hydra sends the browser next.
#[derive(Debug, Clone, Deserialize)]
struct RedirectTo {
    redirect_to: String,
}

/// Hydra's admin API. Cloning is cheap: it shares the HTTP client's pool.
#[derive(Debug, Clone)]
pub struct Hydra {
    http: reqwest::Client,
    admin: Url,
}

impl Hydra {
    pub const fn new(http: reqwest::Client, admin: Url) -> Self {
        Self { http, admin }
    }

    /// Confirms the sign-in request behind `challenge` exists and is waiting.
    ///
    /// # Errors
    /// Returns [`HydraError::NotFound`] for an unknown, expired, or used challenge.
    pub async fn login_request(&self, challenge: &str) -> Result<(), HydraError> {
        let url = self.url(
            "admin/oauth2/auth/requests/login",
            "login_challenge",
            challenge,
        );
        self.send::<Value>(Method::GET, url, None).await.map(drop)
    }

    /// Signs `subject` in for the request behind `challenge`, and answers where the browser
    /// goes next. Hydra remembers nothing: whether someone is signed in is Kratos's to say.
    ///
    /// # Errors
    /// Returns [`HydraError::NotFound`] for an unknown, expired, or used challenge.
    pub async fn accept_login(&self, challenge: &str, subject: Uuid) -> Result<String, HydraError> {
        let url = self.url(
            "admin/oauth2/auth/requests/login/accept",
            "login_challenge",
            challenge,
        );
        let body = json!({ "subject": subject.to_string(), "remember": false });
        let answer: RedirectTo = self.send(Method::PUT, url, Some(body)).await?;
        Ok(answer.redirect_to)
    }

    /// The consent request behind `challenge`.
    ///
    /// # Errors
    /// Returns [`HydraError::NotFound`] for an unknown, expired, or used challenge.
    pub async fn consent_request(&self, challenge: &str) -> Result<ConsentRequest, HydraError> {
        let url = self.url(
            "admin/oauth2/auth/requests/consent",
            "consent_challenge",
            challenge,
        );
        self.send(Method::GET, url, None).await
    }

    /// Grants `scopes` for tokens bound to `audience` alone, and answers where the browser goes
    /// next. Remembered, so reconnecting the same client to the same scopes asks no one again;
    /// the person revokes it from Settings.
    ///
    /// # Errors
    /// Returns [`HydraError::NotFound`] for an unknown, expired, or used challenge.
    pub async fn accept_consent(
        &self,
        challenge: &str,
        scopes: &[String],
        audience: &str,
    ) -> Result<String, HydraError> {
        let url = self.url(
            "admin/oauth2/auth/requests/consent/accept",
            "consent_challenge",
            challenge,
        );
        let body = json!({
            "grant_scope": scopes,
            "grant_access_token_audience": [audience],
            "remember": true,
            "remember_for": 0,
        });
        let answer: RedirectTo = self.send(Method::PUT, url, Some(body)).await?;
        Ok(answer.redirect_to)
    }

    /// Lets `client` hold tokens for `audience`, if it may not already.
    ///
    /// Clients register themselves without naming one, and Hydra refuses to refresh a token whose
    /// audience the client may not hold, so the audience consent grants is added to the client
    /// as well.
    ///
    /// # Errors
    /// Returns [`HydraError::NotFound`] for an unknown client.
    pub async fn allow_audience(&self, client: &Client, audience: &str) -> Result<(), HydraError> {
        if client.audience.iter().any(|allowed| allowed == audience) {
            return Ok(());
        }
        let operation = if client.audience.is_empty() {
            json!({ "op": "replace", "path": "/audience", "value": [audience] })
        } else {
            json!({ "op": "add", "path": "/audience/-", "value": audience })
        };
        let url = self.client_url(&client.client_id);
        self.send::<Value>(Method::PATCH, url, Some(json!([operation])))
            .await
            .map(drop)
    }

    /// Refuses the consent request behind `challenge` with `refusal`, and answers where the
    /// browser goes next: back to the client, told why.
    ///
    /// # Errors
    /// Returns [`HydraError::NotFound`] for an unknown, expired, or used challenge.
    pub async fn reject_consent(
        &self,
        challenge: &str,
        refusal: ConsentRefusal,
    ) -> Result<String, HydraError> {
        let url = self.url(
            "admin/oauth2/auth/requests/consent/reject",
            "consent_challenge",
            challenge,
        );
        let (error, description) = match refusal {
            ConsentRefusal::Denied => (
                "access_denied",
                "The person declined to connect this application.",
            ),
            ConsentRefusal::NothingGrantable => (
                "invalid_scope",
                "None of the requested scopes can be granted. Ask for workspace:read.",
            ),
        };
        let body = json!({ "error": error, "error_description": description });
        let answer: RedirectTo = self.send(Method::PUT, url, Some(body)).await?;
        Ok(answer.redirect_to)
    }

    /// What `token` grants, or that it grants nothing.
    ///
    /// # Errors
    /// Returns [`HydraError::Refused`] when Hydra cannot be reached.
    pub async fn introspect(&self, token: &str) -> Result<Introspection, HydraError> {
        let response = self
            .http
            .post(
                self.admin
                    .join("admin/oauth2/introspect")
                    .expect("a relative path joins"),
            )
            .form(&[("token", token)])
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| HydraError::Refused(error.without_url().to_string()))?;
        read(response).await
    }

    /// Every client `subject` has consented to, with what each was granted.
    ///
    /// # Errors
    /// Returns [`HydraError::Refused`] when Hydra cannot be reached.
    pub async fn consent_sessions(&self, subject: Uuid) -> Result<Vec<ConsentSession>, HydraError> {
        let url = self.url(
            "admin/oauth2/auth/sessions/consent",
            "subject",
            &subject.to_string(),
        );
        self.send(Method::GET, url, None).await
    }

    /// Revokes what `subject` granted `client_id`, or every client for `None`, with every token
    /// issued under it.
    ///
    /// # Errors
    /// Returns [`HydraError::Refused`] when Hydra cannot be reached. Nothing to revoke is not an
    /// error.
    pub async fn revoke_consent(
        &self,
        subject: Uuid,
        client_id: Option<&str>,
    ) -> Result<(), HydraError> {
        let mut url = self.url(
            "admin/oauth2/auth/sessions/consent",
            "subject",
            &subject.to_string(),
        );
        match client_id {
            Some(client) => url.query_pairs_mut().append_pair("client", client),
            None => url.query_pairs_mut().append_pair("all", "true"),
        };
        match self.send::<Value>(Method::DELETE, url, None).await {
            Ok(_) | Err(HydraError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Registered clients, up to [`CLIENT_LIST_LIMIT`], in the order Hydra keeps them.
    ///
    /// # Errors
    /// Returns [`HydraError::Refused`] when Hydra cannot be reached.
    pub async fn clients(&self) -> Result<Vec<Client>, HydraError> {
        let url = self.url("admin/clients", "page_size", &CLIENT_LIST_LIMIT.to_string());
        let clients: Vec<Client> = self.send(Method::GET, url, None).await?;
        if clients.len() >= CLIENT_LIST_LIMIT {
            event!(
                name: "oauth.clients.truncated",
                Level::WARN,
                oauth.clients.limit = CLIENT_LIST_LIMIT,
                "listed only the first {{oauth.clients.limit}} OAuth clients",
            );
        }
        Ok(clients)
    }

    /// Deletes a client, with every grant and token issued to it.
    ///
    /// # Errors
    /// Returns [`HydraError::NotFound`] for an unknown client.
    pub async fn delete_client(&self, client_id: &str) -> Result<(), HydraError> {
        self.send::<Value>(Method::DELETE, self.client_url(client_id), None)
            .await
            .map(drop)
    }

    /// One client's URL under the admin API, its id encoded as a single path segment.
    fn client_url(&self, client_id: &str) -> Url {
        let mut url = self
            .admin
            .join("admin/clients/")
            .expect("a relative path joins");
        url.path_segments_mut()
            .expect("an http URL has path segments")
            .pop_if_empty()
            .push(client_id);
        url
    }

    /// `path` under the admin API, with one query parameter.
    fn url(&self, path: &str, key: &str, value: &str) -> Url {
        let mut url = self
            .admin
            .join(path)
            .expect("a relative path joins onto an origin");
        url.query_pairs_mut().append_pair(key, value);
        url
    }

    async fn send<Response: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<Value>,
    ) -> Result<Response, HydraError> {
        let mut request = self
            .http
            .request(method, url)
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| HydraError::Refused(error.without_url().to_string()))?;
        read(response).await
    }
}

async fn read<Response: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<Response, HydraError> {
    match response.status() {
        // A used or expired challenge answers Gone; to the caller it is as gone as an unknown one.
        StatusCode::NOT_FOUND | StatusCode::GONE => Err(HydraError::NotFound),
        StatusCode::NO_CONTENT => serde_json::from_value(Value::Null)
            .map_err(|error| HydraError::Refused(format!("unexpected empty answer: {error}"))),
        status if status.is_success() => response
            .json()
            .await
            .map_err(|error| HydraError::Refused(format!("unreadable answer: {error}"))),
        status => {
            let body = response.text().await.unwrap_or_default();
            Err(HydraError::Refused(format!("{status}: {body}")))
        }
    }
}
