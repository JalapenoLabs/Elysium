// Copyright © 2026 Jalapeno Labs

//! Ory Kratos's public and admin APIs, reached through one [`Kratos`] handle.
//!
//! The public API answers whose session a cookie is, and ends a person's own sessions, asked
//! with their cookie as their browser would ask; the admin API manages identities and sessions
//! for admins. Both are on the compose network, and browsers never reach the admin API. Kratos is
//! the source of truth for everything it stores: this client only asks and tells, and caches
//! nothing (`super::sessions` caches what it answers).
//!
//! Every call that ends a session is visible to `crate::auth` alone, so the rest of the API ends
//! one only through `super::sessions`, which forgets what it cached in the same step.

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::{HeaderValue, SET_COOKIE};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

/// How long one call may take. Every call is a single row lookup or write on Kratos's side.
/// `super::revocable_cache::REVOCATION_GRACE` outlasts it.
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How many identities one listing page holds. Kratos's admin API pages its listings; a
/// workspace is a few dozen people, so this is one page in practice, well under its limit.
const IDENTITY_PAGE_SIZE: &str = "500";

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

/// How many factors a session was signed in with. Declared weakest first, so comparisons read
/// as assurance: `level < AssuranceLevel::Aal2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

/// What a person's identity stores: `kratos/identity-v2.schema.json`, or the legacy
/// `kratos/identity.schema.json` until `super::identity_schemas` moves the identity on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Traits {
    pub email: String,
    pub name: PersonName,
}

/// A person's name, as either identity schema stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PersonName {
    /// `person-v2`: first and last name apart. The last name may be empty, for people who have
    /// one name.
    Split {
        first: String,
        #[serde(default)]
        last: String,
    },
    /// The legacy `person` schema's one string, read until the identity is moved on.
    Whole(String),
}

impl PersonName {
    /// The name the workspace shows: the first name, then the last name if there is one.
    pub fn full_name(&self) -> String {
        match self {
            Self::Split { first, last } => {
                let first = first.trim();
                let last = last.trim();
                if last.is_empty() {
                    return first.to_owned();
                }
                format!("{first} {last}")
            }
            Self::Whole(name) => name.trim().to_owned(),
        }
    }
}

/// An identity's schema and traits, as the admin API lists them.
#[derive(Debug, Clone, Deserialize)]
pub struct ListedIdentity {
    pub id: Uuid,
    pub schema_id: String,
    pub traits: Traits,
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
        let response = self
            .as_person(Method::GET, "sessions/whoami", cookie)
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

    /// Signs the session `cookie` holds out through Kratos's logout flow, as the browser would,
    /// and answers the `Set-Cookie` headers that clear the cookie, for the browser. A session
    /// still waiting on its second factor signs out too.
    ///
    /// # Errors
    /// Returns [`KratosError::NoSession`] when the cookie holds no session, and
    /// [`KratosError::Refused`] when Kratos cannot be reached or answers otherwise.
    pub(super) async fn log_out(&self, cookie: &str) -> Result<Vec<HeaderValue>, KratosError> {
        /// The part of a logout flow the API needs: the token that ends the session.
        #[derive(Deserialize)]
        struct LogoutFlow {
            logout_token: String,
        }

        let flow: LogoutFlow =
            answer(self.as_person(Method::GET, "self-service/logout/browser", cookie)).await?;
        let response = self
            .as_person(Method::GET, "self-service/logout", cookie)
            .query(&[("token", flow.logout_token)])
            .send()
            .await
            .map_err(|error| KratosError::Refused(error.without_url().to_string()))?;
        match response.status() {
            StatusCode::NO_CONTENT => Ok(response
                .headers()
                .get_all(SET_COOKIE)
                .iter()
                .cloned()
                .collect()),
            StatusCode::UNAUTHORIZED => Err(KratosError::NoSession),
            status => {
                let body = response.text().await.unwrap_or_default();
                Err(KratosError::Refused(format!(
                    "logout answered {status}: {body}"
                )))
            }
        }
    }

    /// Ends `id`, one of the person's other sessions, asked with the `cookie` of the one they
    /// are using. Kratos checks the session is theirs, and refuses to end the one asking.
    ///
    /// # Errors
    /// Returns [`KratosError::NotFound`] for a session that is not theirs, and
    /// [`KratosError::NoSession`] when the cookie holds no session.
    pub(super) async fn revoke_own_session(
        &self,
        cookie: &str,
        id: Uuid,
    ) -> Result<(), KratosError> {
        answer::<Value>(self.as_person(Method::DELETE, &format!("sessions/{id}"), cookie))
            .await
            .map(drop)
    }

    /// Ends every session of the person but the one `cookie` holds.
    ///
    /// # Errors
    /// Returns [`KratosError::NoSession`] when the cookie holds no session.
    pub(super) async fn revoke_own_other_sessions(&self, cookie: &str) -> Result<(), KratosError> {
        answer::<Value>(self.as_person(Method::DELETE, "sessions", cookie))
            .await
            .map(drop)
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

    /// One identity with the kinds of credential it holds. Only this call reports them: a
    /// listing leaves them out unless asked to include each kind's secrets as well.
    ///
    /// # Errors
    /// Returns [`KratosError::NotFound`] for an unknown identity.
    pub async fn identity(&self, id: Uuid) -> Result<AdminIdentity, KratosError> {
        self.admin_call(Method::GET, &format!("admin/identities/{id}"), None)
            .await
    }

    /// Every identity, with its schema and traits, read page by page.
    ///
    /// # Errors
    /// Returns [`KratosError::Refused`] when Kratos cannot be reached or answers otherwise.
    pub async fn identities(&self) -> Result<Vec<ListedIdentity>, KratosError> {
        let mut identities = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = join(&self.admin, "admin/identities");
            url.query_pairs_mut()
                .append_pair("page_size", IDENTITY_PAGE_SIZE);
            if let Some(token) = &page_token {
                url.query_pairs_mut().append_pair("page_token", token);
            }
            let response = self
                .http
                .get(url)
                .header(reqwest::header::ACCEPT, "application/json")
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await
                .map_err(|error| KratosError::Refused(error.without_url().to_string()))?;
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(KratosError::Refused(format!("{status}: {body}")));
            }

            let next = response
                .headers()
                .get(reqwest::header::LINK)
                .and_then(|link| link.to_str().ok())
                .and_then(next_page_token);
            let page: Vec<ListedIdentity> = read(response).await?;
            identities.extend(page);
            match next {
                Some(token) => page_token = Some(token),
                None => return Ok(identities),
            }
        }
    }

    /// Applies a JSON Patch to an identity, which Kratos then validates against the schema the
    /// patched identity names. Kratos refuses to patch credentials, so they are never touched.
    ///
    /// # Errors
    /// Returns [`KratosError::NotFound`] for an unknown identity, and
    /// [`KratosError::Refused`] for a patch Kratos refuses or traits its schema does not allow.
    pub async fn patch_identity(&self, id: Uuid, operations: Value) -> Result<(), KratosError> {
        self.admin_call::<Value>(
            Method::PATCH,
            &format!("admin/identities/{id}"),
            Some(operations),
        )
        .await
        .map(drop)
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
        self.patch_identity(id, patch).await
    }

    /// Signs the identity out everywhere.
    ///
    /// # Errors
    /// Returns [`KratosError::Refused`] when Kratos cannot be reached. An identity with no
    /// sessions is not an error.
    pub(super) async fn revoke_sessions(&self, id: Uuid) -> Result<(), KratosError> {
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
    pub(super) async fn delete_identity(&self, id: Uuid) -> Result<(), KratosError> {
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
        let mut request = self.request(method, join(&self.admin, path));
        if let Some(body) = body {
            request = request.json(&body);
        }
        answer(request).await
    }

    /// A request to the public API carrying the person's session cookie, as their browser
    /// sends it.
    fn as_person(&self, method: Method, path: &str, cookie: &str) -> reqwest::RequestBuilder {
        self.request(method, join(&self.public, path)).header(
            reqwest::header::COOKIE,
            format!("{SESSION_COOKIE}={cookie}"),
        )
    }

    /// A request asking for JSON, which Kratos's self-service routes then answer with, rather
    /// than redirecting as they would a browser.
    fn request(&self, method: Method, url: Url) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT)
    }
}

/// Sends `request` and reads Kratos's answer.
async fn answer<Response: DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<Response, KratosError> {
    let response = request
        .send()
        .await
        .map_err(|error| KratosError::Refused(error.without_url().to_string()))?;

    match response.status() {
        StatusCode::NOT_FOUND => Err(KratosError::NotFound),
        // Only the public API answers it: the cookie holds no session.
        StatusCode::UNAUTHORIZED => Err(KratosError::NoSession),
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

/// The `page_token` of the next page, from a listing's `Link` header, such as
/// `</admin/identities?page_size=500&page_token=abc>; rel="next"`. None on the last page.
fn next_page_token(link: &str) -> Option<String> {
    link.split(',').find_map(|entry| {
        let (target, parameters) = entry.split_once(';')?;
        if !parameters.contains(r#"rel="next""#) {
            return None;
        }
        let target = target.trim().strip_prefix('<')?.strip_suffix('>')?;
        // The target is relative; any base parses its query the same way.
        let url = Url::parse("http://kratos.invalid")
            .ok()?
            .join(target)
            .ok()?;
        url.query_pairs()
            .find(|(key, _)| key == "page_token")
            .map(|(_, value)| value.into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_schemas_names() {
        let split: Traits = serde_json::from_value(
            json!({ "email": "a@example.com", "name": { "first": "Ada", "last": "Lovelace" } }),
        )
        .expect("person-v2 traits");
        assert_eq!(split.name.full_name(), "Ada Lovelace");

        let one_name: Traits = serde_json::from_value(
            json!({ "email": "c@example.com", "name": { "first": " Cher " } }),
        )
        .expect("person-v2 traits without a last name");
        assert_eq!(one_name.name.full_name(), "Cher");

        let legacy: Traits =
            serde_json::from_value(json!({ "email": "g@example.com", "name": "  Grace Hopper " }))
                .expect("legacy person traits");
        assert_eq!(legacy.name.full_name(), "Grace Hopper");
    }

    #[test]
    fn a_cached_session_keeps_its_name_shape() {
        let name = PersonName::Split {
            first: "Ada".to_owned(),
            last: String::new(),
        };
        let round_trip: PersonName =
            serde_json::from_value(serde_json::to_value(&name).expect("serializes"))
                .expect("reads back");
        assert_eq!(round_trip, name);
    }

    #[test]
    fn follows_the_next_page_alone() {
        let first_page = r#"</admin/identities?page_size=500&page_token=00000000-0000-0000-0000-000000000000>; rel="first",</admin/identities?page_size=500&page_token=019a4b2c>; rel="next""#;
        assert_eq!(next_page_token(first_page).as_deref(), Some("019a4b2c"));

        let last_page = r#"</admin/identities?page_size=500&page_token=00000000-0000-0000-0000-000000000000>; rel="first""#;
        assert_eq!(next_page_token(last_page), None);
    }
}
