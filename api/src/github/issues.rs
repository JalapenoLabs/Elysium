// Copyright © 2026 Jalapeno Labs

//! Issues and pull requests on github.com, and the milestones and labels that group them.
//!
//! GitHub serves pull requests through its issues API too: a pull request is an issue that
//! carries a `pull_request` object, which says when it merged. So one [`Issue`] shape covers
//! both, and one listing reads a repository's changes of either kind.
//!
//! Everything here names a repository by owner and name, checked against GitHub's alphabets
//! by [`Repository`], so no caller input reaches a path unchecked.

use std::fmt;

use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::{Method, StatusCode};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::json;
use url::Url;

use super::{Github, GithubError, Paged, Repository};

/// The most issues one page of a listing holds, which is GitHub's own ceiling.
const PAGE_SIZE: u32 = 100;

/// One issue or pull request, named the way GitHub writes it: `owner/name#12`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRef {
    pub repository: Repository,
    pub number: u64,
}

impl IssueRef {
    /// The issue `owner/name#12` names, or `None` when it names none.
    pub fn parse(reference: &str) -> Option<Self> {
        let (repository, number) = reference.trim().rsplit_once('#')?;
        let repository = Repository::from_url(&format!("https://github.com/{repository}"))?;
        let number = parse_number(number)?;
        Some(Self { repository, number })
    }

    /// The pull request a `https://github.com/owner/name/pull/12` URL points at, or `None`
    /// when it points at anything else.
    pub fn from_pull_request_url(url: &str) -> Option<Self> {
        let path = url.trim().strip_prefix("https://github.com/")?;
        let path = path.trim_end_matches('/');
        let mut segments = path.split('/');
        let (Some(owner), Some(name), Some("pull"), Some(number), None) = (
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
        ) else {
            return None;
        };
        let repository = Repository::from_url(&format!("https://github.com/{owner}/{name}"))?;
        let number = parse_number(number)?;
        Some(Self { repository, number })
    }
}

impl fmt::Display for IssueRef {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}/{}#{}",
            self.repository.owner, self.repository.name, self.number
        )
    }
}

/// An issue, pull request, or milestone number: digits only, greater than zero.
fn parse_number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.chars().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|number| *number > 0)
}

/// An issue or a pull request as GitHub reports it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// `owner/name`, as GitHub spells it.
    pub repository: String,
    pub number: u64,
    pub title: String,
    /// Where a person reads it.
    pub url: String,
    pub is_open: bool,
    /// Why a closed issue closed: `completed` or `not_planned`. GitHub leaves it empty for
    /// pull requests and for issues closed before it recorded reasons.
    pub state_reason: Option<String>,
    /// The login of the first assignee, or `None` when nobody is assigned.
    pub assignee: Option<String>,
    /// `Some` for a pull request, saying whether it merged.
    pub pull_request: Option<PullRequest>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl Issue {
    /// `owner/name#12`.
    pub fn reference(&self) -> String {
        format!("{}#{}", self.repository, self.number)
    }
}

/// What makes an issue a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullRequest {
    pub merged: bool,
}

/// A milestone of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub is_open: bool,
}

/// A label of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    /// The label's issues, where a person reads them.
    pub url: String,
}

/// Which of a repository's issues a listing returns. Closed ones are included unless
/// `open_only` says otherwise, so a listing sees issues that closed.
#[derive(Debug, Clone, Default)]
pub struct IssueQuery<'a> {
    pub open_only: bool,
    /// Only issues updated at or after this moment.
    pub since: Option<DateTime<Utc>>,
    /// Only issues in this milestone.
    pub milestone: Option<u64>,
    /// Only issues carrying this label.
    pub label: Option<&'a str>,
}

impl IssueQuery<'_> {
    /// The query string GitHub's issue listing takes, every value encoded.
    fn parameters(&self) -> String {
        let mut parameters = url::form_urlencoded::Serializer::new(String::new());
        parameters.append_pair("per_page", &PAGE_SIZE.to_string());
        parameters.append_pair("sort", "updated");
        parameters.append_pair("direction", "desc");
        parameters.append_pair("state", if self.open_only { "open" } else { "all" });
        if let Some(since) = self.since {
            parameters.append_pair("since", &since.to_rfc3339_opts(SecondsFormat::Secs, true));
        }
        if let Some(milestone) = self.milestone {
            parameters.append_pair("milestone", &milestone.to_string());
        }
        if let Some(label) = self.label {
            parameters.append_pair("labels", label);
        }
        parameters.finish()
    }
}

impl Github {
    /// One issue or pull request, or `None` when the token cannot see it.
    ///
    /// # Errors
    /// Returns [`GithubError::Unauthorized`] when GitHub rejects the token, and
    /// [`GithubError::Refused`] when it cannot be reached or refuses the call.
    pub async fn issue(
        &self,
        token: &SecretString,
        issue: &IssueRef,
    ) -> Result<Option<Issue>, GithubError> {
        let url = self.repository_url(&issue.repository, &["issues", &issue.number.to_string()]);
        let reply = self.get(&url, token).await?;
        if reply.status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !reply.status.is_success() {
            return Err(reply.refusal());
        }
        let body: IssueBody = reply.read("an issue")?;
        Ok(Some(body.into_issue()))
    }

    /// A repository's issues and pull requests that `query` selects, most recently updated
    /// first, following pages up to `page_limit`.
    ///
    /// # Errors
    /// As [`Github::issue`].
    pub async fn list_issues(
        &self,
        token: &SecretString,
        repository: &Repository,
        query: &IssueQuery<'_>,
        page_limit: usize,
    ) -> Result<Paged<Issue>, GithubError> {
        let path = format!(
            "/repos/{}/{}/issues?{}",
            repository.owner,
            repository.name,
            query.parameters()
        );
        let listing: Paged<IssueBody> = self.paged(token, &path, page_limit, "issues").await?;
        Ok(Paged {
            items: listing
                .items
                .into_iter()
                .map(IssueBody::into_issue)
                .collect(),
            truncated: listing.truncated,
            scopes: listing.scopes,
        })
    }

    /// Whether a pull request merged, from the pull request itself.
    ///
    /// The issues API marks a pull request with `pull_request.merged_at`, but GitHub does not
    /// promise that field on every issue payload, so a closed pull request that does not
    /// say it merged is asked here before it is read as closed without merging.
    ///
    /// # Errors
    /// Returns [`GithubError::Refused`] for a pull request GitHub does not have or will not
    /// show, and otherwise as [`Github::issue`].
    pub async fn pull_request_merged(
        &self,
        token: &SecretString,
        pull_request: &IssueRef,
    ) -> Result<bool, GithubError> {
        let url = self.repository_url(
            &pull_request.repository,
            &["pulls", &pull_request.number.to_string()],
        );
        let reply = self.get(&url, token).await?;
        if !reply.status.is_success() {
            return Err(reply.refusal());
        }
        let body: MergedBody = reply.read("a pull request")?;
        Ok(body.merged)
    }

    /// Closes an issue as completed.
    ///
    /// # Errors
    /// Returns [`GithubError::Refused`] with GitHub's message when it refuses, such as for a
    /// token without write access to the repository, and otherwise as [`Github::issue`].
    pub async fn close_issue(
        &self,
        token: &SecretString,
        issue: &IssueRef,
    ) -> Result<(), GithubError> {
        let url = self.repository_url(&issue.repository, &["issues", &issue.number.to_string()]);
        let body = json!({ "state": "closed", "state_reason": "completed" });
        let reply = self.send(Method::PATCH, &url, token, Some(&body)).await?;
        if !reply.status.is_success() {
            return Err(reply.refusal());
        }
        Ok(())
    }

    /// Writes a comment on an issue or a pull request, as the token's account.
    ///
    /// # Errors
    /// As [`Github::close_issue`].
    pub async fn comment(
        &self,
        token: &SecretString,
        issue: &IssueRef,
        body: &str,
    ) -> Result<(), GithubError> {
        let url = self.repository_url(
            &issue.repository,
            &["issues", &issue.number.to_string(), "comments"],
        );
        let request = json!({ "body": body });
        let reply = self.send(Method::POST, &url, token, Some(&request)).await?;
        if !reply.status.is_success() {
            return Err(reply.refusal());
        }
        Ok(())
    }

    /// One milestone, or `None` when the token cannot see it.
    ///
    /// # Errors
    /// As [`Github::issue`].
    pub async fn milestone(
        &self,
        token: &SecretString,
        repository: &Repository,
        number: u64,
    ) -> Result<Option<Milestone>, GithubError> {
        let url = self.repository_url(repository, &["milestones", &number.to_string()]);
        let reply = self.get(&url, token).await?;
        if reply.status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !reply.status.is_success() {
            return Err(reply.refusal());
        }
        let body: MilestoneBody = reply.read("a milestone")?;
        Ok(Some(body.into_milestone()))
    }

    /// A repository's open milestones.
    ///
    /// # Errors
    /// As [`Github::issue`].
    pub async fn list_milestones(
        &self,
        token: &SecretString,
        repository: &Repository,
        page_limit: usize,
    ) -> Result<Paged<Milestone>, GithubError> {
        let path = format!(
            "/repos/{}/{}/milestones?state=open&per_page={PAGE_SIZE}",
            repository.owner, repository.name
        );
        let listing: Paged<MilestoneBody> =
            self.paged(token, &path, page_limit, "milestones").await?;
        Ok(Paged {
            items: listing
                .items
                .into_iter()
                .map(MilestoneBody::into_milestone)
                .collect(),
            truncated: listing.truncated,
            scopes: listing.scopes,
        })
    }

    /// One label by name, or `None` when the repository has no such label.
    ///
    /// # Errors
    /// As [`Github::issue`].
    pub async fn label(
        &self,
        token: &SecretString,
        repository: &Repository,
        name: &str,
    ) -> Result<Option<Label>, GithubError> {
        let url = self.repository_url(repository, &["labels", name]);
        let reply = self.get(&url, token).await?;
        if reply.status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !reply.status.is_success() {
            return Err(reply.refusal());
        }
        let body: LabelBody = reply.read("a label")?;
        Ok(Some(label_of(repository, body.name)))
    }

    /// A repository's labels.
    ///
    /// # Errors
    /// As [`Github::issue`].
    pub async fn list_labels(
        &self,
        token: &SecretString,
        repository: &Repository,
        page_limit: usize,
    ) -> Result<Paged<Label>, GithubError> {
        let path = format!(
            "/repos/{}/{}/labels?per_page={PAGE_SIZE}",
            repository.owner, repository.name
        );
        let listing: Paged<LabelBody> = self.paged(token, &path, page_limit, "labels").await?;
        Ok(Paged {
            items: listing
                .items
                .into_iter()
                .map(|label| label_of(repository, label.name))
                .collect(),
            truncated: listing.truncated,
            scopes: listing.scopes,
        })
    }

    /// The API URL of a path under a repository, each segment percent-encoded, so a label
    /// name with a slash or a space stays one segment.
    fn repository_url(&self, repository: &Repository, segments: &[&str]) -> String {
        let mut url = Url::parse(&self.url("/")).expect("the API origin is a valid URL");
        url.path_segments_mut()
            .expect("the API origin is a base URL")
            .clear()
            .extend(["repos", &repository.owner, &repository.name])
            .extend(segments);
        url.into()
    }
}

/// A label with the URL of its issues on github.com.
fn label_of(repository: &Repository, name: String) -> Label {
    let mut url = Url::parse("https://github.com/").expect("github.com is a valid URL");
    url.path_segments_mut()
        .expect("github.com is a base URL")
        .clear()
        .extend([
            repository.owner.as_str(),
            repository.name.as_str(),
            "labels",
            name.as_str(),
        ]);
    Label {
        name,
        url: url.into(),
    }
}

/// The parts of an issue or pull request Elysium reads.
#[derive(Deserialize)]
struct IssueBody {
    number: u64,
    #[serde(default)]
    title: String,
    html_url: String,
    /// `https://api.github.com/repos/owner/name`, the one place the answer names its
    /// repository.
    repository_url: String,
    state: String,
    state_reason: Option<String>,
    assignee: Option<LoginBody>,
    pull_request: Option<PullRequestBody>,
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct LoginBody {
    login: String,
}

#[derive(Deserialize)]
struct PullRequestBody {
    merged_at: Option<DateTime<Utc>>,
}

impl IssueBody {
    fn into_issue(self) -> Issue {
        let repository = self
            .repository_url
            .rsplit_once("/repos/")
            .map(|(_origin, full_name)| full_name.to_owned())
            .unwrap_or_default();
        Issue {
            repository,
            number: self.number,
            title: self.title,
            url: self.html_url,
            is_open: self.state == "open",
            state_reason: self.state_reason,
            assignee: self.assignee.map(|assignee| assignee.login),
            pull_request: self.pull_request.map(|pull_request| PullRequest {
                merged: pull_request.merged_at.is_some(),
            }),
            updated_at: self.updated_at,
        }
    }
}

/// The part of `GET /repos/{owner}/{repo}/pulls/{number}` Elysium reads.
#[derive(Deserialize)]
struct MergedBody {
    #[serde(default)]
    merged: bool,
}

#[derive(Deserialize)]
struct MilestoneBody {
    number: u64,
    #[serde(default)]
    title: String,
    html_url: String,
    state: String,
}

impl MilestoneBody {
    fn into_milestone(self) -> Milestone {
        Milestone {
            number: self.number,
            title: self.title,
            url: self.html_url,
            is_open: self.state == "open",
        }
    }
}

#[derive(Deserialize)]
struct LabelBody {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issues_are_named_owner_name_and_number() {
        let parsed = IssueRef::parse("JalapenoLabs/Elysium#12").expect("a reference");
        assert_eq!(parsed.repository.owner, "JalapenoLabs");
        assert_eq!(parsed.repository.name, "Elysium");
        assert_eq!(parsed.number, 12);
        assert_eq!(parsed.to_string(), "JalapenoLabs/Elysium#12");

        for refused in [
            "JalapenoLabs/Elysium",
            "JalapenoLabs/Elysium#",
            "JalapenoLabs/Elysium#0",
            "JalapenoLabs/Elysium#-1",
            "JalapenoLabs/Elysium#1a",
            "Elysium#12",
            "../..#12",
            "a/b/c#12",
        ] {
            assert_eq!(IssueRef::parse(refused), None, "{refused}");
        }
    }

    #[test]
    fn a_pull_request_url_names_its_pull_request_and_nothing_else() {
        let parsed =
            IssueRef::from_pull_request_url("https://github.com/JalapenoLabs/Elysium/pull/15")
                .expect("a pull request");
        assert_eq!(parsed.to_string(), "JalapenoLabs/Elysium#15");
        assert_eq!(
            IssueRef::from_pull_request_url("https://github.com/JalapenoLabs/Elysium/pull/15/"),
            Some(parsed)
        );

        for refused in [
            "https://github.com/JalapenoLabs/Elysium/issues/15",
            "https://github.com/JalapenoLabs/Elysium/pull/15/files",
            "https://github.com/JalapenoLabs/Elysium/pull/",
            "http://github.com/JalapenoLabs/Elysium/pull/15",
            "https://github.com.evil.com/JalapenoLabs/Elysium/pull/15",
            "JalapenoLabs/Elysium#15",
        ] {
            assert_eq!(IssueRef::from_pull_request_url(refused), None, "{refused}");
        }
    }

    #[test]
    fn path_segments_are_encoded_so_a_label_stays_one_segment() {
        let github = Github::new(reqwest::Client::new());
        let repository = Repository {
            owner: "JalapenoLabs".to_owned(),
            name: "Elysium".to_owned(),
        };

        assert_eq!(
            github.repository_url(&repository, &["labels", "good first/issue"]),
            "https://api.github.com/repos/JalapenoLabs/Elysium/labels/good%20first%2Fissue"
        );
        assert_eq!(
            label_of(&repository, "good first/issue".to_owned()).url,
            "https://github.com/JalapenoLabs/Elysium/labels/good%20first%2Fissue"
        );
    }
}
