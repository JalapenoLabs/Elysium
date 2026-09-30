// Copyright © 2026 Jalapeno Labs

//! Ory Kratos's public and admin APIs, reached through one [`Kratos`] handle.
//!
//! The public API answers whose session a cookie is; the admin API manages identities and
//! sessions for admins. Both are on the compose network, and browsers never reach the admin
//! API. Kratos is the source of truth for everything it stores: this client only asks and
//! tells, and caches nothing (`super::sessions` caches what it answers).

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

/// How long one call may take. Every call is a single row lookup or write on Kratos's side.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The session cookie Kratos sets, named in `kratos/kratos.yml`.
pub const SESSION_COOKIE: &str = "elysium_session";

/// Kratos refused a call or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum KratosError {
    /// The cookie names no active session.
    #[error("no active session")]
    NoSession,
    /// The session is signed in with one factor, and the identity has a second one it must
    /// use too (`required_aal: highest_available`).
    #[error("the session must be completed with a second factor")]
    SecondFactorRequired,
    /// The identity does not exist.
    #[error("identity not found")]
    NotFound,
    #[error("Kratos: {0}")]
    Refused(String),
}

/// An active session, as `whoami` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub authenticator_assurance_level: AssuranceLevel,
    pub identity: Identity,
}

/// How many factors a session was signed in with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssuranceLevel {
    Aal0,
    Aal1,
    Aal2,
    Aal3,
}

/// An identity, as the public API reports it inside a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub id: Uuid,
    pub traits: Traits,
}

/// What `kratos/identity.schema.json` stores for a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Traits {
    pub email: String,
    pub name: String,
}

/// An identity with the kinds of credential it holds, as the admin API reports it.
#[derive(Debug, Clone, Deserialize)]
pub struct AdminIdentity {
    pub id: Uuid,
    /// Keyed by credential type: `password`, `passkey`, `totp`, `lookup_secret`. Kratos
    /// omits the map for an identity with none.
    #[serde(default)]
    pub credentials: std::collections::HashMap<String, Value>,
}

impl AdminIdentity {
    /// The credential types the identity holds, sorted, such as `["password", "totp"]`.
    pub fn methods(&self) -> Vec<String> {
        let mut methods: Vec<String> = self.credentials.keys().cloned().collect();
        methods.sort_unstable();
        methods
    }
}

/// Whether an identity may sign in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityState {
    Active,
    /// Kratos refuses to sign it in.
    Inactive,
}

/// A one-time link that signs its holder in and opens the page to set a new password.
#[derive(Debug, Clone, Deserialize)]
pub struct RecoveryLink {
    pub recovery_link: String,
    pub expires_at: DateTime<Utc>,
}

/// The two Kratos APIs. Cloning is cheap: it shares the HTTP client's pool.
#[derive(Debug, Clone)]
pub struct Kratos {
    http: reqwest::Client,
    public: Url,
    admin: Url,
}

impl Kratos {
    pub const fn new(http: reqwest::Client, public: Url, admin: Url) -> Self {
        Self {
            http,
            public,
            admin,
        }
    }

    /// The session `cookie` holds: the value of [`SESSION_COOKIE`].
    ///
    /// # Errors
    /// Returns [`KratosError::NoSession`] for a missing, expired, or revoked session,
    /// [`KratosError::SecondFactorRequired`] for one waiting on a second factor, and
    /// [`KratosError::Refused`] when Kratos cannot be reached.
    pub async fn whoami(&self, cookie: &str) -> Result<Session, KratosError> {
        let url = join(&self.public, "sessions/whoami");
        let response = self
            .http
            .get(url)
            .header(
                reqwest::header::COOKIE,
                format!("{SESSION_COOKIE}={cookie}"),
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|error| KratosError::Refused(error.without_url().to_string()))?;

        match response.status() {
            StatusCode::OK => read(response).await,
            StatusCode::UNAUTHORIZED => Err(KratosError::NoSession),
            StatusCode::FORBIDDEN => {
                let body: Value = response.json().await.unwrap_or(Value::Null);
                if body["error"]["id"] == "session_aal2_required" {
                    return Err(KratosError::SecondFactorRequired);
                }
                Err(KratosError::Refused(format!("whoami refused: {body}")))
            }
            status => Err(KratosError::Refused(format!("whoami answered {status}"))),
        }
    }

    /// Pushes a session's expiry out to a full lifespan from now. Kratos ignores a call made
    /// before `session.earliest_possible_extend`.
    ///
    /// # Errors
    /// Returns [`KratosError::NotFound`] for an unknown session.
    pub async fn extend_session(&self, id: Uuid) -> Result<(), KratosError> {
        let path = format!("admin/sessions/{id}/extend");
        self.admin_call::<Value>(Method::PATCH, &path, None)
            .await
            .map(drop)
    }

    /// The identities `ids` names, with the kinds of credential each holds. Unknown ids are
    /// left out.
    ///
    /// # Errors
    /// Returns [`KratosError::Refused`] when Kratos cannot be reached.
    pub async fn identities(&self, ids: &[Uuid]) -> Result<Vec<AdminIdentity>, KratosError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut url = join(&self.admin, "admin/identities");
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("page_size", &ids.len().to_string());
            for id in ids {
                query.append_pair("ids", &id.to_string());
            }
        }
        self.send(Method::GET, url, None).await
    }

    /// Lets an identity sign in, or stops it.
    ///
    /// # Errors
    /// Returns [`KratosError::NotFound`] for an unknown identity.
    pub async fn set_state(&self, id: Uuid, state: IdentityState) -> Result<(), KratosError> {
        let value = match state {
            IdentityState::Active => "active",
            IdentityState::Inactive => "inactive",
        };
        let patch = json!([{ "op": "replace", "path": "/state", "value": value }]);
        self.admin_call::<Value>(
            Method::PATCH,
            &format!("admin/identities/{id}"),
            Some(patch),
        )
        .await
        .map(drop)
    }

    /// Signs the identity out everywhere.
    ///
    /// # Errors
    /// Returns [`KratosError::Refused`] when Kratos cannot be reached. An identity with no
    /// sessions is not an error.
    pub async fn revoke_sessions(&self, id: Uuid) -> Result<(), KratosError> {
        let path = format!("admin/identities/{id}/sessions");
        match self.admin_call::<Value>(Method::DELETE, &path, None).await {
            Ok(_) | Err(KratosError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Removes every credential of `kind` (`totp`, `lookup_secret`) from the identity.
    ///
    /// # Errors
    /// Returns [`KratosError::Refused`] when Kratos cannot be reached. An identity without
    /// that credential is not an error.
    pub async fn delete_credentials(&self, id: Uuid, kind: &str) -> Result<(), KratosError> {
        let path = format!("admin/identities/{id}/credentials/{kind}");
        match self.admin_call::<Value>(Method::DELETE, &path, None).await {
            Ok(_) | Err(KratosError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Deletes the identity, its credentials, and its sessions.
    ///
    /// # Errors
    /// Returns [`KratosError::Refused`] when Kratos cannot be reached. An identity that is
    /// already gone is not an error.
    pub async fn delete_identity(&self, id: Uuid) -> Result<(), KratosError> {
        match self
            .admin_call::<Value>(Method::DELETE, &format!("admin/identities/{id}"), None)
            .await
        {
            Ok(_) | Err(KratosError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// A one-time recovery link for the identity, valid for `expires_in`.
    ///
    /// # Errors
    /// Returns [`KratosError::NotFound`] for an unknown identity.
    pub async fn recovery_link(
        &self,
        id: Uuid,
        expires_in: Duration,
    ) -> Result<RecoveryLink, KratosError> {
        let body = json!({
            "identity_id": id,
            "expires_in": format!("{}m", expires_in.as_secs() / 60),
        });
        self.admin_call(Method::POST, "admin/recovery/link", Some(body))
            .await
    }

    async fn admin_call<Response: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Response, KratosError> {
        self.send(method, join(&self.admin, path), body).await
    }

    async fn send<Response: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<Value>,
    ) -> Result<Response, KratosError> {
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
            .map_err(|error| KratosError::Refused(error.without_url().to_string()))?;

        match response.status() {
            StatusCode::NOT_FOUND => Err(KratosError::NotFound),
            // Deletes answer with no body; reading it as JSON `null` keeps one code path.
            StatusCode::NO_CONTENT => serde_json::from_value(Value::Null)
                .map_err(|error| KratosError::Refused(format!("unexpected empty answer: {error}"))),
            status if status.is_success() => read(response).await,
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(KratosError::Refused(format!("{status}: {body}")))
            }
        }
    }
}

async fn read<Response: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<Response, KratosError> {
    response
        .json()
        .await
        .map_err(|error| KratosError::Refused(format!("unreadable answer: {error}")))
}

/// `base` with `path` appended. Both Kratos URLs are origins, so this never drops a segment.
fn join(base: &Url, path: &str) -> Url {
    base.join(path)
        .expect("a relative path joins onto an origin")
}
