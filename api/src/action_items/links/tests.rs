// Copyright © 2026 Jalapeno Labs

//! Each provider against a local fake of its API, with state, through credentials stored in
//! a real Postgres: what a link reads, what a write sends, and that every Jira call stays
//! inside the credential's allowlist.
//!
//! [`LinkFixture`] is shared with the watcher's tests: a migrated database, a Jira and a
//! GitHub credential, and a fake of each provider that a test changes the way someone
//! working in Jira or GitHub would.

use std::collections::BTreeMap;
use std::sync::Mutex;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::TimeDelta;
use diesel_async::AsyncPgConnection;
use secrecy::SecretString;
use serde_json::{Value, json};

use super::*;
use crate::action_items::watcher::WatchContext;
use crate::connections;
use crate::github::fake::{self, FakeGithub, FakeIssue};
use crate::jira::tests::{EMAIL, SITE_URL, TOKEN};
use crate::models::action_item_link::LinkCredential;
use crate::models::github_credential::{self, GithubTokenKind, NewGithubCredential};
use crate::models::jira_credential::{self, Allowed, AllowedProject, Allowlist, NewJiraCredential};
use crate::models::jira_done_transition;
use crate::realtime::EventBus;
use crate::test_support::{cipher, migrated_database};

/// The Jira account the credential's token belongs to. An issue assigned to it is the
/// user's.
pub(crate) const JIRA_ACCOUNT_ID: &str = "5b10a2844c20165700ede21g";

/// A repository the GitHub token reaches.
pub(crate) const REPOSITORY: &str = "JalapenoLabs/Elysium";

/// One issue the fake Jira holds.
#[derive(Debug, Clone)]
pub(crate) struct FakeJiraIssue {
    pub(crate) id: String,
    pub(crate) key: String,
    pub(crate) project: String,
    pub(crate) summary: String,
    pub(crate) status_id: String,
    /// The assignee's account id and name.
    pub(crate) assignee: Option<(String, String)>,
    pub(crate) priority: Option<String>,
    pub(crate) due_date: Option<String>,
    /// The id of the issue whose child this is.
    pub(crate) parent_id: Option<String>,
}

impl FakeJiraIssue {
    /// An issue in `ELY`, to do, assigned to nobody.
    pub(crate) fn new(id: &str, key: &str) -> Self {
        let project = key
            .rsplit_once('-')
            .map_or("ELY", |(project, _number)| project);
        Self {
            id: id.to_owned(),
            key: key.to_owned(),
            project: project.to_owned(),
            summary: format!("Work on {key}"),
            status_id: TO_DO.to_owned(),
            assignee: None,
            priority: None,
            due_date: None,
            parent_id: None,
        }
    }
}

/// The statuses every fake project has: to do, in progress, and done.
pub(crate) const TO_DO: &str = "1";
pub(crate) const DONE: &str = "10001";
/// A second `done` status, which only `OPS` has.
pub(crate) const WONT_DO: &str = "10002";

#[derive(Debug, Default)]
struct FakeJiraState {
    issues: Vec<FakeJiraIssue>,
    /// Saved filters: their names and the keys of the issues they find.
    filters: BTreeMap<String, (String, Vec<String>)>,
    received: Vec<(Method, String, Value)>,
}

/// A fake Jira Cloud site with state: issues that move between statuses, a transition into
/// every status, saved filters, and searches that honor `project IN (...)`, `id IN (...)`,
/// `parent = ...`, and `filter = ...`.
#[derive(Clone)]
pub(crate) struct FakeJira {
    state: Arc<Mutex<FakeJiraState>>,
    pub(crate) jira: Jira,
}

impl FakeJira {
    pub(crate) async fn start() -> Self {
        let state = Arc::new(Mutex::new(FakeJiraState::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("address"));
        let router = Router::new()
            .fallback(answer_jira)
            .with_state(Arc::clone(&state));
        tokio::spawn(async move { axum::serve(listener, router).await });
        Self {
            state,
            jira: Jira::with_test_origin(reqwest::Client::new(), origin),
        }
    }

    pub(crate) fn put_issue(&self, issue: FakeJiraIssue) {
        self.state.lock().expect("lock").issues.push(issue);
    }

    pub(crate) fn put_filter(&self, id: &str, name: &str, keys: &[&str]) {
        self.state.lock().expect("lock").filters.insert(
            id.to_owned(),
            (
                name.to_owned(),
                keys.iter().map(|key| (*key).to_owned()).collect(),
            ),
        );
    }

    /// Changes an issue as someone working in Jira would.
    pub(crate) fn change_issue(&self, key: &str, change: impl FnOnce(&mut FakeJiraIssue)) {
        let mut state = self.state.lock().expect("lock");
        let issue = state
            .issues
            .iter_mut()
            .find(|issue| issue.key == key)
            .expect("the fake holds that issue");
        change(issue);
    }

    pub(crate) fn issue(&self, key: &str) -> FakeJiraIssue {
        self.state
            .lock()
            .expect("lock")
            .issues
            .iter()
            .find(|issue| issue.key == key)
            .cloned()
            .expect("the fake holds that issue")
    }

    /// The requests that changed something in Jira.
    pub(crate) fn writes(&self) -> Vec<(Method, String, Value)> {
        self.state
            .lock()
            .expect("lock")
            .received
            .iter()
            .filter(|(method, _path, _body)| *method != Method::GET)
            .filter(|(_method, path, _body)| !path.ends_with("/search/jql"))
            .cloned()
            .collect()
    }

    /// Every JQL the fake was asked to run.
    pub(crate) fn searches(&self) -> Vec<String> {
        self.state
            .lock()
            .expect("lock")
            .received
            .iter()
            .filter(|(_method, path, _body)| path.ends_with("/search/jql"))
            .filter_map(|(_method, _path, body)| body["jql"].as_str().map(str::to_owned))
            .collect()
    }
}

/// A project's statuses: every project can be to do, in progress, or done, and `OPS` can
/// also be won't do, a second `done` status.
fn statuses_of(project: &str) -> Vec<(&'static str, &'static str, &'static str)> {
    let mut statuses = vec![
        (TO_DO, "To Do", "new"),
        ("3", "In Progress", "indeterminate"),
        (DONE, "Done", "done"),
    ];
    if project == "OPS" {
        statuses.push((WONT_DO, "Won't Do", "done"));
    }
    statuses
}

fn status_json(project: &str, status_id: &str) -> Value {
    let (id, name, category) = statuses_of(project)
        .into_iter()
        .find(|(id, _name, _category)| *id == status_id)
        .expect("a status the project has");
    json!({ "id": id, "name": name, "statusCategory": { "key": category } })
}

fn jira_issue_json(issue: &FakeJiraIssue) -> Value {
    json!({
        "id": issue.id,
        "key": issue.key,
        "fields": {
            "summary": issue.summary,
            "status": status_json(&issue.project, &issue.status_id),
            "assignee": issue.assignee.as_ref().map(|(account_id, name)| {
                json!({ "accountId": account_id, "displayName": name })
            }),
            "priority": issue.priority.as_ref().map(|name| json!({ "name": name })),
            "duedate": issue.due_date,
            "labels": [],
            "project": { "key": issue.project },
        },
    })
}

/// The comma-separated values inside the parentheses after `marker`, unquoted.
fn listed_after(jql: &str, marker: &str) -> Option<Vec<String>> {
    let (_before, after) = jql.split_once(marker)?;
    let (inside, _rest) = after.split_once(')')?;
    Some(
        inside
            .split(',')
            .map(|value| value.trim().trim_matches('"').to_owned())
            .collect(),
    )
}

/// The value after `marker`, up to the next space or parenthesis.
fn value_after(jql: &str, marker: &str) -> Option<String> {
    let (_before, after) = jql.split_once(marker)?;
    Some(
        after
            .split([' ', ')'])
            .next()
            .unwrap_or_default()
            .to_owned(),
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "one fake of every route the provider calls, read as a table of answers"
)]
async fn answer_jira(
    State(state): State<Arc<Mutex<FakeJiraState>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: String,
) -> Response {
    let path = uri.path().to_owned();
    let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let mut state = state.lock().expect("lock");
    state
        .received
        .push((method.clone(), path.clone(), request.clone()));

    let expected = format!("Basic {}", BASE64.encode(format!("{EMAIL}:{TOKEN}")));
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok());
    if authorization != Some(expected.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let not_found = || {
        (
            StatusCode::NOT_FOUND,
            axum::Json(json!({ "errorMessages": ["Issue does not exist"], "errors": {} })),
        )
            .into_response()
    };

    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method, segments.as_slice()) {
        (Method::POST, ["rest", "api", "3", "search", "jql"]) => {
            let jql = request["jql"].as_str().unwrap_or_default().to_owned();
            let projects = listed_after(&jql, "project IN (");
            let ids = listed_after(&jql, "id IN (");
            let parent = value_after(&jql, "parent = ");
            let filtered_keys = value_after(&jql, "filter = ")
                .and_then(|id| state.filters.get(&id).map(|(_name, keys)| keys.clone()));
            let issues: Vec<Value> = state
                .issues
                .iter()
                .filter(|issue| {
                    projects
                        .as_ref()
                        .is_none_or(|projects| projects.contains(&issue.project))
                })
                .filter(|issue| ids.as_ref().is_none_or(|ids| ids.contains(&issue.id)))
                .filter(|issue| {
                    parent
                        .as_ref()
                        .is_none_or(|parent| issue.parent_id.as_ref() == Some(parent))
                })
                .filter(|issue| {
                    filtered_keys
                        .as_ref()
                        .is_none_or(|keys| keys.contains(&issue.key))
                })
                .map(jira_issue_json)
                .collect();
            axum::Json(json!({ "issues": issues, "isLast": true })).into_response()
        }

        (Method::GET, ["rest", "api", "3", "issue", key]) => {
            let found = state.issues.iter().find(|issue| issue.key == *key);
            found.map_or_else(not_found, |issue| {
                axum::Json(jira_issue_json(issue)).into_response()
            })
        }

        (Method::GET, ["rest", "api", "3", "issue", key, "transitions"]) => {
            let Some(issue) = state.issues.iter().find(|issue| issue.key == *key) else {
                return not_found();
            };
            let transitions: Vec<Value> = statuses_of(&issue.project)
                .into_iter()
                .filter(|(id, _name, _category)| *id != issue.status_id)
                .map(|(id, name, _category)| {
                    json!({
                        "id": format!("t{id}"),
                        "name": format!("To {name}"),
                        "to": status_json(&issue.project, id),
                    })
                })
                .collect();
            axum::Json(json!({ "transitions": transitions })).into_response()
        }

        (Method::POST, ["rest", "api", "3", "issue", key, "transitions"]) => {
            let Some(issue) = state.issues.iter_mut().find(|issue| issue.key == *key) else {
                return not_found();
            };
            let transition = request["transition"]["id"].as_str().unwrap_or_default();
            let Some(status_id) = transition.strip_prefix('t') else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            issue.status_id = status_id.to_owned();
            StatusCode::NO_CONTENT.into_response()
        }

        (Method::POST, ["rest", "api", "3", "issue", key, "comment"]) => {
            if !state.issues.iter().any(|issue| issue.key == *key) {
                return not_found();
            }
            (
                StatusCode::CREATED,
                axum::Json(json!({ "id": "10100", "body": request["body"] })),
            )
                .into_response()
        }

        (Method::GET, ["rest", "api", "3", "project", project, "statuses"]) => {
            let statuses: Vec<Value> = statuses_of(project)
                .into_iter()
                .map(|(id, _name, _category)| status_json(project, id))
                .collect();
            axum::Json(json!([{ "name": "Task", "statuses": statuses }])).into_response()
        }

        (Method::GET, ["rest", "api", "3", "filter", id]) => {
            let found = state.filters.get(*id).map(|(name, _keys)| name.clone());
            found.map_or_else(not_found, |name| {
                axum::Json(json!({ "id": id, "name": name })).into_response()
            })
        }

        _other => not_found(),
    }
}

/// A migrated database with a Jira credential allowed `ELY` and `OPS`, a GitHub token, and a
/// fake of each provider behind them.
pub(crate) struct LinkFixture {
    pub(crate) connection: AsyncPgConnection,
    pub(crate) jira: FakeJira,
    pub(crate) github: FakeGithub,
    pub(crate) links: Links,
    pub(crate) events: EventBus,
    pub(crate) database: Pool,
    pub(crate) jira_credential: LinkCredential,
    pub(crate) github_credential: LinkCredential,
}

impl LinkFixture {
    pub(crate) async fn start() -> Self {
        let (url, mut connection) = migrated_database().await;
        let database = connections::connect_postgres(&url, 4)
            .await
            .expect("a pool on the test database");
        let cipher = Arc::new(cipher());

        let jira_credential = jira_credential::create(
            &mut connection,
            &cipher,
            &NewJiraCredential {
                name: "Work".to_owned(),
                verified: jira_credential::VerifiedToken {
                    site_url: SITE_URL.to_owned(),
                    account_email: EMAIL.to_owned(),
                    token: SecretString::from(TOKEN),
                    account: crate::jira::Account {
                        account_id: JIRA_ACCOUNT_ID.to_owned(),
                        display_name: "Alex Navarro".to_owned(),
                        email: None,
                    },
                },
                allowed: Allowed {
                    projects: Allowlist::Only(vec![
                        AllowedProject {
                            id: "10002".to_owned(),
                            key: "ELY".to_owned(),
                            name: "Elysium".to_owned(),
                        },
                        AllowedProject {
                            id: "10003".to_owned(),
                            key: "OPS".to_owned(),
                            name: "Operations".to_owned(),
                        },
                    ]),
                    boards: Allowlist::Only(Vec::new()),
                },
            },
        )
        .await
        .expect("a Jira credential");
        let github_credential = github_credential::create(
            &mut connection,
            &cipher,
            &NewGithubCredential {
                name: "Personal".to_owned(),
                verified: github_credential::VerifiedToken {
                    kind: GithubTokenKind::Classic,
                    token: SecretString::from(fake::TOKEN),
                    account: crate::github::Account {
                        login: fake::LOGIN.to_owned(),
                        scopes: vec!["repo".to_owned()],
                        token_expires_at: None,
                    },
                },
            },
        )
        .await
        .expect("a GitHub credential");

        let jira = FakeJira::start().await;
        let github = FakeGithub::start().await;
        let links = Links::new(database.clone(), cipher, &jira.jira, &github.github);
        Self {
            connection,
            jira,
            github,
            links,
            events: EventBus::new(),
            database,
            jira_credential: LinkCredential {
                provider: LinkProvider::Jira,
                id: jira_credential.id,
            },
            github_credential: LinkCredential {
                provider: LinkProvider::Github,
                id: github_credential.id,
            },
        }
    }

    pub(crate) fn context(&self) -> WatchContext {
        WatchContext {
            database: self.database.clone(),
            events: self.events.clone(),
            links: self.links.clone(),
        }
    }

    pub(crate) fn jira_provider(&self) -> &dyn Provider {
        self.links.provider(LinkProvider::Jira)
    }

    pub(crate) fn github_provider(&self) -> &dyn Provider {
        self.links.provider(LinkProvider::Github)
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_jira_issue_reads_in_the_items_terms_and_only_inside_the_allowlist() {
    let fixture = LinkFixture::start().await;
    fixture.jira.put_issue(FakeJiraIssue {
        assignee: Some((JIRA_ACCOUNT_ID.to_owned(), "Alex Navarro".to_owned())),
        priority: Some("Highest".to_owned()),
        due_date: Some("2026-09-30".to_owned()),
        ..FakeJiraIssue::new("10042", "ELY-12")
    });
    fixture.jira.put_issue(FakeJiraIssue {
        assignee: Some(("62a1".to_owned(), "Sam".to_owned())),
        ..FakeJiraIssue::new("10043", "ELY-13")
    });
    fixture
        .jira
        .put_issue(FakeJiraIssue::new("10099", "SECRET-1"));

    let remote = fixture
        .jira_provider()
        .find(fixture.jira_credential.id, LinkKind::Issue, "ELY-12")
        .await
        .expect("found");
    assert_eq!(
        remote.external_id, "10042",
        "an issue is its id, which survives a move"
    );
    assert_eq!(remote.key, "ELY-12");
    assert_eq!(remote.url, "https://acme.atlassian.net/browse/ELY-12");
    assert_eq!(remote.state, LinkState::Open);
    assert_eq!(
        remote.owner,
        Owner::User,
        "assigned to the credential's own account: the user's"
    );
    assert_eq!(remote.starting_priority(), ActionItemPriority::Urgent);
    assert_eq!(
        remote.starting_due_at().expect("a due date").to_rfc3339(),
        "2026-09-30T23:59:59.999+00:00"
    );

    let theirs = fixture
        .jira_provider()
        .find(fixture.jira_credential.id, LinkKind::Issue, "ELY-13")
        .await
        .expect("found");
    assert_eq!(
        theirs.owner,
        Owner::Other {
            name: "Sam".to_owned()
        }
    );
    assert_eq!(theirs.starting_priority(), ActionItemPriority::Normal);

    let refused = fixture
        .jira_provider()
        .find(fixture.jira_credential.id, LinkKind::Issue, "SECRET-1")
        .await
        .unwrap_err();
    assert!(matches!(refused, LinkError::Forbidden(_)), "{refused}");
    let refused = fixture
        .jira_provider()
        .find(fixture.jira_credential.id, LinkKind::PullRequest, "ELY-12")
        .await
        .unwrap_err();
    assert!(matches!(refused, LinkError::Invalid(_)), "{refused}");
}

/// The link row a test hands a provider, as if an item had linked `remote`.
async fn linked(
    fixture: &mut LinkFixture,
    credential: LinkCredential,
    remote: &Remote,
    kind: LinkKind,
) -> ActionItemLink {
    use crate::action_items::Actor;
    use crate::models::action_item::{ActionItemState, NewActionItem};
    use crate::models::action_item_link::{self, NewLink};

    let new_item = NewActionItem {
        title: remote.title.clone(),
        notes: String::new(),
        state: ActionItemState::Open,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    };
    let new_link = NewLink {
        credential,
        kind,
        external_id: remote.external_id.clone(),
        observation: remote.observation(),
    };
    action_item_link::create_linked_item(
        &mut fixture.connection,
        new_item,
        new_link,
        Actor::User,
        Utc::now(),
    )
    .await
    .expect("linked")
    .links
    .remove(0)
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_jira_close_takes_the_only_done_transition_once() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .jira
        .put_issue(FakeJiraIssue::new("10042", "ELY-12"));
    let remote = fixture
        .jira_provider()
        .find(fixture.jira_credential.id, LinkKind::Issue, "ELY-12")
        .await
        .expect("found");
    let credential = fixture.jira_credential;
    let link = linked(&mut fixture, credential, &remote, LinkKind::Issue).await;

    let closed = fixture.jira_provider().close(&link).await.expect("closed");
    assert_eq!(closed, WriteOutcome::Landed);
    assert_eq!(fixture.jira.issue("ELY-12").status_id, DONE);

    let again = fixture.jira_provider().close(&link).await.expect("read");
    assert_eq!(
        again,
        WriteOutcome::AlreadyDone,
        "the issue is read first, so a retried close never moves it twice"
    );
    let transitions = fixture
        .jira
        .writes()
        .into_iter()
        .filter(|(_method, path, _body)| path.ends_with("/transitions"))
        .count();
    assert_eq!(transitions, 1);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_jira_close_waits_for_a_choice_among_several_done_statuses() {
    let mut fixture = LinkFixture::start().await;
    fixture.jira.put_issue(FakeJiraIssue::new("10050", "OPS-7"));
    let remote = fixture
        .jira_provider()
        .find(fixture.jira_credential.id, LinkKind::Issue, "OPS-7")
        .await
        .expect("found");
    let credential = fixture.jira_credential;
    let link = linked(&mut fixture, credential, &remote, LinkKind::Issue).await;

    let waiting = fixture.jira_provider().close(&link).await.expect("read");
    let WriteOutcome::Waiting(reason) = waiting else {
        panic!("a project with two done statuses waits for the user, got {waiting:?}");
    };
    assert!(
        reason.contains("Done") && reason.contains("Won't Do"),
        "{reason}"
    );
    assert!(reason.contains("Settings, Jira"), "{reason}");
    assert!(
        fixture.jira.writes().is_empty(),
        "nothing moved while it waits"
    );

    jira_done_transition::choose(
        &mut fixture.connection,
        credential.id,
        "OPS",
        WONT_DO,
        "Won't Do",
    )
    .await
    .expect("chosen");
    let closed = fixture.jira_provider().close(&link).await.expect("closed");
    assert_eq!(closed, WriteOutcome::Landed);
    assert_eq!(fixture.jira.issue("OPS-7").status_id, WONT_DO);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_jira_containers_children_are_searched_inside_the_allowlist() {
    let fixture = LinkFixture::start().await;
    fixture.jira.put_issue(FakeJiraIssue::new("10001", "ELY-1"));
    for (id, key) in [
        ("10002", "ELY-2"),
        ("10003", "ELY-3"),
        ("10009", "SECRET-9"),
    ] {
        fixture.jira.put_issue(FakeJiraIssue {
            parent_id: Some("10001".to_owned()),
            ..FakeJiraIssue::new(id, key)
        });
    }
    fixture
        .jira
        .put_filter("20001", "My open work", &["ELY-2", "SECRET-9"]);

    let epic = fixture
        .jira_provider()
        .find_container(fixture.jira_credential.id, ContainerKind::Epic, "ELY-1")
        .await
        .expect("an epic");
    assert_eq!(
        (epic.external_id.as_str(), epic.key.as_str()),
        ("10001", "ELY-1")
    );
    let filter = fixture
        .jira_provider()
        .find_container(fixture.jira_credential.id, ContainerKind::Filter, "20001")
        .await
        .expect("a filter");
    assert_eq!(filter.title, "My open work");
    fixture
        .jira_provider()
        .find_container(
            fixture.jira_credential.id,
            ContainerKind::Filter,
            "not-a-number",
        )
        .await
        .expect_err("a filter is named by its id");

    let container = |kind, external_id: &str, external_key: &str| InitiativeLink {
        id: Uuid::now_v7(),
        initiative_id: Uuid::now_v7(),
        provider: LinkProvider::Jira,
        kind,
        jira_credential_id: Some(fixture.jira_credential.id),
        github_credential_id: None,
        external_id: external_id.to_owned(),
        external_key: external_key.to_owned(),
        url: String::new(),
        title: String::new(),
        synced_at: None,
        sync_error: None,
        truncated: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let children = fixture
        .jira_provider()
        .children(&container(ContainerKind::Epic, "10001", "ELY-1"))
        .await
        .expect("children");
    let keys: Vec<&str> = children
        .items
        .iter()
        .map(|child| child.key.as_str())
        .collect();
    assert_eq!(
        keys,
        ["ELY-2", "ELY-3"],
        "a child outside the allowlist is never read"
    );
    assert_eq!(children.title, "Work on ELY-1");
    let filtered = fixture
        .jira_provider()
        .children(&container(ContainerKind::Filter, "20001", "20001"))
        .await
        .expect("children");
    assert_eq!(filtered.items.len(), 1);

    for jql in fixture.jira.searches() {
        assert!(
            jql.contains(r#"project IN ("ELY", "OPS")"#),
            "every search is bounded: {jql}"
        );
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_github_link_is_the_kind_it_says_and_a_pull_request_is_never_closed() {
    let mut fixture = LinkFixture::start().await;
    fixture.github.put_issue(
        REPOSITORY,
        12,
        FakeIssue {
            assignee: Some(fake::LOGIN.to_owned()),
            ..FakeIssue::open("Watch links")
        },
    );
    fixture.github.put_issue(
        REPOSITORY,
        15,
        FakeIssue::pull_request("Serve elysium_work"),
    );
    let credential = fixture.github_credential;

    let issue = fixture
        .github_provider()
        .find(credential.id, LinkKind::Issue, "JalapenoLabs/Elysium#12")
        .await
        .expect("an issue");
    assert_eq!(issue.external_id, "JalapenoLabs/Elysium#12");
    assert_eq!(issue.owner, Owner::User);
    let refused = fixture
        .github_provider()
        .find(
            credential.id,
            LinkKind::PullRequest,
            "JalapenoLabs/Elysium#12",
        )
        .await
        .unwrap_err();
    assert!(matches!(refused, LinkError::Invalid(_)), "{refused}");
    let pull_request = fixture
        .github_provider()
        .find(
            credential.id,
            LinkKind::PullRequest,
            "JalapenoLabs/Elysium#15",
        )
        .await
        .expect("a pull request");
    assert_eq!(pull_request.owner, Owner::Nobody);

    let issue_link = linked(&mut fixture, credential, &issue, LinkKind::Issue).await;
    let pull_request_link = linked(
        &mut fixture,
        credential,
        &pull_request,
        LinkKind::PullRequest,
    )
    .await;

    assert_eq!(
        fixture
            .github_provider()
            .close(&issue_link)
            .await
            .expect("closed"),
        WriteOutcome::Landed
    );
    assert_eq!(
        fixture.github.issue(REPOSITORY, 12).state_reason.as_deref(),
        Some("completed")
    );
    assert_eq!(
        fixture
            .github_provider()
            .close(&issue_link)
            .await
            .expect("read"),
        WriteOutcome::AlreadyDone
    );
    fixture
        .github_provider()
        .close(&pull_request_link)
        .await
        .expect_err("Elysium never closes a pull request");
    assert!(fixture.github.issue(REPOSITORY, 15).is_open);
    assert_eq!(fixture.github.writes().len(), 1, "one close, sent once");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn github_changes_since_a_cursor_are_only_what_changed() {
    let mut fixture = LinkFixture::start().await;
    fixture
        .github
        .put_issue(REPOSITORY, 1, FakeIssue::open("Quiet"));
    fixture
        .github
        .put_issue(REPOSITORY, 2, FakeIssue::open("Busy"));
    let credential = fixture.github_credential;
    let mut links = Vec::new();
    for reference in ["JalapenoLabs/Elysium#1", "JalapenoLabs/Elysium#2"] {
        let remote = fixture
            .github_provider()
            .find(credential.id, LinkKind::Issue, reference)
            .await
            .expect("found");
        links.push(linked(&mut fixture, credential, &remote, LinkKind::Issue).await);
    }

    let since = Utc::now() - TimeDelta::minutes(30);
    fixture
        .github
        .change_issue(REPOSITORY, 2, |issue| issue.is_open = false);
    let changed = fixture
        .github_provider()
        .changes(credential.id, &links, Some(since))
        .await
        .expect("changes");
    let keys: Vec<&str> = changed.iter().map(|remote| remote.key.as_str()).collect();
    assert_eq!(keys, ["JalapenoLabs/Elysium#2"]);
    assert_eq!(changed[0].state, LinkState::Done);

    let everything = fixture
        .github_provider()
        .changes(credential.id, &links, None)
        .await
        .expect("changes");
    assert_eq!(everything.len(), 2, "with no cursor, every link is read");
}
