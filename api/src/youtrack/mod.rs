// Copyright © 2026 Jalapeno Labs

//! YouTrack Cloud's REST API, reached through one [`Youtrack`] handle.
//!
//! Elysium signs in with a person's permanent token (`perm:...`), sent as a bearer token, so it
//! reads exactly what that person can and installs nothing into the organization. YouTrack
//! Server, which lives on a customer's own host, is not supported yet.
//!
//! Every account names its own site, so the host is validated instead of assumed:
//! [`normalize_site_url`] accepts only an `https` origin on `youtrack.cloud` and stores the
//! origin alone, so a stored account can never point Elysium at another host.
//!
//! This module is the only place that calls YouTrack. What a connector reads, its chosen
//! projects, is applied above it, in `crate::connectors::kinds::youtrack` and the reader in
//! [`reader`]. Its writes (opening an issue, commenting, moving an issue's state) serve action
//! item links, through `crate::action_items::links::youtrack`.

pub mod reader;
#[cfg(test)]
pub(crate) mod tests;

use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use reqwest::header::ACCEPT;
use reqwest::{Method, RequestBuilder, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

/// The suffix every YouTrack Cloud site's host ends in.
const CLOUD_HOST_SUFFIX: &str = ".youtrack.cloud";

/// The longest site URL Elysium stores, as the column's own constraint allows.
const SITE_URL_MAX_BYTES: usize = 240;

/// How long one call may take. YouTrack answers a page in well under a second.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How many projects one listing asks for. A YouTrack instance with more than this is far
/// past what a person picks from by hand; the listing says when it was cut short.
const PROJECT_LIMIT: u32 = 500;

/// The fields every issue is read with. The endpoint's defaults are only an id, so each field
/// Elysium reads is named. A custom field's `$type` says what it holds, which is how the state
/// field is told from the rest whatever a project calls it.
const ISSUE_FIELDS: &str = "id,idReadable,summary,description,created,updated,resolved,\
    reporter(login,fullName),project(id,shortName),\
    customFields($type,name,value(login,fullName,name))";

/// The fields a project's custom fields are read with: enough to find its state field and the
/// values it may take.
const PROJECT_FIELD_FIELDS: &str = "$type,field(name),bundle(values(name,isResolved,archived))";

/// The `$type` YouTrack gives a project's state field, and an issue's value of it. A project
/// names its state field as it likes (`State`, or `Stage`), so the type is what is matched.
const STATE_PROJECT_FIELD: &str = "StateProjectCustomField";
const STATE_ISSUE_FIELD: &str = "StateIssueCustomField";

/// The fields every comment is read with.
const COMMENT_FIELDS: &str = "id,text,created,deleted,author(login,fullName)";

/// The custom field YouTrack keeps an issue's assignee in, in every default project template.
const ASSIGNEE_FIELD: &str = "Assignee";

/// The custom field YouTrack keeps an issue's priority in, in every default project template.
const PRIORITY_FIELD: &str = "Priority";

/// A site to call, signed in as its account.
#[derive(Debug, Clone, Copy)]
pub struct Site<'site> {
    /// The site's origin, as [`normalize_site_url`] answered it.
    pub url: &'site str,
    pub token: &'site SecretString,
}

/// The person a token belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub login: String,
    pub full_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// YouTrack's database id, such as `0-1`, which never changes.
    pub id: String,
    /// The key issues are numbered under, such as `JL`.
    pub short_name: String,
    pub name: String,
}

/// The projects a token reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projects {
    pub items: Vec<Project>,
    /// The instance held more than one listing reads.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// YouTrack's database id, such as `2-41`.
    pub id: String,
    /// What a person reads, such as `JL-12`.
    pub key: String,
    pub summary: String,
    pub description: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub resolved: bool,
    pub reporter: Option<User>,
    pub assignee: Option<User>,
    pub project_id: String,
    /// The project's short name, such as `JL`.
    pub project_short_name: String,
    /// The value of the issue's state field, such as `In Progress`, when it has one.
    pub state: Option<String>,
    /// The value of its `Priority` field, such as `Major`, when it has one.
    pub priority: Option<String>,
}

/// A project's state field: what it is called, and the values it may take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateField {
    /// The field's name in the project, such as `State`.
    pub name: String,
    /// Its values in the project's order, archived ones left out.
    pub values: Vec<StateValue>,
}

/// One value a state field may take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateValue {
    pub name: String,
    /// Whether an issue in this state counts as resolved, as YouTrack decides it.
    pub is_resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub text: String,
    pub created: DateTime<Utc>,
    pub author: Option<User>,
}

#[derive(Debug, thiserror::Error)]
pub enum YoutrackError {
    /// The site refused the token.
    #[error("{0}")]
    Unauthorized(&'static str),
    /// What was asked for does not exist, or the token may not see it.
    #[error("YouTrack has no such issue, or the token may not read it")]
    NotFound,
    /// The site could not be reached, or answered something other than what was asked for.
    #[error("{0}")]
    Refused(String),
}

/// The site a person typed, as the origin Elysium stores, or why it is not a YouTrack Cloud site.
///
/// A site is `https://<name>.youtrack.cloud` and nothing more: no path, port, query, or
/// credentials, the name in letters, digits, and hyphens. That is what keeps a stored account
/// from pointing Elysium at any other host.
///
/// # Errors
/// Advice for the person, naming what is wrong.
pub fn normalize_site_url(value: &str) -> Result<String, &'static str> {
    const ADVICE: &str =
        "a YouTrack site is its address alone, such as https://your-site.youtrack.cloud";

    let value = value.trim().trim_end_matches('/');
    if value.len() > SITE_URL_MAX_BYTES {
        return Err("the site URL is too long to be a YouTrack Cloud site");
    }
    let Ok(url) = Url::parse(value) else {
        return Err(ADVICE);
    };
    if url.scheme() != "https" {
        return Err("a YouTrack Cloud site is reached over https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("a YouTrack site URL carries no username or password");
    }
    if url.port().is_some() {
        return Err("a YouTrack Cloud site is reached on the default port");
    }
    if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
        return Err(ADVICE);
    }
    let Some(host) = url.host_str() else {
        return Err(ADVICE);
    };
    let Some(site) = host
        .strip_suffix(CLOUD_HOST_SUFFIX)
        .filter(|site| !site.is_empty())
    else {
        return Err("Elysium reaches YouTrack Cloud only, so a site ends in .youtrack.cloud");
    };
    // `url` has already lowercased the host and punycoded anything that was not ASCII. A
    // YouTrack Cloud site is one label.
    let named = site.starts_with(|first: char| first.is_ascii_alphanumeric())
        && site
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-');
    if !named {
        return Err(
            "a YouTrack Cloud site is named in letters, digits, and hyphens, and starts with a \
             letter or a digit",
        );
    }

    Ok(format!("https://{host}"))
}

/// The address a person opens an issue at.
pub fn issue_url(site_url: &str, key: &str) -> String {
    format!("{site_url}/issue/{key}")
}

/// YouTrack's API client. Cheap to clone; clones share one HTTP client.
#[derive(Debug, Clone)]
pub struct Youtrack {
    http: reqwest::Client,
    /// Stands in for every site's origin, so tests can point the client at a local fake.
    /// Production code has no way to set it.
    #[cfg(test)]
    test_origin: Option<String>,
}

impl Youtrack {
    pub const fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            #[cfg(test)]
            test_origin: None,
        }
    }

    /// A client that sends every site's requests to `origin`, such as a local fake.
    #[cfg(test)]
    pub const fn with_test_origin(http: reqwest::Client, origin: String) -> Self {
        Self {
            http,
            test_origin: Some(origin),
        }
    }

    /// Who the token belongs to.
    ///
    /// # Errors
    /// [`YoutrackError::Unauthorized`] when the site refuses the token, and
    /// [`YoutrackError::Refused`] when it cannot be reached.
    pub async fn whoami(&self, site: &Site<'_>) -> Result<User, YoutrackError> {
        let body: UserBody = self
            .get(site, "/api/users/me", &[("fields", "login,fullName")])
            .await?;
        Ok(body.into_user())
    }

    /// The projects the token reaches, archived ones left out.
    ///
    /// # Errors
    /// As [`Youtrack::whoami`].
    pub async fn list_projects(&self, site: &Site<'_>) -> Result<Projects, YoutrackError> {
        // One past the limit, so a full listing can be told from a cut one.
        let top = (PROJECT_LIMIT + 1).to_string();
        let mut bodies: Vec<ProjectBody> = self
            .get(
                site,
                "/api/admin/projects",
                &[("fields", "id,shortName,name,archived"), ("$top", &top)],
            )
            .await?;
        let truncated = bodies.len() > PROJECT_LIMIT as usize;
        bodies.truncate(PROJECT_LIMIT as usize);
        Ok(Projects {
            items: bodies
                .into_iter()
                .filter(|project| !project.archived)
                .map(|project| Project {
                    id: project.id,
                    short_name: project.short_name,
                    name: project.name.unwrap_or_default(),
                })
                .collect(),
            truncated,
        })
    }

    /// Issues matching `query`, most recently updated first, `top` of them from `skip` on.
    ///
    /// # Errors
    /// As [`Youtrack::whoami`]; a query YouTrack cannot parse is [`YoutrackError::Refused`].
    pub async fn search(
        &self,
        site: &Site<'_>,
        query: &str,
        skip: u32,
        top: u32,
    ) -> Result<Vec<Issue>, YoutrackError> {
        let query = format!("{query} sort by: updated desc");
        let skip = skip.to_string();
        let top = top.to_string();
        let bodies: Vec<IssueBody> = self
            .get(
                site,
                "/api/issues",
                &[
                    ("query", query.trim()),
                    ("fields", ISSUE_FIELDS),
                    ("$skip", &skip),
                    ("$top", &top),
                ],
            )
            .await?;
        Ok(bodies.into_iter().map(IssueBody::into_issue).collect())
    }

    /// An issue's comments, oldest first, deleted ones left out.
    ///
    /// # Errors
    /// [`YoutrackError::NotFound`] for an issue the token may not read; otherwise as
    /// [`Youtrack::whoami`].
    pub async fn comments(
        &self,
        site: &Site<'_>,
        issue_id: &str,
        top: u32,
    ) -> Result<Vec<Comment>, YoutrackError> {
        if !is_database_id(issue_id) {
            return Err(YoutrackError::NotFound);
        }
        let top = top.to_string();
        let bodies: Vec<CommentBody> = self
            .get(
                site,
                &format!("/api/issues/{issue_id}/comments"),
                &[("fields", COMMENT_FIELDS), ("$top", &top)],
            )
            .await?;
        Ok(bodies
            .into_iter()
            .filter(|comment| !comment.deleted)
            .map(|comment| Comment {
                id: comment.id,
                text: comment.text.unwrap_or_default(),
                created: from_millis(comment.created),
                author: comment.author.map(UserBody::into_user),
            })
            .collect())
    }

    /// One issue, named by its database id (`2-41`) or the key a person reads (`JL-12`).
    ///
    /// # Errors
    /// [`YoutrackError::NotFound`] for an issue that does not exist or the token may not read,
    /// and for a reference that is neither; otherwise as [`Youtrack::whoami`].
    pub async fn issue(&self, site: &Site<'_>, reference: &str) -> Result<Issue, YoutrackError> {
        if !is_database_id(reference) && !is_issue_key(reference) {
            return Err(YoutrackError::NotFound);
        }
        let body: IssueBody = self
            .get(
                site,
                &format!("/api/issues/{reference}"),
                &[("fields", ISSUE_FIELDS)],
            )
            .await?;
        Ok(body.into_issue())
    }

    /// Opens an issue in the project with database id `project_id`, as the token's user.
    ///
    /// # Errors
    /// [`YoutrackError::NotFound`] for a project the token cannot reach, and
    /// [`YoutrackError::Refused`] when YouTrack refuses the issue, such as for a user who may
    /// not create issues there; otherwise as [`Youtrack::whoami`].
    pub async fn create_issue(
        &self,
        site: &Site<'_>,
        project_id: &str,
        summary: &str,
        description: &str,
    ) -> Result<Issue, YoutrackError> {
        if !is_database_id(project_id) {
            return Err(YoutrackError::NotFound);
        }
        let request = json!({
            "project": { "id": project_id },
            "summary": summary,
            "description": description,
        });
        let body: IssueBody = self
            .send(
                Method::POST,
                site,
                "/api/issues",
                &[("fields", ISSUE_FIELDS)],
                Some(&request),
            )
            .await?;
        Ok(body.into_issue())
    }

    /// Writes a comment on the issue with database id `issue_id`, as the token's user.
    ///
    /// # Errors
    /// As [`Youtrack::comments`].
    pub async fn add_comment(
        &self,
        site: &Site<'_>,
        issue_id: &str,
        text: &str,
    ) -> Result<(), YoutrackError> {
        if !is_database_id(issue_id) {
            return Err(YoutrackError::NotFound);
        }
        let request = json!({ "text": text });
        let _created: Value = self
            .send(
                Method::POST,
                site,
                &format!("/api/issues/{issue_id}/comments"),
                &[("fields", "id")],
                Some(&request),
            )
            .await?;
        Ok(())
    }

    /// The state field of the project with database id `project_id`, or `None` when the
    /// project has none.
    ///
    /// # Errors
    /// As [`Youtrack::create_issue`].
    pub async fn state_field(
        &self,
        site: &Site<'_>,
        project_id: &str,
    ) -> Result<Option<StateField>, YoutrackError> {
        if !is_database_id(project_id) {
            return Err(YoutrackError::NotFound);
        }
        let fields: Vec<ProjectFieldBody> = self
            .get(
                site,
                &format!("/api/admin/projects/{project_id}/customFields"),
                &[("fields", PROJECT_FIELD_FIELDS)],
            )
            .await?;
        Ok(fields
            .into_iter()
            .find(|field| field.kind == STATE_PROJECT_FIELD)
            .map(|field| StateField {
                name: field.field.name,
                values: field
                    .bundle
                    .map(|bundle| bundle.values)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|value| !value.archived)
                    .map(|value| StateValue {
                        name: value.name,
                        is_resolved: value.is_resolved,
                    })
                    .collect(),
            }))
    }

    /// Moves the issue with database id `issue_id` to `value` of its state field `field`.
    ///
    /// # Errors
    /// [`YoutrackError::Refused`] when YouTrack refuses the move, such as a workflow that
    /// forbids it; otherwise as [`Youtrack::comments`].
    pub async fn set_state(
        &self,
        site: &Site<'_>,
        issue_id: &str,
        field: &str,
        value: &str,
    ) -> Result<(), YoutrackError> {
        if !is_database_id(issue_id) {
            return Err(YoutrackError::NotFound);
        }
        let request = json!({
            "customFields": [{
                "name": field,
                "$type": STATE_ISSUE_FIELD,
                "value": { "name": value },
            }],
        });
        let _updated: Value = self
            .send(
                Method::POST,
                site,
                &format!("/api/issues/{issue_id}"),
                &[("fields", "id")],
                Some(&request),
            )
            .await?;
        Ok(())
    }

    /// One authenticated GET against the account's site, read in full as `Shape`.
    async fn get<Shape: serde::de::DeserializeOwned>(
        &self,
        site: &Site<'_>,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Shape, YoutrackError> {
        self.send(Method::GET, site, path, query, None).await
    }

    /// One authenticated call against the account's site, with an optional JSON body, read in
    /// full as `Shape`.
    async fn send<Shape: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        site: &Site<'_>,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<Shape, YoutrackError> {
        let mut request = self.request(method, site, path).query(query);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|error| {
            YoutrackError::Refused(format!("YouTrack: {}", error.without_url()))
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            YoutrackError::Refused(format!("YouTrack: {}", error.without_url()))
        })?;
        if !status.is_success() {
            return Err(failure(status, &body));
        }
        serde_json::from_str(&body).map_err(|error| {
            YoutrackError::Refused(format!(
                "YouTrack sent an answer that cannot be read: {error}"
            ))
        })
    }

    fn request(&self, method: Method, site: &Site<'_>, path: &str) -> RequestBuilder {
        #[cfg(not(test))]
        let origin = site.url;
        #[cfg(test)]
        let origin = self.test_origin.as_deref().unwrap_or(site.url);

        self.http
            .request(method, format!("{origin}{path}"))
            .bearer_auth(site.token.expose_secret())
            .header(ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT)
    }
}

/// An unsuccessful answer as the error whoever can fix it should see.
fn failure(status: StatusCode, body: &str) -> YoutrackError {
    match status {
        StatusCode::UNAUTHORIZED => YoutrackError::Unauthorized(
            "YouTrack refused the token; check that it was copied whole, is a permanent token \
             for this site, and has not been revoked",
        ),
        StatusCode::FORBIDDEN => YoutrackError::Unauthorized(
            "YouTrack accepted the token but refused to answer; its user may not read this",
        ),
        StatusCode::NOT_FOUND => YoutrackError::NotFound,
        other => {
            let detail: String = serde_json::from_str::<ErrorBody>(body)
                .ok()
                .and_then(|error| error.error_description.or(error.error))
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect();
            YoutrackError::Refused(format!("YouTrack answered {other}: {detail}"))
        }
    }
}

/// Whether `id` is a YouTrack database id, such as `2-41`: two runs of digits and a hyphen. An
/// id that passes holds nothing to encode and cannot reach another resource in a path.
fn is_database_id(id: &str) -> bool {
    id.split_once('-').is_some_and(|(type_id, entity_id)| {
        (1..=10).contains(&type_id.len())
            && (1..=19).contains(&entity_id.len())
            && type_id.chars().all(|digit| digit.is_ascii_digit())
            && entity_id.chars().all(|digit| digit.is_ascii_digit())
    })
}

/// Whether `key` is an issue key a person reads, such as `JL-12`: a project's short name, a
/// hyphen, and a number. A key that passes holds nothing to encode in a path.
pub fn is_issue_key(key: &str) -> bool {
    key.rsplit_once('-').is_some_and(|(project, number)| {
        (1..=50).contains(&project.len())
            && project.starts_with(|first: char| first.is_ascii_alphabetic())
            && project
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
            && (1..=19).contains(&number.len())
            && number.chars().all(|digit| digit.is_ascii_digit())
    })
}

/// A YouTrack timestamp, milliseconds since the epoch, as a UTC moment. One out of range,
/// which YouTrack never sends, reads as the epoch rather than failing the whole answer.
fn from_millis(millis: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(millis)
        .single()
        .unwrap_or(DateTime::UNIX_EPOCH)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserBody {
    #[serde(default)]
    login: String,
    #[serde(default)]
    full_name: Option<String>,
}

impl UserBody {
    fn into_user(self) -> User {
        let full_name = self
            .full_name
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.login.clone());
        User {
            login: self.login,
            full_name,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectBody {
    id: String,
    short_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    archived: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectReference {
    id: String,
    #[serde(default)]
    short_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueBody {
    id: String,
    id_readable: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    created: i64,
    updated: i64,
    #[serde(default)]
    resolved: Option<i64>,
    #[serde(default)]
    reporter: Option<UserBody>,
    project: ProjectReference,
    #[serde(default)]
    custom_fields: Vec<CustomFieldBody>,
}

impl IssueBody {
    fn into_issue(self) -> Issue {
        let mut assignee = None;
        let mut state = None;
        let mut priority = None;
        for field in self.custom_fields {
            let Some(value) = field.value else {
                continue;
            };
            if field.name == ASSIGNEE_FIELD {
                assignee = serde_json::from_value::<UserBody>(value)
                    .ok()
                    .filter(|user| !user.login.is_empty())
                    .map(UserBody::into_user);
            } else if field.kind == STATE_ISSUE_FIELD {
                state = value_name(&value);
            } else if field.name == PRIORITY_FIELD {
                priority = value_name(&value);
            }
        }
        Issue {
            id: self.id,
            key: self.id_readable,
            summary: self.summary.unwrap_or_default(),
            description: self.description.unwrap_or_default(),
            created: from_millis(self.created),
            updated: from_millis(self.updated),
            resolved: self.resolved.is_some(),
            reporter: self.reporter.map(UserBody::into_user),
            assignee,
            project_id: self.project.id,
            project_short_name: self.project.short_name,
            state,
            priority,
        }
    }
}

/// The name an enum or state value carries, such as `{ "name": "Fixed" }`.
fn value_name(value: &serde_json::Value) -> Option<String> {
    value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// A custom field as YouTrack sends it. Its value's shape depends on the field's type (a user,
/// an enum value, a list), so it is read as JSON and only the assignee, the state, and the
/// priority are interpreted.
#[derive(Debug, Deserialize)]
struct CustomFieldBody {
    name: String,
    #[serde(rename = "$type", default)]
    kind: String,
    #[serde(default)]
    value: Option<serde_json::Value>,
}

/// One of a project's custom fields, as its settings list them.
#[derive(Debug, Deserialize)]
struct ProjectFieldBody {
    #[serde(rename = "$type", default)]
    kind: String,
    field: FieldNameBody,
    #[serde(default)]
    bundle: Option<BundleBody>,
}

#[derive(Debug, Deserialize)]
struct FieldNameBody {
    name: String,
}

#[derive(Debug, Deserialize)]
struct BundleBody {
    #[serde(default)]
    values: Vec<BundleValueBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleValueBody {
    name: String,
    #[serde(default)]
    is_resolved: bool,
    #[serde(default)]
    archived: bool,
}

#[derive(Debug, Deserialize)]
struct CommentBody {
    id: String,
    #[serde(default)]
    text: Option<String>,
    created: i64,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    author: Option<UserBody>,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}
