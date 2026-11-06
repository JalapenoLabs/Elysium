// Copyright © 2026 Jalapeno Labs

//! Filing against Postgres and a fake GitHub: one issue per problem, comments from everyone
//! after the first, the daily limit, and what each route answers.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::SecretString;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::{Uuid, uuid};

use super::draft::{self, Draft, Environment, Problem, fingerprint_line};
use super::redact::Redactor;
use super::subject::{self, Subject};
use super::{REPORT_LIMIT, REPOSITORY_NAME, REPOSITORY_OWNER};
use crate::errors::ApiError;
use crate::github::fake::{self, FakeGithub, FakeIssue};
use crate::models::connector;
use crate::models::connector_check::{self, CheckResult};
use crate::models::github_credential::{
    self, GithubCredential, GithubTokenKind, NewGithubCredential,
};
use crate::models::issue_report::{self, Filing, IssueReport};
use crate::routes;
use crate::state::AppState;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database, signed_in_cookie};

/// A second person, who hits the same problem as the test person.
const OTHER_PERSON_ID: Uuid = uuid!("0199a3c4-0000-7000-8000-00000000f00d");

const REPOSITORY: &str = "JalapenoLabs/Elysium";

struct Fixture {
    state: AppState,
    connection: AsyncPgConnection,
    github: FakeGithub,
    token: SecretString,
}

impl Fixture {
    async fn start() -> Self {
        let (url, mut connection) = migrated_database().await;
        diesel::sql_query(
            "INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) \
             VALUES ($1, $2, 'grace@navy.mil', 'Grace Hopper', 'member', 'active', now())",
        )
        .bind::<diesel::sql_types::Uuid, _>(OTHER_PERSON_ID)
        .bind::<diesel::sql_types::Uuid, _>(Uuid::now_v7())
        .execute(&mut connection)
        .await
        .expect("the other person is created");

        let github = FakeGithub::start().await;
        let mut state = app_state(&url).await;
        state.github = github.github.clone();
        Self {
            state,
            connection,
            github,
            token: SecretString::from(fake::TOKEN),
        }
    }

    async fn file(&self, person: Uuid, draft: &Draft) -> Result<super::FiledReport, ApiError> {
        self.state
            .issue_reports
            .file(
                &self.state.database,
                &self.state.github,
                person,
                &self.token,
                draft,
            )
            .await
    }

    async fn credential(&mut self, person: Uuid, name: &str, scopes: &[&str]) -> GithubCredential {
        github_credential::create(
            &mut self.connection,
            &self.state.cipher,
            &NewGithubCredential {
                created_by: person,
                name: name.to_owned(),
                connector_content: serde_json::json!({ "repositories": "*" }),
                verified: github_credential::VerifiedToken {
                    kind: GithubTokenKind::Classic,
                    token: SecretString::from(fake::TOKEN),
                    account: crate::github::Account {
                        login: fake::LOGIN.to_owned(),
                        scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
                        token_expires_at: None,
                    },
                },
            },
        )
        .await
        .expect("a GitHub credential")
    }

    async fn send(&self, path: &str, body: &Value) -> (StatusCode, Value) {
        let cookie = signed_in_cookie(&self.state).await;
        let request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, "http://localhost:4000")
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::USER_AGENT,
                "Mozilla/5.0 (X11; Linux x86_64; rv:145.0) Gecko/20100101 Firefox/145.0",
            )
            .body(Body::from(body.to_string()))
            .expect("a request builds");
        let response = routes::router(&self.state)
            .with_state(self.state.clone())
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body reads");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

fn environment() -> Environment {
    Environment {
        elysium: "1.4.2 (release)".to_owned(),
        build: "abc1234 on main, built 2026-11-02T10:00:00Z".to_owned(),
        browser: "Firefox 145 on Linux".to_owned(),
    }
}

/// The same failure as each person sees it, with their own address in it.
fn draft_for(address: &str) -> Draft {
    let problem = Problem {
        category: "connector:github:check:sign_in".to_owned(),
        summary: "The `github` connector's `sign_in` check failed.".to_owned(),
        error: format!("GitHub refused {address}: Bad credentials"),
        facts: Vec::new(),
        related: Vec::new(),
    };
    draft::render(&problem, &environment(), None, &Redactor::new([address]))
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_first_report_opens_an_issue_and_everyone_after_comments_on_it() {
    let fixture = Fixture::start().await;
    let first = draft_for("tester@example.com");
    let second = draft_for("grace@navy.mil");
    assert_eq!(first.fingerprint, second.fingerprint);

    let opened = fixture.file(TEST_PERSON_ID, &first).await.expect("filed");
    assert_eq!(opened.filed_as, Filing::Issue);
    let issue = fixture.github.issue(REPOSITORY, opened.issue_number);
    assert!(issue.body.contains(&fingerprint_line(&first.fingerprint)));
    assert!(!issue.body.contains("tester@example.com"));
    assert_eq!(issue.title, first.title);

    let joined = fixture.file(OTHER_PERSON_ID, &second).await.expect("filed");
    assert_eq!(joined.filed_as, Filing::Comment);
    assert_eq!(joined.issue_number, opened.issue_number);
    assert!(joined.url.contains("#issuecomment-"), "{}", joined.url);
    let comment = fixture
        .github
        .writes()
        .into_iter()
        .rfind(|request| request.path.ends_with("/comments"))
        .expect("a comment was posted");
    assert_eq!(comment.body["body"], second.body);

    let again = fixture.file(TEST_PERSON_ID, &first).await;
    assert!(
        matches!(
            again,
            Err(ApiError::CodedConflict {
                code: "already_reported",
                ..
            })
        ),
        "{again:?}"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_issue_search_has_not_indexed_yet_is_found_from_the_last_report() {
    let fixture = Fixture::start().await;
    fixture.github.lag_search(true);

    let opened = fixture
        .file(TEST_PERSON_ID, &draft_for("tester@example.com"))
        .await
        .expect("filed");
    let joined = fixture
        .file(OTHER_PERSON_ID, &draft_for("grace@navy.mil"))
        .await
        .expect("filed");
    assert_eq!(joined.filed_as, Filing::Comment);
    assert_eq!(joined.issue_number, opened.issue_number);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_closed_issue_means_the_problem_came_back_and_opens_another() {
    let fixture = Fixture::start().await;
    let opened = fixture
        .file(TEST_PERSON_ID, &draft_for("tester@example.com"))
        .await
        .expect("filed");
    fixture
        .github
        .change_issue(REPOSITORY, opened.issue_number, |issue| {
            issue.is_open = false;
        });

    let reopened = fixture
        .file(OTHER_PERSON_ID, &draft_for("grace@navy.mil"))
        .await
        .expect("filed");
    assert_eq!(reopened.filed_as, Filing::Issue);
    assert_ne!(reopened.issue_number, opened.issue_number);

    // The one who reported it before may report its return too.
    let returned = fixture
        .file(TEST_PERSON_ID, &draft_for("tester@example.com"))
        .await
        .expect("filed");
    assert_eq!(returned.filed_as, Filing::Comment);
    assert_eq!(returned.issue_number, reopened.issue_number);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn an_issue_another_deployment_opened_is_found_by_its_fingerprint_line_alone() {
    let fixture = Fixture::start().await;
    let draft = draft_for("tester@example.com");
    // Mentions the fingerprint, but is not a report of it.
    fixture.github.put_issue(
        REPOSITORY,
        3,
        FakeIssue {
            body: format!("Is {} the same as #4?", draft.fingerprint),
            ..FakeIssue::open("A question")
        },
    );
    fixture.github.put_issue(
        REPOSITORY,
        4,
        FakeIssue {
            body: format!("...\n{}\n", fingerprint_line(&draft.fingerprint)),
            ..FakeIssue::open("Reported elsewhere")
        },
    );

    let joined = fixture.file(TEST_PERSON_ID, &draft).await.expect("filed");
    assert_eq!(joined.filed_as, Filing::Comment);
    assert_eq!(joined.issue_number, 4);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_person_past_the_daily_limit_is_refused_before_github_is_asked() {
    let mut fixture = Fixture::start().await;
    for _report in 0..REPORT_LIMIT {
        issue_report::record(
            &mut fixture.connection,
            &IssueReport {
                id: Uuid::now_v7(),
                fingerprint: "0123456789abcdef".to_owned(),
                issue_number: 1,
                issue_url: "https://github.com/JalapenoLabs/Elysium/issues/1".to_owned(),
                filed_as: Filing::Comment,
                url: "https://github.com/JalapenoLabs/Elysium/issues/1#issuecomment-1".to_owned(),
                created_by: TEST_PERSON_ID,
                created_at: chrono::Utc::now(),
            },
        )
        .await
        .expect("a report row");
    }

    let refused = fixture
        .file(TEST_PERSON_ID, &draft_for("tester@example.com"))
        .await;
    assert!(
        matches!(
            refused,
            Err(ApiError::TooManyRequests {
                code: "report_limit",
                ..
            })
        ),
        "{refused:?}"
    );
    assert!(fixture.github.received().is_empty(), "GitHub was asked");

    // Someone else's reports are their own.
    fixture
        .file(OTHER_PERSON_ID, &draft_for("grace@navy.mil"))
        .await
        .expect("filed");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_token_github_will_not_let_write_says_what_would_work() {
    let fixture = Fixture::start().await;
    fixture.github.refuse_writes(true);
    let refused = fixture
        .file(TEST_PERSON_ID, &draft_for("tester@example.com"))
        .await;
    let Err(ApiError::CodedBadRequest { code, message }) = refused else {
        panic!("{refused:?}");
    };
    assert_eq!(code, "github_token_cannot_report");
    assert!(message.contains("public_repo"), "{message}");
    assert!(
        message.contains(&format!("{REPOSITORY_OWNER}/{REPOSITORY_NAME}")),
        "{message}"
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_stored_check_is_reported_from_what_elysium_recorded() {
    let mut fixture = Fixture::start().await;
    let credential = fixture
        .credential(TEST_PERSON_ID, "Work", &["public_repo"])
        .await;
    let stored = connector::find_by_account(
        &mut fixture.connection,
        connector::AccountRef::Github(credential.id),
    )
    .await
    .expect("the query runs")
    .expect("the account carries its connector");
    connector_check::record(
        &mut fixture.connection,
        &[
            CheckResult {
                connector_id: stored.id,
                check_id: "sign_in".to_owned(),
                state: "failed".to_owned(),
                count: None,
                reason: None,
                message: Some("GitHub: Bad credentials for tester@example.com".to_owned()),
                blocked_by: None,
                ran_at: chrono::Utc::now(),
            },
            CheckResult {
                connector_id: stored.id,
                check_id: "list_repositories".to_owned(),
                state: "skipped".to_owned(),
                count: None,
                reason: None,
                message: None,
                blocked_by: Some("sign_in".to_owned()),
                ran_at: chrono::Utc::now(),
            },
        ],
    )
    .await
    .expect("check results");

    let problem = subject::gather(
        &mut fixture.connection,
        &fixture.state.connectors.check_runs,
        TEST_PERSON_ID,
        &Subject::ConnectorCheck {
            connector_id: stored.id,
            check_id: "sign_in".to_owned(),
        },
    )
    .await
    .expect("a problem");
    assert_eq!(problem.category, "connector:github:check:sign_in");
    assert_eq!(
        problem.error,
        "GitHub: Bad credentials for tester@example.com"
    );
    assert_eq!(
        problem.related,
        vec!["list_repositories: skipped, blocked by sign_in"]
    );

    let passed = subject::gather(
        &mut fixture.connection,
        &fixture.state.connectors.check_runs,
        TEST_PERSON_ID,
        &Subject::ConnectorCheck {
            connector_id: stored.id,
            check_id: "list_repositories".to_owned(),
        },
    )
    .await;
    assert!(matches!(passed, Err(ApiError::BadRequest(_))), "{passed:?}");
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn the_routes_preview_exactly_what_they_then_post_with_the_persons_own_token() {
    let mut fixture = Fixture::start().await;
    let mine = fixture
        .credential(TEST_PERSON_ID, "Mine", &["public_repo"])
        .await;
    let scopeless = fixture.credential(TEST_PERSON_ID, "Scopeless", &[]).await;
    let theirs = fixture
        .credential(OTHER_PERSON_ID, "Theirs", &["repo"])
        .await;
    let report = json!({
        "subject": {
            "kind": "error",
            "area": "action_items.link_write",
            "message": "Jira refused tester@example.com on https://acme.atlassian.net/rest/api/3/issue",
        },
        "note": "Tester here, it broke after lunch",
    });

    let (status, preview) = fixture.send("/api/v1/issue-reports/preview", &report).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let body = preview["draft"]["body"].as_str().expect("a body");
    for hidden in ["tester@example.com", "acme", "Tester here"] {
        assert!(!body.contains(hidden), "{hidden} in {body}");
    }
    assert!(body.contains("Firefox 145 on Linux"), "{body}");
    assert_eq!(preview["repository"], "JalapenoLabs/Elysium");
    assert_eq!(preview["suggestedCredentialId"], mine.id.to_string());
    assert_eq!(preview["previousReport"], Value::Null);
    assert_eq!(preview["remainingToday"], REPORT_LIMIT);
    let listed: Vec<(&str, &str)> = preview["credentials"]
        .as_array()
        .expect("credentials")
        .iter()
        .map(|choice| {
            (
                choice["name"].as_str().unwrap_or_default(),
                choice["readiness"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        vec![("Mine", "ready"), ("Scopeless", "missing_scope")]
    );

    let with = |credential: Uuid| {
        let mut body = report.clone();
        body["credentialId"] = json!(credential);
        body
    };
    let (status, refused) = fixture
        .send("/api/v1/issue-reports", &with(theirs.id))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another person's token: {refused}"
    );
    let (status, refused) = fixture
        .send("/api/v1/issue-reports", &with(scopeless.id))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["code"], "github_token_cannot_report");
    assert!(fixture.github.writes().is_empty(), "nothing was posted");

    let (status, filed) = fixture.send("/api/v1/issue-reports", &with(mine.id)).await;
    assert_eq!(status, StatusCode::CREATED, "{filed}");
    assert_eq!(filed["report"]["filedAs"], "issue");
    let posted = fixture.github.writes().pop().expect("the issue was posted");
    assert_eq!(posted.body["body"], preview["draft"]["body"]);
    assert_eq!(posted.body["title"], preview["draft"]["title"]);

    let (_status, again) = fixture.send("/api/v1/issue-reports/preview", &report).await;
    assert_eq!(
        again["previousReport"]["issueNumber"],
        filed["report"]["issueNumber"]
    );
    assert_eq!(again["remainingToday"], REPORT_LIMIT - 1);
}
