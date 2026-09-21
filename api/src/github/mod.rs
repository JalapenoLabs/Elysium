// Copyright © 2026 Jalapeno Labs

//! GitHub's REST API, reached through one [`Github`] handle.
//!
//! Elysium authenticates with a personal access token, classic or fine-grained: both are
//! sent as a bearer token, and the API tells them apart only by what it answers with. A
//! classic token's scopes come back in `X-OAuth-Scopes`; a fine-grained token's
//! permissions are per repository and are not reported, so that header is absent.
//!
//! The host is fixed here, `api.github.com`, so a stored credential carries no endpoint.
//! GitHub Enterprise Server, which lives on a customer's own host, is not supported.
//!
//! GitHub refuses any request without a `User-Agent`; the shared client sets Elysium's.
//!
//! Issues and pull requests, with the milestones and labels that group them, are in
//! [`issues`]: what action item links read and write.

#[cfg(test)]
pub(crate) mod fake;
pub mod issues;

use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, Utc};
use reqwest::header::ACCEPT;
use reqwest::{Method, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{Level, event};

/// The authenticated user, the one call that proves a token works and says whose it is.
const USER_PATH: &str = "/user";

/// The first page of the repositories a token can see, most recently pushed first.
///
/// With no `type`, GitHub already lists the account's own repositories, the ones it
/// collaborates on, and its organizations' repositories; an `affiliation` naming all three
/// changed nothing when measured. A hundred is the most GitHub sends per page.
const USER_REPOSITORIES_PATH: &str = "/user/repos?per_page=100&sort=pushed";

/// How many pages of repositories a listing follows, so 1,000 repositories at most.
///
/// Pages are fetched one after another, each 0.7 to 2.5 seconds when measured, and the whole
/// listing answers one request under the API's 30-second handler deadline (`408` past it). An
/// account near the cap on a slow GitHub can reach that deadline; lower this before raising
/// the deadline. An account past the cap is told the listing was cut short and can still add a
/// repository by its URL.
const REPOSITORY_PAGE_LIMIT: usize = 10;

/// Where every call goes. GitHub's `Link` header is followed only on this host, so the token
/// is never sent anywhere else.
const API_ORIGIN: &str = "https://api.github.com";

/// The REST API version these calls are written against. GitHub keeps older versions
/// working, so pinning one means a new default never changes an answer under Elysium.
const API_VERSION: &str = "2022-11-28";

/// How long one call may take. `GET /user` answers in well under a second.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// GitHub refused a call or could not be reached.
#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    /// The token is wrong, expired, or revoked. The message says what to check.
    #[error("{0}")]
    Unauthorized(&'static str),
    #[error("{0}")]
    Refused(String),
}

/// Who a token belongs to and what it may do, as GitHub reports it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// The account the token acts as, such as `octocat`.
    pub login: String,
    /// A classic token's scopes. Empty for a fine-grained token, whose permissions are
    /// granted per repository and are not reported over the API.
    pub scopes: Vec<String>,
    /// When GitHub stops accepting the token, or `None` for one that does not expire.
    pub token_expires_at: Option<DateTime<Utc>>,
}

/// GitHub's error body, such as `{"message":"Bad credentials"}`.
#[derive(Deserialize)]
struct ErrorBody {
    message: String,
}

/// The parts of `GET /user` Elysium keeps.
#[derive(Deserialize)]
struct UserBody {
    login: String,
}

/// The scopes in an `X-OAuth-Scopes` header, such as `repo, read:org`. An empty header
/// means a classic token with no scopes at all.
fn parse_scopes(header: &str) -> Vec<String> {
    header
        .split(',')
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The instant in a `github-authentication-token-expiration` header.
///
/// GitHub writes it as `2026-12-31 23:59:59 UTC`, and as an offset such as
/// `2026-12-31 23:59:59 +0100` for a token created under one. A header in neither shape
/// reads as no expiry: when the token expires is worth showing, but not worth refusing a
/// working token over.
fn parse_expiration(header: &str) -> Option<DateTime<Utc>> {
    let value = header.trim();
    if value.is_empty() {
        return None;
    }

    if let Ok(offset) = DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S %z") {
        return Some(offset.with_timezone(&Utc));
    }
    let utc = value.strip_suffix(" UTC").unwrap_or(value);
    if let Ok(naive) = NaiveDateTime::parse_from_str(utc, "%Y-%m-%d %H:%M:%S") {
        return Some(naive.and_utc());
    }

    event!(
        name: "github.expiration.unreadable",
        Level::DEBUG,
        github.token.expiration = value,
        "GitHub sent a token expiration in an unknown format",
    );
    None
}

/// A repository on github.com, named the way its URL names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub owner: String,
    pub name: String,
}

impl Repository {
    /// The repository a git remote URL points at, if it is on github.com.
    ///
    /// Accepts `https://github.com/owner/name`, `ssh://git@github.com/owner/name`, and
    /// `git@github.com:owner/name`, each with or without `.git` and a trailing slash. The
    /// owner and name are checked against GitHub's own alphabets, since both become part
    /// of an API path.
    pub fn from_url(url: &str) -> Option<Self> {
        let path = [
            "https://github.com/",
            "ssh://git@github.com/",
            "git@github.com:",
        ]
        .iter()
        .find_map(|prefix| url.strip_prefix(prefix))?;
        let path = path.trim_end_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        let (owner, name) = path.split_once('/')?;

        // GitHub logins: letters, digits, and hyphens, up to 39.
        let is_owner = (1..=39).contains(&owner.len())
            && owner
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-');
        // Repository names: letters, digits, hyphens, underscores, and dots, up to 100,
        // never `.` or `..`.
        let is_name = (1..=100).contains(&name.len())
            && name != "."
            && name != ".."
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            });
        if !is_owner || !is_name {
            return None;
        }
        Some(Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }
}

/// What a token can see of one repository.
#[derive(Debug, Clone)]
pub struct RepositoryAccess {
    /// `owner/name`, as GitHub spells it.
    pub full_name: String,
    pub is_private: bool,
    /// Whether the token's *account* may push. For a classic token its scopes decide the
    /// rest; a fine-grained token's own permissions are not reported, so for one this says
    /// nothing about the token.
    pub role_can_push: bool,
    /// A classic token's scopes; empty for a fine-grained token.
    pub scopes: Vec<String>,
}

/// The parts of `GET /repos/{owner}/{repo}` Elysium reads.
#[derive(Deserialize)]
struct RepositoryBody {
    full_name: String,
    private: bool,
    /// Present whenever the request is authenticated.
    permissions: Option<RepositoryPermissions>,
}

#[derive(Deserialize)]
struct RepositoryPermissions {
    push: bool,
}

/// One repository a token can see, as a listing reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedRepository {
    /// `owner/name`, as GitHub spells it.
    pub full_name: String,
    pub owner: String,
    pub name: String,
    pub is_private: bool,
    pub is_archived: bool,
    pub default_branch: String,
    /// The `https://github.com/` URL to clone from.
    pub clone_url: String,
    /// When anything was last pushed, or `None` for a repository nothing was ever pushed to.
    pub pushed_at: Option<DateTime<Utc>>,
    /// Whether the token's *account* may push, with the same caveats as
    /// [`RepositoryAccess::role_can_push`].
    pub role_can_push: bool,
}

/// The repositories a token can see, most recently pushed first.
#[derive(Debug, Clone)]
pub struct RepositoryListing {
    pub repositories: Vec<ListedRepository>,
    /// Whether GitHub had more than [`REPOSITORY_PAGE_LIMIT`] pages to send.
    pub truncated: bool,
    /// A classic token's scopes; empty for a fine-grained token.
    pub scopes: Vec<String>,
}

/// The parts of one repository in `GET /user/repos` Elysium reads.
#[derive(Deserialize)]
struct ListedRepositoryBody {
    full_name: String,
    name: String,
    owner: OwnerBody,
    private: bool,
    archived: bool,
    default_branch: String,
    clone_url: String,
    pushed_at: Option<DateTime<Utc>>,
    permissions: Option<RepositoryPermissions>,
}

#[derive(Deserialize)]
struct OwnerBody {
    login: String,
}

/// The `rel="next"` URL in a `Link` header, such as
/// `<https://api.github.com/user/repos?page=2>; rel="next", <...>; rel="last"`.
fn next_page_url(header: &str) -> Option<&str> {
    header.split(',').find_map(|link| {
        let (target, parameters) = link.split_once(';')?;
        let is_next = parameters
            .split(';')
            .any(|parameter| parameter.trim() == r#"rel="next""#);
        if !is_next {
            return None;
        }
        target
            .trim()
            .strip_prefix('<')
            .and_then(|target| target.strip_suffix('>'))
    })
}

/// A page-capped listing, with the scopes GitHub reported on its last page.
#[derive(Debug, Clone)]
pub struct Paged<Item> {
    pub items: Vec<Item>,
    /// Whether GitHub had more pages than the cap allowed.
    pub truncated: bool,
    /// A classic token's scopes; empty for a fine-grained token.
    pub scopes: Vec<String>,
}

/// GitHub's API client. Cheap to clone; clones share one HTTP client.
#[derive(Debug, Clone)]
pub struct Github {
    http: reqwest::Client,
    /// Where calls go: [`API_ORIGIN`], which only a test's fake replaces. Production code
    /// has no way to set it.
    origin: String,
}

impl Github {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            origin: API_ORIGIN.to_owned(),
        }
    }

    /// A client that sends every call to `origin`, such as a local fake.
    #[cfg(test)]
    pub const fn with_test_origin(http: reqwest::Client, origin: String) -> Self {
        Self { http, origin }
    }

    /// The URL of an API path, such as `/user`.
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }

    /// Checks a token with GitHub and reports whose it is, what it may do, and when it
    /// expires.
    ///
    /// # Errors
    /// Returns [`GithubError::Unauthorized`] when GitHub rejects the token, and
    /// [`GithubError::Refused`] when it cannot be reached or refuses the call, such as
    /// for a token an organization has blocked.
    pub async fn verify(&self, token: &SecretString) -> Result<Account, GithubError> {
        let reply = self.get(&self.url(USER_PATH), token).await?;
        if !reply.status.is_success() {
            return Err(reply.refusal());
        }

        let user: UserBody = reply.read("an account")?;
        Ok(Account {
            login: user.login,
            scopes: reply.scopes,
            token_expires_at: reply.token_expires_at,
        })
    }

    /// What a token can do with one repository, or `None` when the token cannot see it.
    ///
    /// GitHub answers `404`, not `403`, for a private repository a token has no access
    /// to, so "cannot see" and "does not exist" are the same answer here.
    ///
    /// # Errors
    /// Returns [`GithubError::Unauthorized`] when GitHub rejects the token, and
    /// [`GithubError::Refused`] when it cannot be reached or refuses the call.
    pub async fn repository_access(
        &self,
        token: &SecretString,
        repository: &Repository,
    ) -> Result<Option<RepositoryAccess>, GithubError> {
        let path = format!("/repos/{}/{}", repository.owner, repository.name);
        let reply = self.get(&self.url(&path), token).await?;
        if reply.status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !reply.status.is_success() {
            return Err(reply.refusal());
        }

        let body: RepositoryBody = reply.read("a repository")?;
        Ok(Some(RepositoryAccess {
            full_name: body.full_name,
            is_private: body.private,
            role_can_push: body.permissions.is_some_and(|permissions| permissions.push),
            scopes: reply.scopes,
        }))
    }

    /// The repositories a token can see, following GitHub's pages up to
    /// [`REPOSITORY_PAGE_LIMIT`].
    ///
    /// For a fine-grained token GitHub lists the repositories the token was granted under
    /// its one resource owner, plus the account's own public repositories. Other owners'
    /// public repositories, which the token can still clone, are not listed.
    ///
    /// # Errors
    /// Returns [`GithubError::Unauthorized`] when GitHub rejects the token, and
    /// [`GithubError::Refused`] when it cannot be reached, refuses a page, or points the
    /// next page at another host.
    pub async fn list_repositories(
        &self,
        token: &SecretString,
    ) -> Result<RepositoryListing, GithubError> {
        let listing: Paged<ListedRepositoryBody> = self
            .paged(
                token,
                USER_REPOSITORIES_PATH,
                REPOSITORY_PAGE_LIMIT,
                "repositories",
            )
            .await?;

        Ok(RepositoryListing {
            repositories: listing
                .items
                .into_iter()
                .map(|body| ListedRepository {
                    full_name: body.full_name,
                    owner: body.owner.login,
                    name: body.name,
                    is_private: body.private,
                    is_archived: body.archived,
                    default_branch: body.default_branch,
                    clone_url: body.clone_url,
                    pushed_at: body.pushed_at,
                    role_can_push: body.permissions.is_some_and(|permissions| permissions.push),
                })
                .collect(),
            truncated: listing.truncated,
            scopes: listing.scopes,
        })
    }

    /// Follows a listing's pages from `first_path` up to `page_limit` of them, reporting
    /// whether GitHub had more. `what` names the items for an error message.
    ///
    /// # Errors
    /// Returns [`GithubError::Unauthorized`] when GitHub rejects the token, and
    /// [`GithubError::Refused`] when it cannot be reached, refuses a page, sends one that
    /// cannot be read, or points the next page at another host.
    async fn paged<Item: DeserializeOwned>(
        &self,
        token: &SecretString,
        first_path: &str,
        page_limit: usize,
        what: &'static str,
    ) -> Result<Paged<Item>, GithubError> {
        let mut items = Vec::new();
        let mut scopes = Vec::new();
        let mut next = Some(self.url(first_path));
        let origin = self.url("/");

        for _page in 0..page_limit {
            let Some(url) = next.take() else {
                break;
            };
            if !url.starts_with(&origin) {
                return Err(GithubError::Refused(format!(
                    "GitHub pointed the next page of {what} at another host"
                )));
            }

            let reply = self.get(&url, token).await?;
            if !reply.status.is_success() {
                return Err(reply.refusal());
            }
            let mut page: Vec<Item> = reply.read(what)?;
            items.append(&mut page);
            scopes = reply.scopes;
            next = reply.next_page;
        }

        Ok(Paged {
            items,
            truncated: next.is_some(),
            scopes,
        })
    }

    /// Sends one authenticated `GET` and reads everything a caller may need from it.
    async fn get(&self, url: &str, token: &SecretString) -> Result<Reply, GithubError> {
        self.send(Method::GET, url, token, None).await
    }

    /// Sends one authenticated call, with a JSON body when there is one, and reads
    /// everything a caller may need from the answer.
    ///
    /// # Errors
    /// Returns [`GithubError::Unauthorized`] when GitHub rejects the token, and
    /// [`GithubError::Refused`] when it cannot be reached. Any other status comes back in
    /// the [`Reply`] for the caller to judge.
    async fn send(
        &self,
        method: Method,
        url: &str,
        token: &SecretString,
        body: Option<&Value>,
    ) -> Result<Reply, GithubError> {
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(token.expose_secret())
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .timeout(REQUEST_TIMEOUT);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GithubError::Refused(format!("GitHub: {error}")))?;

        let status = response.status();
        // Headers are read before the body, which consumes the response.
        let scopes = response
            .headers()
            .get("x-oauth-scopes")
            .and_then(|value| value.to_str().ok())
            .map(parse_scopes)
            .unwrap_or_default();
        let token_expires_at = response
            .headers()
            .get("github-authentication-token-expiration")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_expiration);
        let next_page = response
            .headers()
            .get("link")
            .and_then(|value| value.to_str().ok())
            .and_then(next_page_url)
            .map(str::to_owned);
        let body = response
            .text()
            .await
            .map_err(|error| GithubError::Refused(format!("GitHub: {error}")))?;

        if status == StatusCode::UNAUTHORIZED {
            return Err(GithubError::Unauthorized(
                "GitHub refused the token; check that it was copied whole and has not expired or been revoked",
            ));
        }
        Ok(Reply {
            status,
            scopes,
            token_expires_at,
            next_page,
            body,
        })
    }
}

/// One answer from GitHub, read in full.
struct Reply {
    status: StatusCode,
    scopes: Vec<String>,
    token_expires_at: Option<DateTime<Utc>>,
    /// The next page of a paged answer, from the `Link` header.
    next_page: Option<String>,
    body: String,
}

impl Reply {
    /// The answer parsed as `Shape`, or a refusal naming what could not be read.
    fn read<Shape: DeserializeOwned>(&self, what: &'static str) -> Result<Shape, GithubError> {
        serde_json::from_str(&self.body).map_err(|error| {
            GithubError::Refused(format!("GitHub sent {what} that cannot be read: {error}"))
        })
    }

    /// An unsuccessful answer as an error carrying GitHub's own message.
    fn refusal(&self) -> GithubError {
        let message = serde_json::from_str::<ErrorBody>(&self.body)
            .map_or_else(|_error| self.status.to_string(), |error| error.message);
        GithubError::Refused(format!("GitHub: {message}"))
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn expirations_parse_in_utc_and_under_an_offset() {
        let midnight = Utc.with_ymd_and_hms(2026, 12, 31, 23, 59, 59).unwrap();

        assert_eq!(parse_expiration("2026-12-31 23:59:59 UTC"), Some(midnight));
        assert_eq!(parse_expiration("2026-12-31T23:59:59"), None);
        assert_eq!(
            parse_expiration("2027-01-01 00:59:59 +0100"),
            Some(midnight)
        );
        assert_eq!(parse_expiration(""), None);
        assert_eq!(parse_expiration("never"), None);
    }

    #[test]
    fn repositories_are_read_from_every_github_remote_form() {
        let elysium = Some(Repository {
            owner: "JalapenoLabs".to_owned(),
            name: "Elysium".to_owned(),
        });
        for url in [
            "https://github.com/JalapenoLabs/Elysium",
            "https://github.com/JalapenoLabs/Elysium.git",
            "https://github.com/JalapenoLabs/Elysium/",
            "git@github.com:JalapenoLabs/Elysium.git",
            "ssh://git@github.com/JalapenoLabs/Elysium.git",
        ] {
            assert_eq!(Repository::from_url(url), elysium, "{url}");
        }

        for url in [
            "https://gitlab.com/JalapenoLabs/Elysium.git",
            "https://github.com/JalapenoLabs",
            "https://github.com/JalapenoLabs/Elysium/pulls",
            "https://github.com/../..",
            "https://github.com/owner/name?x=1",
            "https://github.com.evil.com/owner/name",
        ] {
            assert_eq!(Repository::from_url(url), None, "{url}");
        }
    }

    #[test]
    fn the_next_page_is_read_from_the_link_header() {
        let middle = concat!(
            r#"<https://api.github.com/user/repos?page=1>; rel="prev", "#,
            r#"<https://api.github.com/user/repos?page=3>; rel="next", "#,
            r#"<https://api.github.com/user/repos?page=9>; rel="last""#,
        );
        assert_eq!(
            next_page_url(middle),
            Some("https://api.github.com/user/repos?page=3")
        );

        let last = r#"<https://api.github.com/user/repos?page=8>; rel="prev""#;
        assert_eq!(next_page_url(last), None);
        assert_eq!(next_page_url(""), None);
    }

    #[test]
    fn scopes_are_split_and_an_empty_header_means_none() {
        assert_eq!(
            parse_scopes("repo, read:org,workflow"),
            vec!["repo", "read:org", "workflow"]
        );
        assert!(parse_scopes("").is_empty());
        assert!(parse_scopes(" , ").is_empty());
    }
}
