// Copyright © 2026 Jalapeno Labs

//! A local fake of GitHub's REST API, with state, for tests of the client and of links.
//!
//! It answers the way GitHub does for the calls Elysium makes: a bearer token or `401` with
//! `Bad credentials`, issues and pull requests through the issues API (a pull request is an
//! issue carrying `pull_request.merged_at`), `state_reason` on closed issues, filtering by
//! `state`, `since`, `milestone`, and `labels`, and `404` for anything it does not hold.
//! Every request is recorded, so a test asserts on what left the process, and a test can
//! change an issue the way someone on GitHub would, which moves its `updated_at`.
//!
//! No live GitHub account exists here, so these prove Elysium's half of the contract
//! against the shapes GitHub documents, not against GitHub.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};

use super::Github;

/// The token the fake accepts.
pub(crate) const TOKEN: &str = "ghp_FakeTokenForTests0123456789abcdefghij";

/// The account the token acts as.
pub(crate) const LOGIN: &str = "alex-navarro";

/// One issue or pull request the fake holds.
#[derive(Debug, Clone)]
pub(crate) struct FakeIssue {
    pub(crate) title: String,
    pub(crate) is_open: bool,
    pub(crate) state_reason: Option<String>,
    pub(crate) assignee: Option<String>,
    /// `Some(merged)` for a pull request.
    pub(crate) pull_request: Option<bool>,
    pub(crate) milestone: Option<u64>,
    pub(crate) labels: Vec<String>,
    pub(crate) updated_at: DateTime<Utc>,
}

impl FakeIssue {
    /// An open issue assigned to nobody, last updated an hour ago.
    pub(crate) fn open(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            is_open: true,
            state_reason: None,
            assignee: None,
            pull_request: None,
            milestone: None,
            labels: Vec::new(),
            updated_at: Utc::now() - TimeDelta::hours(1),
        }
    }

    /// An open pull request.
    pub(crate) fn pull_request(title: &str) -> Self {
        Self {
            pull_request: Some(false),
            ..Self::open(title)
        }
    }
}

/// One request the fake received.
#[derive(Debug, Clone)]
pub(crate) struct Received {
    pub(crate) method: Method,
    pub(crate) path: String,
    pub(crate) query: String,
    pub(crate) body: Value,
}

#[derive(Debug, Default)]
struct FakeState {
    /// By `owner/name` and number.
    issues: BTreeMap<(String, u64), FakeIssue>,
    /// Milestone titles by `owner/name` and number.
    milestones: BTreeMap<(String, u64), String>,
    /// Label names by `owner/name`.
    labels: BTreeMap<String, Vec<String>>,
    /// When set, every write is refused as a token without write access would be.
    refuse_writes: bool,
    received: Vec<Received>,
}

/// A running fake and a [`Github`] client pointed at it.
#[derive(Clone)]
pub(crate) struct FakeGithub {
    state: Arc<Mutex<FakeState>>,
    pub(crate) github: Github,
}

impl FakeGithub {
    /// Starts the fake on a free local port.
    pub(crate) async fn start() -> Self {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("address"));
        let router = Router::new()
            .fallback(answer)
            .with_state(Arc::clone(&state));
        tokio::spawn(async move { axum::serve(listener, router).await });

        Self {
            state,
            github: Github::with_test_origin(reqwest::Client::new(), origin),
        }
    }

    fn with_state<Output>(&self, act: impl FnOnce(&mut FakeState) -> Output) -> Output {
        act(&mut self.state.lock().expect("lock"))
    }

    /// Holds `issue` as `repository#number`.
    pub(crate) fn put_issue(&self, repository: &str, number: u64, issue: FakeIssue) {
        self.with_state(|state| {
            state.issues.insert((repository.to_owned(), number), issue);
        });
    }

    /// Changes an issue as someone on GitHub would, which moves its `updated_at` to now.
    pub(crate) fn change_issue(
        &self,
        repository: &str,
        number: u64,
        change: impl FnOnce(&mut FakeIssue),
    ) {
        self.with_state(|state| {
            let issue = state
                .issues
                .get_mut(&(repository.to_owned(), number))
                .expect("the fake holds that issue");
            change(issue);
            issue.updated_at = Utc::now();
        });
    }

    pub(crate) fn issue(&self, repository: &str, number: u64) -> FakeIssue {
        self.with_state(|state| {
            state
                .issues
                .get(&(repository.to_owned(), number))
                .cloned()
                .expect("the fake holds that issue")
        })
    }

    pub(crate) fn put_milestone(&self, repository: &str, number: u64, title: &str) {
        self.with_state(|state| {
            state
                .milestones
                .insert((repository.to_owned(), number), title.to_owned());
        });
    }

    pub(crate) fn put_label(&self, repository: &str, name: &str) {
        self.with_state(|state| {
            state
                .labels
                .entry(repository.to_owned())
                .or_default()
                .push(name.to_owned());
        });
    }

    /// Makes every write fail, as it does for a token without write access, or stop failing.
    pub(crate) fn refuse_writes(&self, refuse: bool) {
        self.with_state(|state| state.refuse_writes = refuse);
    }

    /// Everything the fake received, oldest first.
    pub(crate) fn received(&self) -> Vec<Received> {
        self.with_state(|state| state.received.clone())
    }

    /// The requests that changed something on GitHub.
    pub(crate) fn writes(&self) -> Vec<Received> {
        self.received()
            .into_iter()
            .filter(|request| request.method != Method::GET)
            .collect()
    }
}

fn github_error(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(json!({ "message": message }))).into_response()
}

fn issue_json(origin: &str, repository: &str, number: u64, issue: &FakeIssue) -> Value {
    let page = if issue.pull_request.is_some() {
        "pull"
    } else {
        "issues"
    };
    let mut body = json!({
        "number": number,
        "title": issue.title,
        "html_url": format!("https://github.com/{repository}/{page}/{number}"),
        "repository_url": format!("{origin}/repos/{repository}"),
        "state": if issue.is_open { "open" } else { "closed" },
        "state_reason": issue.state_reason,
        "assignee": issue.assignee.as_ref().map(|login| json!({ "login": login })),
        "updated_at": issue.updated_at,
    });
    if let Some(merged) = issue.pull_request {
        body["pull_request"] = json!({
            "url": format!("{origin}/repos/{repository}/pulls/{number}"),
            "merged_at": merged.then_some(issue.updated_at),
        });
    }
    body
}

/// A query string's value for `name`, decoded.
fn parameter(query: &str, name: &str) -> Option<String> {
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _value)| key == name)
        .map(|(_key, value)| value.into_owned())
}

#[expect(
    clippy::too_many_lines,
    reason = "one fake of every route the client calls, read as a table of answers"
)]
async fn answer(
    State(state): State<Arc<Mutex<FakeState>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: String,
) -> Response {
    let path = uri.path().to_owned();
    let query = uri.query().unwrap_or_default().to_owned();
    let origin = format!(
        "http://{}",
        headers
            .get("host")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
    );
    let mut state = state.lock().expect("lock");
    state.received.push(Received {
        method: method.clone(),
        path: path.clone(),
        query: query.clone(),
        body: serde_json::from_str(&body).unwrap_or(Value::Null),
    });

    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok());
    if authorization != Some(&format!("Bearer {TOKEN}")) {
        return github_error(StatusCode::UNAUTHORIZED, "Bad credentials");
    }
    if method != Method::GET && state.refuse_writes {
        return github_error(
            StatusCode::FORBIDDEN,
            "Resource not accessible by personal access token",
        );
    }

    let segments: Vec<String> = path
        .trim_start_matches('/')
        .split('/')
        .map(|segment| {
            percent_encoding::percent_decode_str(segment)
                .decode_utf8_lossy()
                .into_owned()
        })
        .collect();
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();

    match (method, segments.as_slice()) {
        (Method::GET, ["user"]) => axum::Json(json!({ "login": LOGIN })).into_response(),

        (Method::GET, ["repos", owner, name, "issues"]) => {
            let repository = format!("{owner}/{name}");
            let open_only = parameter(&query, "state").as_deref() == Some("open");
            let since: Option<DateTime<Utc>> = parameter(&query, "since")
                .and_then(|since| DateTime::parse_from_rfc3339(&since).ok())
                .map(|since| since.with_timezone(&Utc));
            let milestone: Option<u64> =
                parameter(&query, "milestone").and_then(|number| number.parse().ok());
            let label = parameter(&query, "labels");

            let mut listed: Vec<(&u64, &FakeIssue)> = state
                .issues
                .iter()
                .filter(|((held, _number), _issue)| *held == repository)
                .map(|((_repository, number), issue)| (number, issue))
                .filter(|(_number, issue)| !open_only || issue.is_open)
                .filter(|(_number, issue)| since.is_none_or(|since| issue.updated_at >= since))
                .filter(|(_number, issue)| milestone.is_none() || issue.milestone == milestone)
                .filter(|(_number, issue)| {
                    label
                        .as_ref()
                        .is_none_or(|label| issue.labels.contains(label))
                })
                .collect();
            listed.sort_by_key(|(_number, issue)| std::cmp::Reverse(issue.updated_at));
            let issues: Vec<Value> = listed
                .into_iter()
                .map(|(number, issue)| issue_json(&origin, &repository, *number, issue))
                .collect();
            axum::Json(issues).into_response()
        }

        (method @ (Method::GET | Method::PATCH), ["repos", owner, name, "issues", number]) => {
            let repository = format!("{owner}/{name}");
            let Ok(number) = number.parse::<u64>() else {
                return github_error(StatusCode::NOT_FOUND, "Not Found");
            };
            let Some(issue) = state.issues.get_mut(&(repository.clone(), number)) else {
                return github_error(StatusCode::NOT_FOUND, "Not Found");
            };
            if method == Method::PATCH {
                let change: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                if change["state"] == "closed" {
                    issue.is_open = false;
                    issue.state_reason = change["state_reason"].as_str().map(str::to_owned);
                    issue.updated_at = Utc::now();
                }
            }
            axum::Json(issue_json(&origin, &repository, number, issue)).into_response()
        }

        (Method::POST, ["repos", owner, name, "issues", number, "comments"]) => {
            let repository = format!("{owner}/{name}");
            let held = number
                .parse::<u64>()
                .is_ok_and(|number| state.issues.contains_key(&(repository, number)));
            if !held {
                return github_error(StatusCode::NOT_FOUND, "Not Found");
            }
            (StatusCode::CREATED, axum::Json(json!({ "id": 1 }))).into_response()
        }

        (Method::GET, ["repos", owner, name, "milestones", number]) => {
            let repository = format!("{owner}/{name}");
            let found = number.parse::<u64>().ok().and_then(|number| {
                state
                    .milestones
                    .get(&(repository.clone(), number))
                    .map(|title| (number, title))
            });
            let Some((number, title)) = found else {
                return github_error(StatusCode::NOT_FOUND, "Not Found");
            };
            axum::Json(json!({
                "number": number,
                "title": title,
                "html_url": format!("https://github.com/{repository}/milestone/{number}"),
                "state": "open",
            }))
            .into_response()
        }

        (Method::GET, ["repos", owner, name, "milestones"]) => {
            let repository = format!("{owner}/{name}");
            let milestones: Vec<Value> = state
                .milestones
                .iter()
                .filter(|((held, _number), _title)| *held == repository)
                .map(|((_repository, number), title)| {
                    json!({
                        "number": number,
                        "title": title,
                        "html_url": format!("https://github.com/{repository}/milestone/{number}"),
                        "state": "open",
                    })
                })
                .collect();
            axum::Json(milestones).into_response()
        }

        (Method::GET, ["repos", owner, name, "labels", label]) => {
            let repository = format!("{owner}/{name}");
            let held = state
                .labels
                .get(&repository)
                .is_some_and(|labels| labels.iter().any(|held| held == label));
            if !held {
                return github_error(StatusCode::NOT_FOUND, "Not Found");
            }
            axum::Json(json!({ "name": label })).into_response()
        }

        (Method::GET, ["repos", owner, name, "labels"]) => {
            let repository = format!("{owner}/{name}");
            let labels: Vec<Value> = state
                .labels
                .get(&repository)
                .map(|labels| labels.iter().map(|name| json!({ "name": name })).collect())
                .unwrap_or_default();
            axum::Json(labels).into_response()
        }

        _other => github_error(StatusCode::NOT_FOUND, "Not Found"),
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;
    use crate::github::issues::{IssueQuery, IssueRef};
    use crate::github::{GithubError, Repository};

    const REPOSITORY: &str = "JalapenoLabs/Elysium";

    fn token() -> SecretString {
        SecretString::from(TOKEN)
    }

    fn repository() -> Repository {
        Repository {
            owner: "JalapenoLabs".to_owned(),
            name: "Elysium".to_owned(),
        }
    }

    fn issue_ref(number: u64) -> IssueRef {
        IssueRef {
            repository: repository(),
            number,
        }
    }

    #[tokio::test]
    async fn an_issue_reads_its_state_its_reason_and_whether_it_is_a_pull_request() {
        let fake = FakeGithub::start().await;
        fake.put_issue(
            REPOSITORY,
            12,
            FakeIssue {
                assignee: Some(LOGIN.to_owned()),
                ..FakeIssue::open("Watch links")
            },
        );
        fake.put_issue(
            REPOSITORY,
            15,
            FakeIssue {
                is_open: false,
                pull_request: Some(true),
                ..FakeIssue::pull_request("Serve elysium_work")
            },
        );

        let issue = fake
            .github
            .issue(&token(), &issue_ref(12))
            .await
            .expect("read")
            .expect("held");
        assert_eq!(issue.reference(), "JalapenoLabs/Elysium#12");
        assert!(issue.is_open);
        assert_eq!(issue.assignee.as_deref(), Some(LOGIN));
        assert_eq!(issue.pull_request, None);
        assert_eq!(
            issue.url,
            "https://github.com/JalapenoLabs/Elysium/issues/12"
        );

        let merged = fake
            .github
            .issue(&token(), &issue_ref(15))
            .await
            .expect("read")
            .expect("held");
        assert_eq!(
            merged.pull_request.map(|pull_request| pull_request.merged),
            Some(true)
        );

        assert_eq!(
            fake.github
                .issue(&token(), &issue_ref(404))
                .await
                .expect("read"),
            None,
            "GitHub answers 404 for an issue the token cannot see"
        );
    }

    #[tokio::test]
    async fn a_listing_sends_since_and_state_and_reads_closed_issues_too() {
        let fake = FakeGithub::start().await;
        let mut old = FakeIssue::open("Old");
        old.updated_at = Utc::now() - TimeDelta::days(2);
        fake.put_issue(REPOSITORY, 1, old);
        fake.put_issue(
            REPOSITORY,
            2,
            FakeIssue {
                is_open: false,
                state_reason: Some("completed".to_owned()),
                ..FakeIssue::open("Recent")
            },
        );

        let since = Utc::now() - TimeDelta::days(1);
        let query = IssueQuery {
            since: Some(since),
            ..IssueQuery::default()
        };
        let listing = fake
            .github
            .list_issues(&token(), &repository(), &query, 1)
            .await
            .expect("listed");
        let numbers: Vec<u64> = listing.items.iter().map(|issue| issue.number).collect();
        assert_eq!(
            numbers,
            [2],
            "only what changed since, closed ones included"
        );

        let sent = &fake.received()[0];
        assert_eq!(parameter(&sent.query, "state").as_deref(), Some("all"));
        assert!(parameter(&sent.query, "since").is_some_and(|value| value.ends_with('Z')));
    }

    #[tokio::test]
    async fn closing_says_completed_and_a_comment_carries_its_body() {
        let fake = FakeGithub::start().await;
        fake.put_issue(REPOSITORY, 12, FakeIssue::open("Watch links"));

        fake.github
            .close_issue(&token(), &issue_ref(12))
            .await
            .expect("closed");
        fake.github
            .comment(&token(), &issue_ref(12), "Resolved in Elysium.")
            .await
            .expect("commented");

        let closed = fake.issue(REPOSITORY, 12);
        assert!(!closed.is_open);
        assert_eq!(closed.state_reason.as_deref(), Some("completed"));
        let writes = fake.writes();
        assert_eq!(writes[0].method, Method::PATCH);
        assert_eq!(
            writes[0].body,
            json!({ "state": "closed", "state_reason": "completed" })
        );
        assert_eq!(writes[1].body, json!({ "body": "Resolved in Elysium." }));
    }

    #[tokio::test]
    async fn a_refused_write_carries_githubs_message_and_a_wrong_token_says_what_to_check() {
        let fake = FakeGithub::start().await;
        fake.put_issue(REPOSITORY, 12, FakeIssue::open("Watch links"));
        fake.refuse_writes(true);

        let refused = fake
            .github
            .close_issue(&token(), &issue_ref(12))
            .await
            .unwrap_err();
        let GithubError::Refused(message) = refused else {
            panic!("a refused write is GitHub's answer, not the token's");
        };
        assert!(message.contains("not accessible"), "{message}");

        let wrong = fake
            .github
            .issue(&SecretString::from("ghp_wrong"), &issue_ref(12))
            .await
            .unwrap_err();
        assert!(matches!(wrong, GithubError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn milestones_and_labels_are_read_by_number_and_by_name() {
        let fake = FakeGithub::start().await;
        fake.put_milestone(REPOSITORY, 3, "Links");
        fake.put_label(REPOSITORY, "good first/issue");

        let milestone = fake
            .github
            .milestone(&token(), &repository(), 3)
            .await
            .expect("read")
            .expect("held");
        assert_eq!(milestone.title, "Links");

        let label = fake
            .github
            .label(&token(), &repository(), "good first/issue")
            .await
            .expect("read")
            .expect("held");
        assert_eq!(label.name, "good first/issue");
        assert!(
            fake.received()
                .last()
                .expect("a request")
                .path
                .ends_with("/labels/good%20first%2Fissue"),
            "a label with a slash stays one path segment"
        );
        assert_eq!(
            fake.github
                .label(&token(), &repository(), "missing")
                .await
                .expect("read"),
            None
        );
    }
}
