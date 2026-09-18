// Copyright © 2026 Jalapeno Labs

//! Jira Cloud's REST API, reached through one [`Jira`] handle.
//!
//! Elysium authenticates with HTTP basic auth: an Atlassian account's email address as the
//! username, and an API token as the password. Jira Data Center, which lives on a
//! customer's own host and authenticates differently, is not supported.
//!
//! Unlike GitHub, whose host is fixed in code, every credential names its own site. The
//! host is therefore validated instead of assumed: [`normalize_site_url`] accepts only an
//! `https` origin on `atlassian.net` and stores the origin alone, so a stored credential
//! can never point Elysium at another host.
//!
//! Rich text is [Atlassian Document Format](adf), not a string, in both directions.
//!
//! This module is the only place that calls Jira. What a credential is allowed to touch is
//! enforced above it, in `routes/v1/jira_credentials/allowlist.rs`, so that one check
//! covers every read and every write.

pub mod adf;
#[cfg(test)]
pub(crate) mod tests;

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::{Method, RequestBuilder, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{Level, event};
use url::Url;

use self::adf::RichText;

/// The suffix every Jira Cloud site's host ends in.
const CLOUD_HOST_SUFFIX: &str = ".atlassian.net";

/// The longest site URL Elysium stores. A site is a host and nothing else, so this is far
/// more than any real one needs and keeps a row from carrying an arbitrary string.
const SITE_URL_MAX_BYTES: usize = 255;

/// How many entries one page of a listing asks for. Fifty is the most Jira sends for both
/// project search and board search, so asking for more only wastes the ceiling below.
const PAGE_SIZE: u32 = 50;

/// How many pages of projects or boards a listing follows, so 500 of each at most.
///
/// Pages are fetched one after another and the whole listing answers one request under the
/// API's 30 second handler deadline. A site past the cap is told the listing was cut short;
/// widening that is the searchable picker on the roadmap in `docs/jira.md`.
const PAGE_LIMIT: u32 = 10;

/// How long one call may take. Jira answers a page in well under a second.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The fields a search asks for. The endpoint's default is not relied on, and `project` is
/// always among them so the allowlist can be checked against the answer rather than the
/// key that was asked for.
const SEARCH_FIELDS: &[&str] = &[
    "summary",
    "status",
    "issuetype",
    "priority",
    "assignee",
    "reporter",
    "labels",
    "created",
    "updated",
    "project",
];

/// What one issue asks for: everything a search asks for, and the two largest fields a
/// listing has no use for.
const ISSUE_FIELDS: &[&str] = &["description", "comment"];

/// Jira refused a call or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum JiraError {
    /// The email address or token is wrong, or the account may not do this. The message
    /// says what to check.
    #[error("{0}")]
    Unauthorized(&'static str),
    /// Jira refused the request itself: invalid JQL, a field that does not exist on the
    /// project, a transition that does not apply. The message is Jira's own, and the caller
    /// is the one who can fix it.
    #[error("{0}")]
    Invalid(String),
    /// No issue with that key, or none this token may see. Jira answers `404` for both.
    #[error("Jira has no issue with that key, or the token cannot see it")]
    NotFound,
    /// Jira could not be reached, or refused for a reason the caller cannot fix.
    #[error("{0}")]
    Refused(String),
}

/// The site a call goes to and the credentials it goes with.
#[derive(Debug, Clone, Copy)]
pub struct Site<'a> {
    /// The site's origin, as [`normalize_site_url`] returns it.
    pub url: &'a str,
    /// The Atlassian account's email address, the username half of basic auth.
    pub email: &'a str,
    pub token: &'a SecretString,
}

/// Whose token it is, as Jira reports it.
#[expect(
    clippy::struct_field_names,
    reason = "accountId is Atlassian's own name for the field, and the one clients read"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Atlassian's stable id for the account.
    pub account_id: String,
    pub display_name: String,
    /// The address on the account, or `None` when the account hides it, which Atlassian
    /// allows. The address that signs in is the credential's, not this one.
    pub email: Option<String>,
}

/// One project a token can reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub key: String,
    pub name: String,
}

/// One board a token can reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub id: i64,
    pub name: String,
    /// The board's project, when it maps to exactly one.
    pub project_key: Option<String>,
}

/// One page-capped listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing<Item> {
    pub items: Vec<Item>,
    /// Whether Jira had more pages than [`PAGE_LIMIT`] allows.
    pub truncated: bool,
}

/// An account named on an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub account_id: String,
    pub display_name: String,
}

/// An issue's status, with the category that colors it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub name: String,
    /// `new`, `indeterminate`, or `done`, as Jira groups statuses.
    pub category: String,
}

/// One comment on an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub author: Option<User>,
    pub body: RichText,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// One issue, the same shape wherever Elysium returns one.
///
/// `description` and `comments` are filled in by [`Jira::issue`] alone: a search leaves them
/// empty, since a listing has no use for them and they are the largest part of an issue.
#[expect(
    clippy::struct_field_names,
    reason = "issueType is Jira's own name for the field, and the one clients read"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: String,
    pub key: String,
    /// The project Jira reports the issue is in, which the allowlist is checked against.
    pub project_key: String,
    pub summary: String,
    pub status: Option<Status>,
    pub issue_type: Option<String>,
    pub priority: Option<String>,
    pub assignee: Option<User>,
    pub reporter: Option<User>,
    pub labels: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    /// Where a person reads this issue, on the credential's own site.
    pub url: String,
    pub description: Option<RichText>,
    pub comments: Vec<Comment>,
}

/// One transition an issue can make right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub id: String,
    pub name: String,
    /// The status the issue lands in.
    pub to: Option<Status>,
}

/// One page of a bounded search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Search {
    pub issues: Vec<Issue>,
    /// Pass this back as the next call's token. `None` on the last page.
    pub next_page_token: Option<String>,
    pub is_last: bool,
}

/// The fields a new issue is created with. Text arrives plain and is wrapped into ADF here.
#[derive(Debug, Default)]
pub struct NewIssue {
    pub project_key: String,
    pub issue_type: String,
    pub summary: String,
    pub description: Option<String>,
    pub labels: Option<Vec<String>>,
    pub priority: Option<String>,
    pub assignee_account_id: Option<String>,
    /// The epic or parent issue this one belongs under.
    pub parent_key: Option<String>,
}

/// A partial change to an issue's fields. `None` leaves a field alone.
#[derive(Debug, Default)]
pub struct IssueChanges {
    pub summary: Option<String>,
    pub description: Option<String>,
    pub labels: Option<Vec<String>>,
    pub priority: Option<String>,
    /// `Some(None)` unassigns the issue; `None` leaves the assignee alone.
    #[expect(
        clippy::option_option,
        reason = "absent, unassign, and assign are three distinct requests"
    )]
    pub assignee_account_id: Option<Option<String>>,
}

impl IssueChanges {
    /// True when applying these changes would not touch any field.
    pub const fn is_empty(&self) -> bool {
        self.summary.is_none()
            && self.description.is_none()
            && self.labels.is_none()
            && self.priority.is_none()
            && self.assignee_account_id.is_none()
    }

    /// These changes as the `fields` object Jira takes.
    fn fields(&self) -> Value {
        let mut fields = serde_json::Map::new();
        if let Some(summary) = &self.summary {
            fields.insert("summary".to_owned(), json!(summary));
        }
        if let Some(description) = &self.description {
            fields.insert("description".to_owned(), adf::from_plain_text(description));
        }
        if let Some(labels) = &self.labels {
            fields.insert("labels".to_owned(), json!(labels));
        }
        if let Some(priority) = &self.priority {
            fields.insert("priority".to_owned(), json!({ "name": priority }));
        }
        if let Some(assignee) = &self.assignee_account_id {
            // Jira unassigns an issue when the whole assignee object is null.
            let value = assignee
                .as_ref()
                .map_or(Value::Null, |account_id| json!({ "accountId": account_id }));
            fields.insert("assignee".to_owned(), value);
        }
        Value::Object(fields)
    }
}

/// The origin of a Jira Cloud site, or what is wrong with it.
///
/// Only an `https` origin on `atlassian.net` is accepted, with no userinfo, port, path,
/// query, or fragment, since anything more would let a stored credential aim Elysium
/// somewhere else. What comes back is the origin alone, so two spellings of one site are
/// one site.
///
/// # Errors
/// Returns what to fix, for a client to read.
pub fn normalize_site_url(value: &str) -> Result<String, &'static str> {
    const ADVICE: &str =
        "a Jira site is its address alone, such as https://your-site.atlassian.net";

    let value = value.trim();
    if value.len() > SITE_URL_MAX_BYTES {
        return Err("the site URL is too long to be a Jira Cloud site");
    }
    let Ok(url) = Url::parse(value) else {
        return Err(ADVICE);
    };
    if url.scheme() != "https" {
        return Err("a Jira Cloud site is reached over https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("a Jira site URL carries no username or password");
    }
    if url.port().is_some() {
        return Err("a Jira Cloud site is reached on the default port");
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
        return Err("Elysium reaches Jira Cloud only, so a site ends in .atlassian.net");
    };
    // What is left is the site's own labels, checked the way the column's own constraint
    // checks them, so a host Postgres would refuse is answered here instead of at the
    // insert. `url` has already lowercased the host and punycoded anything that was not
    // ASCII. A site may carry more than one label, as `acme.jira-dev.atlassian.net` does.
    let named = site.split('.').all(|label| {
        let starts_named = label.starts_with(|first: char| first.is_ascii_alphanumeric());
        starts_named
            && label
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
    });
    if !named {
        return Err(
            "a Jira Cloud site is named in letters, digits, and hyphens, and starts \
                    with a letter or a digit",
        );
    }

    Ok(format!("https://{host}"))
}

/// The project an issue key names, or `None` when it is not an issue key at all.
///
/// An issue key is a project key, a hyphen, and a number, such as `ELY-12`. A project key
/// holds no hyphen, so the project is everything before the last one. Jira accepts a key in
/// any case, so callers compare case insensitively.
///
/// This is also what makes a key safe to put straight into a URL path: a key that passes
/// holds only letters, digits, underscores, and one hyphen, so there is nothing to encode
/// and no way to reach another resource with it.
pub fn project_of_issue_key(issue_key: &str) -> Option<&str> {
    let (project_key, number) = issue_key.rsplit_once('-')?;

    let is_project_key = (2..=255).contains(&project_key.len())
        && project_key.starts_with(|first: char| first.is_ascii_alphabetic())
        && project_key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
    let is_number =
        (1..=19).contains(&number.len()) && number.chars().all(|digit| digit.is_ascii_digit());
    if !is_project_key || !is_number {
        return None;
    }

    Some(project_key)
}

/// Jira's API client. Cheap to clone; clones share one HTTP client.
#[derive(Debug, Clone)]
pub struct Jira {
    http: reqwest::Client,
    /// Stands in for every site's origin, so tests can point the client at a local fake.
    /// Production code has no way to set it.
    #[cfg(test)]
    test_origin: Option<String>,
}

impl Jira {
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

    /// Checks a token with Jira and reports whose it is.
    ///
    /// # Errors
    /// Returns [`JiraError::Unauthorized`] when Jira rejects the email address or token,
    /// and [`JiraError::Refused`] when the site cannot be reached.
    pub async fn verify(&self, site: &Site<'_>) -> Result<Account, JiraError> {
        let reply = self
            .send(self.request(Method::GET, site, "/rest/api/3/myself"))
            .await?;
        let body: AccountBody = reply.read("an account")?;
        Ok(Account {
            account_id: body.account_id,
            display_name: body.display_name.unwrap_or_default(),
            email: body.email_address,
        })
    }

    /// The projects a token can reach, following Jira's pages up to [`PAGE_LIMIT`].
    ///
    /// # Errors
    /// Returns [`JiraError::Unauthorized`] when Jira rejects the credentials, and
    /// [`JiraError::Refused`] when the site cannot be reached or refuses a page.
    pub async fn list_projects(&self, site: &Site<'_>) -> Result<Listing<Project>, JiraError> {
        let listing: Listing<ProjectBody> = self.paged(site, "/rest/api/3/project/search").await?;
        Ok(Listing {
            items: listing
                .items
                .into_iter()
                .map(|project| Project {
                    id: project.id,
                    key: project.key,
                    name: project.name.unwrap_or_default(),
                })
                .collect(),
            truncated: listing.truncated,
        })
    }

    /// The boards a token can reach, following Jira's pages up to [`PAGE_LIMIT`].
    ///
    /// # Errors
    /// As [`Jira::list_projects`].
    pub async fn list_boards(&self, site: &Site<'_>) -> Result<Listing<Board>, JiraError> {
        let listing: Listing<BoardBody> = self.paged(site, "/rest/agile/1.0/board").await?;
        Ok(Listing {
            items: listing
                .items
                .into_iter()
                .map(|board| Board {
                    id: board.id,
                    name: board.name.unwrap_or_default(),
                    project_key: board.location.and_then(|location| location.project_key),
                })
                .collect(),
            truncated: listing.truncated,
        })
    }

    /// One page of a JQL search, in Jira's own order unless the JQL says otherwise.
    ///
    /// The JQL must already be bounded to what the credential may reach: this sends what it
    /// is given. Paging is by token, since Jira Cloud's search endpoint has no offset and
    /// reports no total.
    ///
    /// # Errors
    /// Returns [`JiraError::Invalid`] for JQL Jira will not run, and otherwise as
    /// [`Jira::list_projects`].
    pub async fn search(
        &self,
        site: &Site<'_>,
        jql: &str,
        max_results: u32,
        next_page_token: Option<&str>,
    ) -> Result<Search, JiraError> {
        let mut body = json!({
            "jql": jql,
            "maxResults": max_results,
            "fields": SEARCH_FIELDS,
        });
        // Sent only when there is one: Jira reads an explicit null as a token it cannot use.
        if let Some(token) = next_page_token {
            body["nextPageToken"] = json!(token);
        }
        let reply = self
            .send(
                self.request(Method::POST, site, "/rest/api/3/search/jql")
                    .json(&body),
            )
            .await?;
        let found: SearchBody = reply.read("search results")?;

        let next_page_token = found.next_page_token;
        Ok(Search {
            issues: found
                .issues
                .into_iter()
                .map(|issue| issue.into_issue(site.url))
                .collect(),
            // Jira sends no token on the last page, which is the signal `isLast` repeats.
            is_last: found.is_last.unwrap_or(next_page_token.is_none()),
            next_page_token,
        })
    }

    /// One issue by key, with its description and comments.
    ///
    /// # Errors
    /// Returns [`JiraError::NotFound`] for an issue that does not exist or that the token
    /// cannot see, which Jira answers alike, and otherwise as [`Jira::list_projects`].
    pub async fn issue(&self, site: &Site<'_>, key: &str) -> Result<Issue, JiraError> {
        let fields = SEARCH_FIELDS
            .iter()
            .chain(ISSUE_FIELDS)
            .copied()
            .collect::<Vec<&str>>()
            .join(",");
        let path = format!("/rest/api/3/issue/{key}?fields={fields}");
        let reply = self.send(self.request(Method::GET, site, &path)).await?;
        let body: IssueBody = reply.read("an issue")?;
        Ok(body.into_issue(site.url))
    }

    /// Creates an issue and reports the key Jira gave it.
    ///
    /// # Errors
    /// Returns [`JiraError::Invalid`] when Jira refuses the fields, such as an issue type
    /// the project does not have, and otherwise as [`Jira::list_projects`].
    pub async fn create_issue(
        &self,
        site: &Site<'_>,
        new_issue: &NewIssue,
    ) -> Result<String, JiraError> {
        let mut fields = serde_json::Map::new();
        fields.insert(
            "project".to_owned(),
            json!({ "key": new_issue.project_key }),
        );
        fields.insert(
            "issuetype".to_owned(),
            json!({ "name": new_issue.issue_type }),
        );
        fields.insert("summary".to_owned(), json!(new_issue.summary));
        if let Some(description) = &new_issue.description {
            fields.insert("description".to_owned(), adf::from_plain_text(description));
        }
        if let Some(labels) = &new_issue.labels {
            fields.insert("labels".to_owned(), json!(labels));
        }
        if let Some(priority) = &new_issue.priority {
            fields.insert("priority".to_owned(), json!({ "name": priority }));
        }
        if let Some(account_id) = &new_issue.assignee_account_id {
            fields.insert("assignee".to_owned(), json!({ "accountId": account_id }));
        }
        if let Some(parent_key) = &new_issue.parent_key {
            fields.insert("parent".to_owned(), json!({ "key": parent_key }));
        }

        let body = json!({ "fields": Value::Object(fields) });
        let reply = self
            .send(
                self.request(Method::POST, site, "/rest/api/3/issue")
                    .json(&body),
            )
            .await?;
        let created: CreatedIssueBody = reply.read("the created issue")?;
        Ok(created.key)
    }

    /// Changes an issue's fields.
    ///
    /// # Errors
    /// As [`Jira::create_issue`], plus [`JiraError::NotFound`] for an unknown issue.
    pub async fn update_issue(
        &self,
        site: &Site<'_>,
        key: &str,
        changes: &IssueChanges,
    ) -> Result<(), JiraError> {
        let path = format!("/rest/api/3/issue/{key}");
        let body = json!({ "fields": changes.fields() });
        self.send(self.request(Method::PUT, site, &path).json(&body))
            .await?;
        Ok(())
    }

    /// The transitions an issue can make right now, for whoever the token is.
    ///
    /// # Errors
    /// As [`Jira::issue`].
    pub async fn transitions(
        &self,
        site: &Site<'_>,
        key: &str,
    ) -> Result<Vec<Transition>, JiraError> {
        let path = format!("/rest/api/3/issue/{key}/transitions");
        let reply = self.send(self.request(Method::GET, site, &path)).await?;
        let body: TransitionsBody = reply.read("transitions")?;
        Ok(body
            .transitions
            .into_iter()
            .map(|transition| Transition {
                id: transition.id,
                name: transition.name.unwrap_or_default(),
                to: transition.to.map(StatusBody::into_status),
            })
            .collect())
    }

    /// Moves an issue along one transition, optionally leaving a comment with it.
    ///
    /// # Errors
    /// Returns [`JiraError::Invalid`] for a transition that does not apply to the issue in
    /// its current status, and otherwise as [`Jira::issue`].
    pub async fn apply_transition(
        &self,
        site: &Site<'_>,
        key: &str,
        transition_id: &str,
        comment: Option<&str>,
    ) -> Result<(), JiraError> {
        let path = format!("/rest/api/3/issue/{key}/transitions");
        let mut body = json!({ "transition": { "id": transition_id } });
        if let Some(comment) = comment {
            body["update"] = json!({
                "comment": [{ "add": { "body": adf::from_plain_text(comment) } }],
            });
        }
        self.send(self.request(Method::POST, site, &path).json(&body))
            .await?;
        Ok(())
    }

    /// Adds a comment, written as plain text and sent as ADF.
    ///
    /// # Errors
    /// As [`Jira::issue`].
    pub async fn add_comment(
        &self,
        site: &Site<'_>,
        key: &str,
        body: &str,
    ) -> Result<Comment, JiraError> {
        let path = format!("/rest/api/3/issue/{key}/comment");
        let request = json!({ "body": adf::from_plain_text(body) });
        let reply = self
            .send(self.request(Method::POST, site, &path).json(&request))
            .await?;
        let added: CommentBody = reply.read("the added comment")?;
        Ok(added.into_comment())
    }

    /// Follows one listing's pages up to [`PAGE_LIMIT`], reporting whether more remained.
    async fn paged<Item: serde::de::DeserializeOwned>(
        &self,
        site: &Site<'_>,
        path: &str,
    ) -> Result<Listing<Item>, JiraError> {
        let mut items = Vec::new();
        let mut truncated = false;

        for page in 0..PAGE_LIMIT {
            let start_at = page * PAGE_SIZE;
            let paged_path = format!("{path}?startAt={start_at}&maxResults={PAGE_SIZE}");
            let reply = self
                .send(self.request(Method::GET, site, &paged_path))
                .await?;
            let mut page: PageBody<Item> = reply.read("a page")?;

            let is_last = page
                .is_last
                .unwrap_or(page.values.len() < PAGE_SIZE as usize);
            items.append(&mut page.values);
            truncated = !is_last;
            if is_last {
                break;
            }
        }

        Ok(Listing { items, truncated })
    }

    /// One authenticated request against the credential's site.
    fn request(&self, method: Method, site: &Site<'_>, path: &str) -> RequestBuilder {
        #[cfg(not(test))]
        let origin = site.url;
        #[cfg(test)]
        let origin = self.test_origin.as_deref().unwrap_or(site.url);

        let url = format!("{origin}{path}");
        self.http
            .request(method, url)
            .basic_auth(site.email, Some(site.token.expose_secret()))
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .timeout(REQUEST_TIMEOUT)
    }

    /// Sends a request and reads the whole answer, turning anything but success into an
    /// error that says whose fault it is.
    async fn send(&self, request: RequestBuilder) -> Result<Reply, JiraError> {
        let response = request
            .send()
            .await
            .map_err(|error| JiraError::Refused(format!("Jira: {error}")))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| JiraError::Refused(format!("Jira: {error}")))?;

        let reply = Reply { status, body };
        if status.is_success() {
            return Ok(reply);
        }
        Err(reply.failure())
    }
}

/// One answer from Jira, read in full.
struct Reply {
    status: StatusCode,
    body: String,
}

impl Reply {
    /// The answer parsed as `Shape`, or a refusal naming what could not be read.
    fn read<Shape: serde::de::DeserializeOwned>(
        &self,
        expected: &'static str,
    ) -> Result<Shape, JiraError> {
        serde_json::from_str(&self.body).map_err(|error| {
            JiraError::Refused(format!("Jira sent {expected} that cannot be read: {error}"))
        })
    }

    /// An unsuccessful answer as the error whoever can fix it should see.
    fn failure(&self) -> JiraError {
        match self.status {
            StatusCode::UNAUTHORIZED => JiraError::Unauthorized(
                "Jira refused the account email and API token; check that the token was copied \
                 whole, belongs to that email address, and has not been revoked",
            ),
            StatusCode::FORBIDDEN => JiraError::Unauthorized(
                "Jira accepted the token but refused the request; check that the account may see \
                 this project and that the site allows API tokens",
            ),
            StatusCode::NOT_FOUND => JiraError::NotFound,
            StatusCode::BAD_REQUEST => JiraError::Invalid(format!("Jira: {}", self.message())),
            _ => JiraError::Refused(format!("Jira: {}", self.message())),
        }
    }

    /// Jira's own account of what went wrong, which never carries a credential.
    fn message(&self) -> String {
        let Ok(body) = serde_json::from_str::<ErrorBody>(&self.body) else {
            return self.status.to_string();
        };

        let mut messages = body.error_messages;
        messages.extend(
            body.errors
                .into_iter()
                .map(|(field, message)| format!("{field}: {message}")),
        );
        if messages.is_empty() {
            return self.status.to_string();
        }
        messages.join("; ")
    }
}

/// Jira's error body, such as `{"errorMessages":["..."],"errors":{"summary":"..."}}`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody {
    #[serde(default)]
    error_messages: Vec<String>,
    #[serde(default)]
    errors: std::collections::BTreeMap<String, String>,
}

/// The parts of `GET /rest/api/3/myself` Elysium keeps.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountBody {
    account_id: String,
    display_name: Option<String>,
    /// Absent when the account hides its address.
    email_address: Option<String>,
}

/// One page of any of Jira's paged listings.
#[derive(Deserialize)]
#[serde(
    rename_all = "camelCase",
    bound(deserialize = "Item: Deserialize<'de>")
)]
struct PageBody<Item> {
    #[serde(default)]
    values: Vec<Item>,
    is_last: Option<bool>,
}

#[derive(Deserialize)]
struct ProjectBody {
    id: String,
    key: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct BoardBody {
    id: i64,
    name: Option<String>,
    location: Option<BoardLocationBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoardLocationBody {
    project_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchBody {
    #[serde(default)]
    issues: Vec<IssueBody>,
    next_page_token: Option<String>,
    is_last: Option<bool>,
}

#[derive(Deserialize)]
struct CreatedIssueBody {
    key: String,
}

#[derive(Deserialize)]
struct TransitionsBody {
    #[serde(default)]
    transitions: Vec<TransitionBody>,
}

#[derive(Deserialize)]
struct TransitionBody {
    id: String,
    name: Option<String>,
    to: Option<StatusBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusBody {
    name: Option<String>,
    status_category: Option<StatusCategoryBody>,
}

impl StatusBody {
    fn into_status(self) -> Status {
        Status {
            name: self.name.unwrap_or_default(),
            category: self
                .status_category
                .and_then(|category| category.key)
                .unwrap_or_default(),
        }
    }
}

#[derive(Deserialize)]
struct StatusCategoryBody {
    key: Option<String>,
}

#[derive(Deserialize)]
struct NamedBody {
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserBody {
    account_id: Option<String>,
    display_name: Option<String>,
}

impl UserBody {
    fn into_user(self) -> Option<User> {
        Some(User {
            account_id: self.account_id?,
            display_name: self.display_name.unwrap_or_default(),
        })
    }
}

#[derive(Deserialize)]
struct CommentsBody {
    #[serde(default)]
    comments: Vec<CommentBody>,
}

#[derive(Deserialize)]
struct CommentBody {
    id: String,
    author: Option<UserBody>,
    body: Option<Value>,
    created: Option<String>,
    updated: Option<String>,
}

impl CommentBody {
    fn into_comment(self) -> Comment {
        Comment {
            id: self.id,
            author: self.author.and_then(UserBody::into_user),
            body: RichText::from_document(self.body.unwrap_or(Value::Null)),
            created_at: self.created.as_deref().and_then(parse_timestamp),
            updated_at: self.updated.as_deref().and_then(parse_timestamp),
        }
    }
}

#[derive(Deserialize)]
struct IssueBody {
    id: String,
    key: String,
    #[serde(default)]
    fields: IssueFieldsBody,
}

#[derive(Deserialize, Default)]
struct IssueFieldsBody {
    summary: Option<String>,
    status: Option<StatusBody>,
    /// Jira spells this one without a separator, unlike every other field here.
    issuetype: Option<NamedBody>,
    priority: Option<NamedBody>,
    assignee: Option<UserBody>,
    reporter: Option<UserBody>,
    #[serde(default)]
    labels: Vec<String>,
    created: Option<String>,
    updated: Option<String>,
    project: Option<ProjectRefBody>,
    description: Option<Value>,
    comment: Option<CommentsBody>,
}

#[derive(Deserialize)]
struct ProjectRefBody {
    key: Option<String>,
}

impl IssueBody {
    /// The issue as Elysium returns it, linked on the site it lives on.
    ///
    /// The project comes from what Jira reported, falling back to the key's own prefix, so
    /// the allowlist always has a project to check even if a field was not asked for.
    fn into_issue(self, site_url: &str) -> Issue {
        let fields = self.fields;
        let project_key = fields
            .project
            .and_then(|project| project.key)
            .or_else(|| project_of_issue_key(&self.key).map(str::to_owned))
            .unwrap_or_default();

        Issue {
            url: format!("{site_url}/browse/{}", self.key),
            id: self.id,
            key: self.key,
            project_key,
            summary: fields.summary.unwrap_or_default(),
            status: fields.status.map(StatusBody::into_status),
            issue_type: fields.issuetype.and_then(|issue_type| issue_type.name),
            priority: fields.priority.and_then(|priority| priority.name),
            assignee: fields.assignee.and_then(UserBody::into_user),
            reporter: fields.reporter.and_then(UserBody::into_user),
            labels: fields.labels,
            created_at: fields.created.as_deref().and_then(parse_timestamp),
            updated_at: fields.updated.as_deref().and_then(parse_timestamp),
            description: fields.description.map(RichText::from_document),
            comments: fields
                .comment
                .map(|comment| {
                    comment
                        .comments
                        .into_iter()
                        .map(CommentBody::into_comment)
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// The instant in a Jira timestamp, such as `2026-09-17T12:00:00.000+0000`.
///
/// Jira writes the offset without a colon, which is not RFC 3339, so this parses that shape
/// first and falls back to RFC 3339 for anything better behaved. A timestamp in neither
/// shape reads as none: when an issue was updated is worth showing, but not worth failing a
/// whole issue over.
fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f%z") {
        return Some(parsed.with_timezone(&Utc));
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.with_timezone(&Utc));
    }

    event!(
        name: "jira.timestamp.unreadable",
        Level::DEBUG,
        jira.timestamp = value,
        "Jira sent a timestamp in an unknown format",
    );
    None
}
