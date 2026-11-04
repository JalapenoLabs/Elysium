// Copyright © 2026 Jalapeno Labs

//! Problems people report from the app, filed on Elysium's own GitHub repository as them.
//!
//! When something fails and the person believes it is not their doing, Report turns it into an
//! issue on [`REPOSITORY_OWNER`]/[`REPOSITORY_NAME`], posted with one of their own GitHub tokens
//! so it is filed under their name. See `docs/issue-reporting.md`.
//!
//! - **What is reported** is a [`Subject`]: something Elysium holds, such as a connector's
//!   failed check, read from its own records; or an error any other surface showed.
//! - **What is posted** is a [`Draft`], rendered by [`prepare`] from the problem, the build, and
//!   the browser, with everything personal redacted ([`redact`]). The person sees exactly that
//!   before they confirm, since rendering is deterministic.
//! - **One issue per problem.** A problem's fingerprint is the same for everyone who hits it.
//!   [`IssueReports::file`] posts to the open issue already carrying it, as a comment, or opens
//!   one. The issue is found from the last report this deployment filed for the fingerprint
//!   first, then by GitHub's search, whose index lags new issues by up to a minute or so.
//! - **Limits.** Each person may file [`REPORT_LIMIT`] reports a day, and reports the same
//!   problem to the same open issue once.

pub mod browser;
pub mod draft;
#[cfg(test)]
mod filing_tests;
mod fingerprint;
pub mod redact;
pub mod subject;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::Serialize;
use uuid::Uuid;

use self::draft::{Draft, Environment, fingerprint_line};
use self::redact::Redactor;
use self::subject::Subject;
use crate::connectors::checks::CheckRuns;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::github::issues::IssueRef;
use crate::github::{Github, GithubError, Repository};
use crate::models::github_credential::{GithubCredential, GithubTokenKind};
use crate::models::issue_report::{self, Filing, IssueReport};
use crate::version::VersionInfo;

/// The repository reports are filed on. Fixed here, like GitHub's own host: no request names
/// where a report goes.
pub const REPOSITORY_OWNER: &str = "JalapenoLabs";
pub const REPOSITORY_NAME: &str = "Elysium";

/// How many reports one person may file in [`REPORT_WINDOW`]. Enough for a bad day with several
/// broken connectors; few enough that nobody floods the repository, by accident or not.
pub const REPORT_LIMIT: i64 = 10;

/// The rolling window [`REPORT_LIMIT`] counts over.
pub const REPORT_WINDOW: Duration = Duration::from_hours(24);

/// The repository reports are filed on.
pub fn repository() -> Repository {
    Repository {
        owner: REPOSITORY_OWNER.to_owned(),
        name: REPOSITORY_NAME.to_owned(),
    }
}

/// Whether a token can file a report, as far as Elysium can tell before trying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    /// A classic token with `public_repo` or `repo`, which may open issues on any public
    /// repository.
    Ready,
    /// A fine-grained token. It may open issues only on repositories its one resource owner
    /// holds, and GitHub does not report a fine-grained token's permissions, so it can only be
    /// tried. For anyone outside the repository's organization it will be refused.
    FineGrained,
    /// A classic token without `public_repo` or `repo`, which GitHub will refuse.
    MissingScope,
    /// A token past its expiry, which GitHub will refuse.
    Expired,
}

impl Readiness {
    /// Whether a report is sent with the token at all.
    pub const fn may_try(self) -> bool {
        matches!(self, Self::Ready | Self::FineGrained)
    }
}

/// Whether `credential` can file a report at `now`.
pub fn readiness(credential: &GithubCredential, now: DateTime<Utc>) -> Readiness {
    if credential
        .token_expires_at
        .is_some_and(|expires_at| expires_at <= now)
    {
        return Readiness::Expired;
    }
    match credential.kind {
        GithubTokenKind::FineGrained => Readiness::FineGrained,
        GithubTokenKind::Classic => {
            let has_scope = credential
                .scopes
                .split(',')
                .map(str::trim)
                .any(|scope| scope == "repo" || scope == "public_repo");
            if has_scope {
                Readiness::Ready
            } else {
                Readiness::MissingScope
            }
        }
    }
}

/// The token a report should be filed with, of `credentials`, which are the reporter's own: the
/// workspace default when it may be tried, else the first ready one, else the first that may be
/// tried.
pub fn suggested_credential(credentials: &[GithubCredential], now: DateTime<Utc>) -> Option<Uuid> {
    let may_try = |credential: &&GithubCredential| readiness(credential, now).may_try();
    credentials
        .iter()
        .filter(may_try)
        .find(|credential| credential.is_default)
        .or_else(|| {
            credentials
                .iter()
                .find(|credential| readiness(credential, now) == Readiness::Ready)
        })
        .or_else(|| credentials.iter().find(may_try))
        .map(|credential| credential.id)
}

/// What was running when a report was made: the build `version` describes, and the browser
/// `user_agent` names.
pub fn environment(version: &VersionInfo, user_agent: Option<&str>) -> Environment {
    let elysium = match version.release {
        Some(release) => format!("{release} (release)"),
        None => format!("{} ({} build)", version.version, version.profile),
    };
    let build = match &version.git.commit {
        Some(commit) if version.git.branch.is_empty() => {
            format!("{}, built {}", commit.short_hash, version.built_at)
        }
        Some(commit) => format!(
            "{} on {}, built {}",
            commit.short_hash, version.git.branch, version.built_at
        ),
        None => format!("no commit recorded, built {}", version.built_at),
    };
    Environment {
        elysium,
        build,
        browser: browser::describe(user_agent),
    }
}

/// The report of `subject` as `person` would file it, with their `note`.
///
/// # Errors
/// As [`Subject::validate`] and [`subject::gather`], and [`ApiError::Internal`] for a database
/// failure.
pub async fn prepare(
    database: &Pool,
    check_runs: &CheckRuns,
    person: Uuid,
    subject: &Subject,
    note: Option<&str>,
    environment: &Environment,
) -> Result<Draft, ApiError> {
    subject.validate()?;
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let problem = subject::gather(&mut connection, check_runs, person, subject).await?;
    let redactor = Redactor::new(issue_report::personal_values(&mut connection).await?);
    Ok(draft::render(&problem, environment, note, &redactor))
}

/// Where a report went.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiledReport {
    pub filed_as: Filing,
    pub issue_number: u64,
    pub issue_url: String,
    /// Where this report reads: the issue, or the comment on it.
    pub url: String,
}

/// Files reports. Cheap to clone; clones share one lock.
#[derive(Debug, Clone, Default)]
pub struct IssueReports {
    /// Held while a report is filed, so two reports of one problem made at the same moment
    /// cannot both find no issue and both open one. The API runs as one replica, so a lock in
    /// the process is a lock for the deployment; reports are rare, so waiting costs nothing.
    filing: Arc<tokio::sync::Mutex<()>>,
}

impl IssueReports {
    /// Files `draft` as `person` with `token`: a comment on the open issue already carrying its
    /// fingerprint, or a new issue.
    ///
    /// # Errors
    /// [`ApiError::TooManyRequests`] past the daily limit; [`ApiError::CodedConflict`]
    /// `already_reported` when the person already reported this problem to its open issue;
    /// [`ApiError::CodedBadRequest`] `github_token_cannot_report` when GitHub refuses the token
    /// the write; and otherwise as GitHub and the database fail.
    pub async fn file(
        &self,
        database: &Pool,
        github: &Github,
        person: Uuid,
        token: &SecretString,
        draft: &Draft,
    ) -> Result<FiledReport, ApiError> {
        let _filing = self.filing.lock().await;
        let mut connection = database
            .get()
            .await
            .context("no database connection available")?;

        let window = chrono::Duration::from_std(REPORT_WINDOW).context("the window fits")?;
        let now = Utc::now();
        let (made, oldest) =
            issue_report::made_since(&mut connection, person, now - window).await?;
        if made >= REPORT_LIMIT {
            let resets_at = oldest.map_or(now, |oldest| oldest + window);
            return Err(ApiError::TooManyRequests {
                code: "report_limit",
                message: format!(
                    "a person may file {REPORT_LIMIT} reports a day; the next can be filed at {}",
                    resets_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                ),
            });
        }

        let repository = repository();
        let latest = issue_report::latest_of(&mut connection, &draft.fingerprint).await?;
        let open = open_issue(
            github,
            token,
            &repository,
            latest.as_ref(),
            &draft.fingerprint,
        )
        .await
        .map_err(cannot_report)?;

        let (filed_as, issue_number, issue_url, url) = if let Some((issue_number, issue_url)) = open
        {
            let previous =
                issue_report::latest_by(&mut connection, person, &draft.fingerprint).await?;
            let reported_here = previous.is_some_and(|previous| {
                u64::try_from(previous.issue_number).is_ok_and(|number| number == issue_number)
            });
            if reported_here {
                return Err(ApiError::CodedConflict {
                    code: "already_reported",
                    message: "you already reported this problem to its open issue",
                });
            }
            let issue = IssueRef {
                repository,
                number: issue_number,
            };
            let url = github
                .comment(token, &issue, &draft.body)
                .await
                .map_err(cannot_report)?;
            (Filing::Comment, issue_number, issue_url, url)
        } else {
            let created = github
                .create_issue(token, &repository, &draft.title, &draft.body)
                .await
                .map_err(cannot_report)?;
            let url = created.url.clone();
            (Filing::Issue, created.number, created.url, url)
        };

        // GitHub holds the report now; the row only remembers where it went. A row that fails
        // to store loses the limit's count and the shortcut for one report, never the report.
        let stored = IssueReport {
            id: Uuid::now_v7(),
            fingerprint: draft.fingerprint.clone(),
            issue_number: i64::try_from(issue_number).context("an issue number fits")?,
            issue_url: issue_url.clone(),
            filed_as,
            url: url.clone(),
            created_by: person,
            created_at: Utc::now(),
        };
        if let Err(error) = issue_report::record(&mut connection, &stored).await {
            tracing::event!(
                name: "issue_reports.record.failed",
                tracing::Level::ERROR,
                error.message = %error,
                issue.number = issue_number,
                "a report reached GitHub but could not be remembered",
            );
        }
        tracing::event!(
            name: "issue_reports.filed",
            tracing::Level::INFO,
            issue.number = issue_number,
            report.filed_as = ?filed_as,
            report.fingerprint = %draft.fingerprint,
            "a problem report was filed",
        );

        Ok(FiledReport {
            filed_as,
            issue_number,
            issue_url,
            url,
        })
    }
}

/// The open issue carrying `fingerprint`, as its number and URL, or `None` when there is none.
///
/// The issue this deployment last filed the fingerprint to is asked about first: it is found
/// even before GitHub's search has indexed it. Otherwise search finds it, and a result counts
/// only when its body holds the fingerprint's exact line, so a mention of the fingerprint
/// elsewhere never matches. Of several, the oldest is the problem's issue.
async fn open_issue(
    github: &Github,
    token: &SecretString,
    repository: &Repository,
    latest: Option<&IssueReport>,
    fingerprint: &str,
) -> Result<Option<(u64, String)>, GithubError> {
    if let Some(latest) = latest
        && let Ok(number) = u64::try_from(latest.issue_number)
    {
        let known = IssueRef {
            repository: repository.clone(),
            number,
        };
        if let Some(issue) = github.issue(token, &known).await?
            && issue.is_open
            && issue.pull_request.is_none()
        {
            return Ok(Some((issue.number, issue.url)));
        }
    }

    let line = fingerprint_line(fingerprint);
    let found = github
        .search_open_issues(token, repository, fingerprint)
        .await?;
    Ok(found
        .into_iter()
        .filter(|issue| issue.body.contains(&line))
        .min_by_key(|issue| issue.number)
        .map(|issue| (issue.number, issue.url)))
}

/// A GitHub failure as a report answers it: a token GitHub will not let write here says so
/// plainly, with what would work instead.
fn cannot_report(error: GithubError) -> ApiError {
    match error {
        GithubError::NoAccess(message) => ApiError::CodedBadRequest {
            code: "github_token_cannot_report",
            message: format!(
                "{message}. This token cannot open issues on {REPOSITORY_OWNER}/{REPOSITORY_NAME}: \
                 use a classic token with the public_repo scope"
            ),
        },
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_info::{GitCommit, GitInfo};

    fn credential(kind: GithubTokenKind, scopes: &str) -> GithubCredential {
        let now = Utc::now();
        GithubCredential {
            id: Uuid::now_v7(),
            name: "Work".to_owned(),
            kind,
            token_encrypted: Vec::new(),
            login: "ada".to_owned(),
            scopes: scopes.to_owned(),
            token_expires_at: None,
            checked_at: now,
            created_at: now,
            updated_at: now,
            is_default: false,
            created_by: Uuid::nil(),
        }
    }

    #[test]
    fn a_classic_token_needs_a_scope_that_reaches_public_repositories() {
        let now = Utc::now();
        assert_eq!(
            readiness(&credential(GithubTokenKind::Classic, "public_repo"), now),
            Readiness::Ready
        );
        assert_eq!(
            readiness(&credential(GithubTokenKind::Classic, "read:org,repo"), now),
            Readiness::Ready
        );
        assert_eq!(
            readiness(
                &credential(GithubTokenKind::Classic, "read:org,workflow"),
                now
            ),
            Readiness::MissingScope
        );
        assert_eq!(
            readiness(&credential(GithubTokenKind::Classic, ""), now),
            Readiness::MissingScope
        );
    }

    #[test]
    fn a_fine_grained_token_can_only_be_tried_and_an_expired_one_not_even_that() {
        let now = Utc::now();
        let fine_grained = credential(GithubTokenKind::FineGrained, "");
        assert_eq!(readiness(&fine_grained, now), Readiness::FineGrained);
        assert!(Readiness::FineGrained.may_try());

        let mut expired = credential(GithubTokenKind::Classic, "repo");
        expired.token_expires_at = Some(now - chrono::Duration::hours(1));
        assert_eq!(readiness(&expired, now), Readiness::Expired);
        assert!(!Readiness::Expired.may_try());
        assert!(!Readiness::MissingScope.may_try());
    }

    #[test]
    fn the_default_token_is_suggested_when_it_may_be_tried() {
        let now = Utc::now();
        let ready = credential(GithubTokenKind::Classic, "public_repo");
        let mut default_fine_grained = credential(GithubTokenKind::FineGrained, "");
        default_fine_grained.is_default = true;
        assert_eq!(
            suggested_credential(&[ready.clone(), default_fine_grained.clone()], now),
            Some(default_fine_grained.id)
        );

        let mut default_without_scope = credential(GithubTokenKind::Classic, "");
        default_without_scope.is_default = true;
        assert_eq!(
            suggested_credential(&[default_without_scope.clone(), ready.clone()], now),
            Some(ready.id),
            "a default that cannot report gives way to one that can"
        );
        assert_eq!(suggested_credential(&[default_without_scope], now), None);
    }

    #[test]
    fn the_environment_names_the_release_or_the_build() {
        let mut version = VersionInfo {
            name: "elysium-api",
            version: "0.1.0",
            release: None,
            profile: "release",
            rustc: "rustc 1.95.0",
            built_at: "2026-11-02T10:00:00Z",
            git: GitInfo {
                branch: "main".to_owned(),
                commit: Some(GitCommit {
                    hash: "abc1234def".to_owned(),
                    short_hash: "abc1234".to_owned(),
                    author: "Alex".to_owned(),
                    date: "2026-11-02T09:00:00Z".to_owned(),
                    subject: "A commit".to_owned(),
                }),
                history: Vec::new(),
            },
        };
        let built = environment(
            &version,
            Some("Mozilla/5.0 (X11; Linux x86_64) Firefox/145.0"),
        );
        assert_eq!(built.elysium, "0.1.0 (release build)");
        assert_eq!(built.build, "abc1234 on main, built 2026-11-02T10:00:00Z");
        assert_eq!(built.browser, "Firefox 145 on Linux");

        version.release = Some("1.4.2");
        assert_eq!(environment(&version, None).elysium, "1.4.2 (release)");
    }
}
