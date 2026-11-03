// Copyright © 2026 Jalapeno Labs

//! Confluence Cloud's REST API, reached through one [`Confluence`] handle. See
//! `docs/connectors.md`, Confluence.
//!
//! Confluence lives on the same site as Jira (`https://<site>.atlassian.net/wiki`) and takes
//! the same HTTP basic auth: the Atlassian account's email address and an API token. A
//! Confluence connector therefore reads through an Atlassian account a Jira connector already
//! uses, and calls go to the account's stored, validated site origin (`crate::jira::Site`).
//!
//! # Bounded to the chosen spaces
//!
//! A connector reads the spaces its person picked, or all of them. [`Confluence::changes`] is
//! the only call that reads content, and it takes the connector's [`SpaceScope`]: the scope
//! decides the CQL's `space in (...)` clause, and every result is checked against it again on
//! the way back, so a result from another space is dropped rather than recorded. Listing the
//! spaces a token reaches ([`Confluence::list_spaces`]) is what a selection is picked from, so it
//! is not bounded.
//!
//! # Endpoints
//!
//! - `GET /wiki/rest/api/user/current`: whose token it is.
//! - `GET /wiki/api/v2/spaces`: the spaces it reaches, paged by the `_links.next` cursor.
//! - `GET /wiki/rest/api/content/search`: pages and comments changed lately, by CQL, with their
//!   storage-format bodies, paged by `_links.next`.
//!
//! Every field read is typed as Atlassian's `OpenAPI` specifications declare it (v1
//! `swagger.v3.json`, v2 `openapi-v2.v3.json`). v1 and v2 disagree on ids: content ids are
//! strings in both, but a v1 space's id is a number, and a page's `container` is its space. The
//! tests prove Elysium's half against a fake shaped the same way, and `CLAUDE.md` beside this
//! file records how Confluence has broken the reader before.

pub mod reader;
pub mod storage;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
pub(crate) mod tests;

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::ACCEPT;
use reqwest::{Method, RequestBuilder, StatusCode};
use secrecy::ExposeSecret;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tracing::{Level, event};

use crate::jira::{Account, Site};

/// How many spaces one page of the listing asks for: the most Confluence's v2 API sends.
const SPACE_PAGE_SIZE: u32 = 250;

/// How many pages of spaces a listing follows, so 2,500 spaces at most. A site past that is
/// told the listing was cut short, as Jira's project listing is.
const SPACE_PAGE_LIMIT: u32 = 10;

/// How many results one page of a content search asks for. Fifty is Confluence's own limit
/// for a search that expands bodies.
const SEARCH_PAGE_SIZE: u32 = 50;

/// How long one call may take. Confluence answers a page in well under a second.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// What a search asks Confluence to include with each result: the body to render, who changed
/// it and when, its space, who created it, and for a comment the page it is on.
const SEARCH_EXPAND: &str = "body.storage,version,space,history,container,container.history";

/// Confluence refused a call or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum ConfluenceError {
    /// The email address or token is wrong, or the account may not do this.
    #[error("{0}")]
    Unauthorized(&'static str),
    /// Confluence refused the request itself, such as a query it cannot read.
    #[error("{0}")]
    Invalid(String),
    /// Confluence could not be reached, or refused for a reason the caller cannot fix.
    #[error("{0}")]
    Refused(String),
}

/// One space a token reaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    /// Confluence's stable id for the space.
    pub id: String,
    /// The key CQL names it by, such as `ENG`, or `~<account id>` for a personal space.
    pub key: String,
    pub name: String,
}

/// A listing, and whether Confluence had more than it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing<Item> {
    pub items: Vec<Item>,
    pub truncated: bool,
}

/// The spaces a connector reads: all of them, including spaces added later, or the ones its
/// person picked. Stored in `connectors.content` as `"*"` or the picked spaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceScope {
    All,
    Only(Vec<Space>),
}

impl SpaceScope {
    /// Whether the scope is an empty pick, which reads nothing: no search is sent at all.
    pub fn reads_nothing(&self) -> bool {
        matches!(self, Self::Only(spaces) if spaces.is_empty())
    }

    /// The CQL clause that keeps a search inside the scope, when it is not every space.
    fn cql_clause(&self) -> Option<String> {
        let Self::Only(spaces) = self else {
            return None;
        };
        let keys: Vec<String> = spaces.iter().map(|space| quoted(&space.key)).collect();
        Some(format!("space in ({})", keys.join(",")))
    }

    /// Whether a result from the space with `key` is one the scope reads.
    pub fn admits(&self, key: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(spaces) => spaces.iter().any(|space| space.key == key),
        }
    }
}

impl Serialize for SpaceScope {
    fn serialize<Target: Serializer>(
        &self,
        serializer: Target,
    ) -> Result<Target::Ok, Target::Error> {
        match self {
            Self::All => serializer.serialize_str("*"),
            Self::Only(spaces) => spaces.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for SpaceScope {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Picked(Vec<Space>),
            Everything(String),
        }

        match Wire::deserialize(deserializer)? {
            Wire::Picked(spaces) => Ok(Self::Only(spaces)),
            Wire::Everything(text) if text == "*" => Ok(Self::All),
            Wire::Everything(_) => Err(serde::de::Error::custom(
                "spaces are \"*\" or a list of spaces",
            )),
        }
    }
}

/// A CQL string literal, with its quotes and backslashes escaped.
fn quoted(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// What kind of content a change is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentType {
    Page,
    Comment,
}

impl ContentType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Comment => "comment",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Page, Self::Comment]
            .into_iter()
            .find(|kind| kind.as_str() == text)
    }
}

/// Someone, as Confluence names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub account_id: String,
    pub display_name: String,
}

/// The page a comment is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub id: String,
    pub title: String,
    /// The account that created the page, when Confluence says.
    pub created_by: Option<String>,
}

/// One page or comment as it stands after its latest change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub id: String,
    pub content_type: ContentType,
    /// A page's title; a comment's is usually `Re: <page title>`.
    pub title: String,
    pub space_key: String,
    pub space_name: String,
    /// Its version number, which every change raises by one.
    pub version: i64,
    /// When the change happened, when Confluence says in a shape that can be read.
    pub modified_at: Option<DateTime<Utc>>,
    /// Who made the change, when Confluence says.
    pub modified_by: Option<Person>,
    /// The account that created it, when Confluence says.
    pub created_by: Option<String>,
    /// Its body in storage format; see [`storage`].
    pub storage: String,
    /// Where a person reads it, on the account's own site.
    pub url: Option<String>,
    /// The page a comment is on.
    pub container: Option<Container>,
}

/// What a search read, oldest change first, and whether it read everything that matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes {
    pub items: Vec<Change>,
    /// False when more matched than the search was allowed to read.
    pub complete: bool,
}

/// What a content search reads.
#[derive(Debug, Clone, Copy)]
pub struct ChangeQuery<'scope> {
    pub scope: &'scope SpaceScope,
    pub types: &'scope [ContentType],
    /// How far back to look, in minutes. CQL's relative `now("-Nm")` is used rather than a
    /// date, because Confluence reads a written date in the account's own time zone.
    pub since_minutes: i64,
    /// The most results to read, across pages.
    pub max_items: usize,
}

/// The CQL a query sends, or `None` when its scope reads nothing.
fn cql(query: &ChangeQuery<'_>) -> Option<String> {
    if query.scope.reads_nothing() {
        return None;
    }
    let types: Vec<&str> = query.types.iter().map(|kind| kind.as_str()).collect();
    let mut clauses = vec![
        format!("type in ({})", types.join(",")),
        format!("lastmodified >= now(\"-{}m\")", query.since_minutes.max(1)),
    ];
    clauses.extend(query.scope.cql_clause());
    Some(format!(
        "{} ORDER BY lastmodified ASC",
        clauses.join(" AND ")
    ))
}

/// Confluence's API client. Cheap to clone; clones share one HTTP client.
#[derive(Debug, Clone)]
pub struct Confluence {
    http: reqwest::Client,
    /// Stands in for every site's origin, so tests can point the client at a local fake.
    /// Production code has no way to set it.
    #[cfg(test)]
    test_origin: Option<String>,
}

impl Confluence {
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

    /// Checks a token with Confluence and reports whose it is.
    ///
    /// # Errors
    /// [`ConfluenceError::Unauthorized`] when Confluence refuses the email address or token, and
    /// [`ConfluenceError::Refused`] when the site cannot be reached.
    pub async fn verify(&self, site: &Site<'_>) -> Result<Account, ConfluenceError> {
        let reply = self
            .send(self.request(Method::GET, site, "/wiki/rest/api/user/current"))
            .await?;
        let body: CurrentUserBody = reply.read("an account")?;
        if body.account_id.is_empty() {
            // Confluence answers an anonymous user rather than refusing, for some sites.
            return Err(ConfluenceError::Unauthorized(UNAUTHORIZED_ADVICE));
        }
        Ok(Account {
            account_id: body.account_id,
            display_name: body.display_name.or(body.public_name).unwrap_or_default(),
            email: body.email.filter(|email| !email.is_empty()),
        })
    }

    /// The spaces a token reaches, following Confluence's pages up to [`SPACE_PAGE_LIMIT`].
    ///
    /// # Errors
    /// As [`Confluence::verify`].
    pub async fn list_spaces(&self, site: &Site<'_>) -> Result<Listing<Space>, ConfluenceError> {
        let mut items: Vec<Space> = Vec::new();
        let mut path = format!("/wiki/api/v2/spaces?limit={SPACE_PAGE_SIZE}&status=current");
        for _page in 0..SPACE_PAGE_LIMIT {
            let reply = self.send(self.request(Method::GET, site, &path)).await?;
            let body: SpacesBody = reply.read("a list of spaces")?;
            items.extend(body.results.into_iter().map(|space| Space {
                id: space.id,
                name: space.name.unwrap_or_else(|| space.key.clone()),
                key: space.key,
            }));
            match body.links.next {
                // Only a further page of the same listing is followed, never another URL.
                Some(next) if next.starts_with("/wiki/api/v2/spaces?") => path = next,
                Some(next) => {
                    return Err(ConfluenceError::Refused(format!(
                        "Confluence sent a next page Elysium will not follow: {next}"
                    )));
                }
                None => {
                    return Ok(Listing {
                        items,
                        truncated: false,
                    });
                }
            }
        }
        Ok(Listing {
            items,
            truncated: true,
        })
    }

    /// Pages and comments changed within `query`'s window in its spaces, oldest change first.
    ///
    /// This is the only call that reads content, and it is bounded by `query.scope` twice: in
    /// the CQL it sends, and again on each result, so one from another space is dropped. An
    /// empty pick reads nothing and sends nothing.
    ///
    /// # Errors
    /// As [`Confluence::verify`], and [`ConfluenceError::Invalid`] for a query Confluence cannot
    /// read.
    pub async fn changes(
        &self,
        site: &Site<'_>,
        query: &ChangeQuery<'_>,
    ) -> Result<Changes, ConfluenceError> {
        let Some(cql) = cql(query) else {
            return Ok(Changes {
                items: Vec::new(),
                complete: true,
            });
        };
        let mut items: Vec<Change> = Vec::new();
        let mut request = self
            .request(Method::GET, site, "/wiki/rest/api/content/search")
            .query(&[
                ("cql", cql.as_str()),
                ("limit", &SEARCH_PAGE_SIZE.to_string()),
                ("expand", SEARCH_EXPAND),
            ]);
        loop {
            let reply = self.send(request).await?;
            let body: SearchBody = reply.read("search results")?;
            for result in body.results {
                let Some(change) = into_change(result, site.url) else {
                    continue;
                };
                if !query.scope.admits(&change.space_key) {
                    event!(
                        name: "confluence.search.out_of_scope",
                        Level::WARN,
                        confluence.space = %change.space_key,
                        "Confluence answered a result from a space the connector does not read; it was dropped",
                    );
                    continue;
                }
                items.push(change);
                if items.len() >= query.max_items {
                    return Ok(Changes {
                        items,
                        complete: false,
                    });
                }
            }
            match body.links.next {
                // A further page of the same search, relative to `/wiki`; nothing else is followed.
                Some(next) if next.starts_with("/rest/api/content/search?") => {
                    request = self.request(Method::GET, site, &format!("/wiki{next}"));
                }
                Some(next) => {
                    return Err(ConfluenceError::Refused(format!(
                        "Confluence sent a next page Elysium will not follow: {next}"
                    )));
                }
                None => {
                    return Ok(Changes {
                        items,
                        complete: true,
                    });
                }
            }
        }
    }

    /// One authenticated request against the account's site.
    fn request(&self, method: Method, site: &Site<'_>, path: &str) -> RequestBuilder {
        #[cfg(not(test))]
        let origin = site.url;
        #[cfg(test)]
        let origin = self.test_origin.as_deref().unwrap_or(site.url);

        self.http
            .request(method, format!("{origin}{path}"))
            .basic_auth(site.email, Some(site.token.expose_secret()))
            .header(ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT)
    }

    /// Sends a request and reads the whole answer, turning anything but success into an error
    /// that says whose fault it is.
    async fn send(&self, request: RequestBuilder) -> Result<Reply, ConfluenceError> {
        let response = request.send().await.map_err(|error| {
            ConfluenceError::Refused(format!("Confluence: {}", error.without_url()))
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            ConfluenceError::Refused(format!("Confluence: {}", error.without_url()))
        })?;
        let reply = Reply { status, body };
        if status.is_success() {
            return Ok(reply);
        }
        Err(reply.failure())
    }
}

/// What to check when Confluence refuses the credentials.
const UNAUTHORIZED_ADVICE: &str = "Confluence refused the account email and API token; check that \
     the token was copied whole, belongs to that email address, has not been revoked, and that the \
     account may use Confluence on this site";

/// One answer from Confluence, read in full.
#[derive(Debug)]
struct Reply {
    status: StatusCode,
    body: String,
}

impl Reply {
    fn read<Shape: serde::de::DeserializeOwned>(
        &self,
        expected: &'static str,
    ) -> Result<Shape, ConfluenceError> {
        serde_json::from_str(&self.body).map_err(|error| {
            ConfluenceError::Refused(format!(
                "Confluence sent {expected} that cannot be read: {error}"
            ))
        })
    }

    fn failure(&self) -> ConfluenceError {
        match self.status {
            StatusCode::UNAUTHORIZED => ConfluenceError::Unauthorized(UNAUTHORIZED_ADVICE),
            StatusCode::FORBIDDEN => ConfluenceError::Unauthorized(
                "Confluence accepted the token but refused the request; check that the account \
                 may use Confluence on this site and see these spaces",
            ),
            StatusCode::BAD_REQUEST => {
                ConfluenceError::Invalid(format!("Confluence: {}", self.message()))
            }
            _ => ConfluenceError::Refused(format!("Confluence: {}", self.message())),
        }
    }

    /// Confluence's own account of what went wrong: `message` in v1's shape, each error's
    /// title and detail in v2's. Neither ever carries a credential.
    fn message(&self) -> String {
        let Ok(body) = serde_json::from_str::<ErrorBody>(&self.body) else {
            return self.status.to_string();
        };
        let mut messages: Vec<String> = body.message.into_iter().collect();
        messages.extend(body.errors.into_iter().map(|error| {
            [error.title, error.detail]
                .into_iter()
                .flatten()
                .collect::<Vec<String>>()
                .join(": ")
        }));
        messages.retain(|message| !message.is_empty());
        if messages.is_empty() {
            return self.status.to_string();
        }
        messages.join("; ")
    }
}

/// A search result as Elysium uses it, or `None` for one it cannot (no space, an unknown type).
fn into_change(result: ResultBody, site_url: &str) -> Option<Change> {
    let content_type = ContentType::parse(&result.content_type)?;
    let space = result.space?;
    let version = result.version.unwrap_or_default();
    let modified_at = version.when.as_deref().and_then(parse_time);
    let modified_by = version.by.and_then(|by| {
        by.account_id.map(|account_id| Person {
            account_id,
            display_name: by.display_name.or(by.public_name).unwrap_or_default(),
        })
    });
    let created_by = result
        .history
        .and_then(|history| history.created_by)
        .and_then(|by| by.account_id);
    let url = result
        .links
        .and_then(|links| links.webui)
        .filter(|webui| webui.starts_with('/'))
        .map(|webui| format!("{site_url}/wiki{webui}"));
    // A page's container is its space, which `space` already names; only a comment's is the
    // page it is on.
    let container = result
        .container
        .filter(|_container| content_type == ContentType::Comment)
        .map(|container| Container {
            id: container.id,
            title: container.title.unwrap_or_default(),
            created_by: container
                .history
                .and_then(|history| history.created_by)
                .and_then(|by| by.account_id),
        });
    Some(Change {
        id: result.id,
        content_type,
        title: result.title.unwrap_or_default(),
        space_name: space.name.unwrap_or_else(|| space.key.clone()),
        space_key: space.key,
        version: version.number.unwrap_or(1),
        modified_at,
        modified_by,
        created_by,
        storage: result
            .body
            .and_then(|body| body.storage)
            .map(|storage| storage.value)
            .unwrap_or_default(),
        url,
        container,
    })
}

/// A Confluence timestamp: RFC 3339 (`2026-11-02T10:00:00.000Z`), or Jira's offset without a
/// colon (`+0000`), which some Atlassian answers use. Anything else is no timestamp.
fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .or_else(|_rfc3339| DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrentUserBody {
    #[serde(default)]
    account_id: String,
    display_name: Option<String>,
    public_name: Option<String>,
    email: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Links {
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpacesBody {
    #[serde(default)]
    results: Vec<SpaceBody>,
    #[serde(rename = "_links", default)]
    links: Links,
}

#[derive(Debug, Deserialize)]
struct SpaceBody {
    id: String,
    key: String,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchBody {
    #[serde(default)]
    results: Vec<ResultBody>,
    #[serde(rename = "_links", default)]
    links: Links,
}

#[derive(Debug, Deserialize)]
struct ResultBody {
    id: String,
    #[serde(rename = "type")]
    content_type: String,
    title: Option<String>,
    space: Option<ResultSpace>,
    version: Option<VersionBody>,
    history: Option<HistoryBody>,
    body: Option<BodyBody>,
    container: Option<ContainerBody>,
    #[serde(rename = "_links")]
    links: Option<ResultLinks>,
}

#[derive(Debug, Deserialize)]
struct ResultSpace {
    key: String,
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct VersionBody {
    number: Option<i64>,
    when: Option<String>,
    by: Option<UserBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserBody {
    account_id: Option<String>,
    display_name: Option<String>,
    public_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryBody {
    created_by: Option<UserBody>,
}

#[derive(Debug, Deserialize)]
struct BodyBody {
    storage: Option<StorageBody>,
}

#[derive(Debug, Deserialize)]
struct StorageBody {
    value: String,
}

/// What a result sits in. Atlassian documents `container` as an open object because its shape
/// follows the content: a comment's is the page it is on, a `Content` whose id is a string, and a
/// page's is its `Space`, whose v1 id is a number (`131075`). Both are read here, and
/// [`into_change`] keeps only a comment's.
#[derive(Debug, Deserialize)]
struct ContainerBody {
    #[serde(deserialize_with = "string_or_number")]
    id: String,
    title: Option<String>,
    history: Option<HistoryBody>,
}

/// An id as text, whether Confluence sent it as a string (content) or a number (a v1 space).
fn string_or_number<'de, Source: Deserializer<'de>>(
    deserializer: Source,
) -> Result<String, Source::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Text(String),
        Number(i64),
    }

    match Wire::deserialize(deserializer)? {
        Wire::Text(text) => Ok(text),
        Wire::Number(number) => Ok(number.to_string()),
    }
}

#[derive(Debug, Deserialize)]
struct ResultLinks {
    webui: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    message: Option<String>,
    #[serde(default)]
    errors: Vec<ErrorEntry>,
}

#[derive(Debug, Deserialize)]
struct ErrorEntry {
    title: Option<String>,
    detail: Option<String>,
}
