// Copyright © 2026 Jalapeno Labs

//! Slack: a person's workspaces, read through Slack's web client in a browser they signed in
//! to. See `docs/connectors.md`, Slack, and `crate::connectors::slack` for the reading.
//!
//! The account is the browser, as for Outlook and Gmail: nothing is installed into a Slack
//! workspace. A new one is made and signed in to on its own page under Settings, Browser
//! sign-ins; the hub then connects Slack through it. Its content picks the workspaces,
//! conversations, and direct messages it reads.

use serde_json::{Value, json};
use uuid::Uuid;

use super::{AccountChoice, ContentField, ContentFieldType, Definition, Source};
use crate::auth::CurrentUser;
use crate::browser::{BrowserState, Browsers};
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::slack::{self, Content, Counts, START_URL, Slack, Tab, Team, count_teams};
use crate::connectors::{AccountKind, ConnectorKind, Scope, SignInState};
use crate::errors::ApiError;
use crate::models::browser_session::{self, BrowserSession};
use crate::models::connector::{self, AccountRef, Connector, NewConnector};
use crate::routes::v1::jira_credentials::Selection;
use crate::state::AppState;

const CHECKS: &[CheckSpec] = &[
    CheckSpec {
        id: "browser_running",
        required: true,
        depends_on: &[],
    },
    CheckSpec {
        id: "signed_in",
        required: true,
        depends_on: &["browser_running"],
    },
    CheckSpec {
        id: "workspace_reachable",
        required: true,
        depends_on: &["signed_in"],
    },
    CheckSpec {
        id: "sample_conversation",
        required: false,
        depends_on: &["workspace_reachable"],
    },
];

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Slack,
    scope: Scope::Personal,
    account: AccountKind::Browser,
    start_url: Some(START_URL),
    content: &[ContentField {
        id: "conversations",
        field: ContentFieldType::Selection,
        required: true,
    }],
    checks: CHECKS,
};

/// The browser `id`, when `owner` created it.
async fn owned_browser(
    state: &AppState,
    id: Uuid,
    owner: Uuid,
) -> Result<BrowserSession, ApiError> {
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let browser = browser_session::find(&mut connection, id)
        .await?
        .filter(|browser| browser.created_by == owner)
        .ok_or(ApiError::NotFound)?;
    Ok(browser)
}

fn sign_in_first() -> ApiError {
    ApiError::BadRequest(
        "Slack is read through a browser: sign in to one under Settings, Browser sign-ins first"
            .to_owned(),
    )
}

/// What a client sent as Slack's content, checked for shape: `"*"`, or ids that look like a
/// workspace, a workspace's conversation, or its direct messages. Whether each is still there
/// is for the checks to say; one that is gone reads nothing.
fn content_of(content: Value) -> Result<Content, ApiError> {
    let content: Content = serde_json::from_value(content)
        .map_err(|error| ApiError::BadRequest(format!("slack content is not valid: {error}")))?;
    if let Selection::Only(picked) = &content.conversations {
        if picked.is_empty() {
            return Err(ApiError::BadRequest(
                "pick at least one workspace or conversation, or \"*\"".to_owned(),
            ));
        }
        if let Some(bad) = picked.iter().find(|entry| !is_pick(entry)) {
            return Err(ApiError::BadRequest(format!(
                "{bad:?} is not a Slack workspace or conversation"
            )));
        }
    }
    Ok(content)
}

/// `T1`, `T1/C1`, or `T1/dms`, in Slack's id alphabet.
fn is_pick(entry: &str) -> bool {
    let is_id = |id: &str| {
        !id.is_empty()
            && id.len() <= 32
            && id
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
    };
    match entry.split_once('/') {
        None => is_id(entry),
        Some((team, rest)) => is_id(team) && (rest == "dms" || is_id(rest)),
    }
}

pub async fn prepare_checks(
    state: &AppState,
    current: &CurrentUser,
    source: Source<'_>,
) -> Result<SlackChecks, ApiError> {
    let (browser_id, content) = match source {
        Source::Stored(stored) => {
            let id = stored.browser_session_id.ok_or_else(|| {
                ApiError::BadRequest("a Slack connector reads through a browser".to_owned())
            })?;
            let content = slack::parse_content(&stored.content)
                .map_err(|error| ApiError::BadRequest(error.to_string()))?;
            (id, content)
        }
        Source::Draft { account, content } => match account {
            AccountChoice::Existing { id } => (id, content_of(content)?),
            AccountChoice::New { .. } => return Err(sign_in_first()),
        },
    };
    let browser = owned_browser(state, browser_id, current.id()).await?;
    Ok(SlackChecks {
        reader: Slack::new(state.connectors.ingest.devtools().clone()),
        browsers: state.browsers.clone(),
        browser,
        content,
        counted: None,
    })
}

/// Slack's checks against one browser.
pub struct SlackChecks {
    reader: Slack,
    browsers: Browsers,
    browser: BrowserSession,
    content: Content,
    /// The workspaces Slack answered, read once and shared by every check that needs them.
    counted: Option<Result<Vec<(Team, Counts)>, Outcome>>,
}

impl std::fmt::Debug for SlackChecks {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SlackChecks")
            .field("browser", &self.browser.id)
            .finish_non_exhaustive()
    }
}

impl Checks for SlackChecks {
    async fn check(&mut self, id: &'static str) -> Outcome {
        match id {
            "browser_running" => {
                let status = self.browsers.status(self.browser.id);
                if status.state == BrowserState::Running {
                    return Outcome::passed();
                }
                Outcome {
                    message: status.error,
                    ..Outcome::failed_because("browser_not_running")
                }
            }
            "signed_in" => match self.counted().await {
                Ok(_teams) => Outcome::passed(),
                Err(outcome) => outcome,
            },
            "workspace_reachable" => match self.counted().await {
                Ok(teams) => {
                    let reached = teams
                        .iter()
                        .filter(|(team, _counts)| self.content.reads_team(&team.id))
                        .count();
                    if reached == 0 {
                        return Outcome::failed_because("no_workspace_reached");
                    }
                    Outcome::counted(reached)
                }
                Err(outcome) => outcome,
            },
            "sample_conversation" => self.sample().await,
            unknown => Outcome::failed(format!("Slack has no check {unknown}")),
        }
    }
}

impl SlackChecks {
    /// The workspaces the browser is signed in to, with their counts, read once.
    async fn counted(&mut self) -> Result<Vec<(Team, Counts)>, Outcome> {
        if self.counted.is_none() {
            let counted = match self.reader.ask(&self.browser, true, &[]).await {
                Ok(Tab::Open { teams, .. }) => match count_teams(&teams) {
                    Ok(counted) if counted.is_empty() => Err(Outcome::failed_because("signed_out")),
                    Ok(counted) => Ok(counted),
                    Err(error) => Err(Outcome::failed(error.to_string())),
                },
                Ok(Tab::Missing(SignInState::SignedOut)) => {
                    Err(Outcome::failed_because("signed_out"))
                }
                Ok(Tab::Missing(_unknown)) => Err(Outcome::failed_because("slack_not_open")),
                Err(error) => Err(Outcome::failed(error.to_string())),
            };
            self.counted = Some(counted);
        }
        self.counted
            .clone()
            .unwrap_or_else(|| Err(Outcome::failed("Slack was not read")))
    }

    /// Reads the newest few messages of the most recently active conversation it reads.
    async fn sample(&mut self) -> Outcome {
        let teams = match self.counted().await {
            Ok(teams) => teams,
            Err(outcome) => return outcome,
        };
        let newest = teams
            .iter()
            .flat_map(|(team, counts)| {
                counts
                    .conversations
                    .iter()
                    .filter(|conversation| {
                        self.content
                            .reads(&team.id, &conversation.id, conversation.kind)
                    })
                    .filter_map(move |conversation| {
                        conversation
                            .latest
                            .as_deref()
                            .map(|latest| (team, conversation, latest))
                    })
            })
            .max_by(|left, right| slack::ts_order(left.2, right.2));
        let Some((team, conversation, _latest)) = newest else {
            return Outcome::skipped("no_conversations");
        };
        let call = slack::sample_call(&team.id, &conversation.id);
        match self.reader.ask(&self.browser, false, &[call]).await {
            Ok(Tab::Open { results, .. }) => {
                let Some(result) = results.first() else {
                    return Outcome::failed("Slack did not answer the sample");
                };
                if result["ok"] != true {
                    return Outcome::failed(
                        result["error"].as_str().unwrap_or("unknown").to_owned(),
                    );
                }
                let count = result["messages"].as_array().map_or(0, Vec::len);
                if count == 0 {
                    return Outcome::skipped("no_conversations");
                }
                Outcome::counted(count)
            }
            Ok(Tab::Missing(_sign_in)) => Outcome::failed_because("slack_not_open"),
            Err(error) => Outcome::failed(error.to_string()),
        }
    }
}

/// What the content can pick from through one of `current`'s browsers: each workspace, its
/// direct messages, and each channel the person is in.
///
/// # Errors
/// [`ApiError::BadRequest`] for a new account or a browser not signed in to Slack, and
/// [`ApiError::NotFound`] for a browser `current` does not have.
pub async fn options(
    state: &AppState,
    current: &CurrentUser,
    account: AccountChoice,
) -> Result<Value, ApiError> {
    let AccountChoice::Existing { id } = account else {
        return Err(sign_in_first());
    };
    let browser = owned_browser(state, id, current.id()).await?;
    let reader = Slack::new(state.connectors.ingest.devtools().clone());
    let not_open = || {
        ApiError::BadRequest(
            "the browser shows no signed-in Slack: open app.slack.com in it and sign in".to_owned(),
        )
    };
    let teams = match reader.ask(&browser, false, &[]).await {
        Ok(Tab::Open { teams, .. }) => teams,
        Ok(Tab::Missing(_sign_in)) => return Err(not_open()),
        Err(error) => return Err(ApiError::BadRequest(error.to_string())),
    };
    let calls: Vec<slack::Call> = teams
        .iter()
        .map(|team| slack::channel_list_call(&team.id))
        .collect();
    let results = match reader.ask(&browser, false, &calls).await {
        Ok(Tab::Open { results, .. }) => results,
        Ok(Tab::Missing(_sign_in)) => return Err(not_open()),
        Err(error) => return Err(ApiError::BadRequest(error.to_string())),
    };
    Ok(json!({ "conversations": slack::options_of(&teams, &results) }))
}

/// Connects Slack through one of `current`'s browsers, reading `content`.
///
/// # Errors
/// [`ApiError::BadRequest`] for a new account (a browser is signed in to on its own page) or
/// content of the wrong shape, [`ApiError::NotFound`] for a browser `current` does not have,
/// and [`ApiError::Conflict`] when the browser already feeds Slack.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    account: AccountChoice,
    content: Value,
) -> Result<Connector, ApiError> {
    let AccountChoice::Existing { id } = account else {
        return Err(sign_in_first());
    };
    content_of(content.clone())?;
    owned_browser(state, id, current.id()).await?;
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let mut new = NewConnector::new(
        ConnectorKind::Slack.as_str(),
        AccountRef::Browser(id),
        current.id(),
    );
    new.content = content;
    let created = connector::create(&mut connection, new)
        .await
        .map_err(|error| match error {
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            ) => ApiError::Conflict("this browser already reads Slack"),
            other => other.into(),
        })?;
    Ok(created)
}

/// Replaces what the connector reads. Conversations it stops reading keep their place in its
/// state, so reading one again later starts where it left off.
///
/// # Errors
/// [`ApiError::BadRequest`] for content of the wrong shape.
pub async fn update_content(
    state: &AppState,
    stored: &Connector,
    content: Value,
) -> Result<Connector, ApiError> {
    content_of(content.clone())?;
    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    let updated = connector::set_content(
        &mut connection,
        stored.id,
        content,
        stored.storage_location_id,
    )
    .await?;
    state.connectors.activity.changed(stored.id);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_is_a_workspace_a_conversation_or_a_workspaces_direct_messages() {
        for good in [
            "T01ABC",
            "T01ABC/C02DEF",
            "T01ABC/dms",
            "E0ENTERPRISE/G0PRIVATE",
        ] {
            assert!(is_pick(good), "{good}");
        }
        for bad in [
            "",
            "T01/",
            "/C02",
            "T01/C02/extra",
            "T 01",
            "T01/../C02",
            "T01/#general",
        ] {
            assert!(!is_pick(bad), "{bad}");
        }
    }

    #[test]
    fn content_is_everything_or_at_least_one_pick_of_the_right_shape() {
        assert!(matches!(
            content_of(json!({ "conversations": "*" })),
            Ok(Content {
                conversations: Selection::All
            })
        ));
        content_of(json!({ "conversations": ["T01", "T01/dms"] })).expect("accepted");
        content_of(json!({ "conversations": [] })).expect_err("refused");
        content_of(json!({ "conversations": ["T01/x y"] })).expect_err("refused");
        content_of(json!({ "conversations": "*", "other": 1 })).expect_err("refused");
        content_of(json!({})).expect_err("refused");
    }
}
