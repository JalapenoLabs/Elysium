// Copyright © 2026 Jalapeno Labs

//! Slack: a person's workspaces, read through Slack's web client in a browser they signed in
//! to. See `docs/connectors.md`, Slack, and `crate::connectors::slack` for the reading.
//!
//! The account is the connector's own browser, as for Outlook and Gmail, started with it at
//! Slack's web client (`super::browser`): nothing is installed into a Slack workspace. Its
//! person signs in on the stream the connector's page shows. Its content picks the workspaces,
//! conversations, and direct messages it reads, and whether their huddles' transcripts are read
//! as meetings (`crate::connectors::slack::meetings`), which they are unless its person turns it
//! off.

use chrono::Utc;
use serde_json::{Value, json};

use super::browser::{browser_of, owned_browser, starts_with_its_browser};
use super::{AccountChoice, ContentField, ContentFieldType, Definition, Provides, Replies, Source};
use crate::auth::CurrentUser;
use crate::browser::{BrowserState, Browsers};
use crate::connectors::checks::{CheckSpec, Checks, Outcome};
use crate::connectors::slack::meetings::{SlackMeetings, SlackSignIn};
use crate::connectors::slack::{self, Content, Counts, START_URL, Slack, Tab, Team, count_teams};
use crate::connectors::{
    AccountKind, ConnectorError, ConnectorKind, Outgoing, Scope, Sent, SignInState,
};
use crate::errors::ApiError;
use crate::meetings::WAIT_FOR_APPROVAL_FIELD;
use crate::meetings::source::{ListedMeeting, MeetingSource, SourceError};
use crate::models::connector::{self, Connector};
use crate::models::draft::ReplyPlace;
use crate::models::timeline_record::TimelineRecord;
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
    // Huddles are optional: a plan without huddle transcripts still reads every message.
    CheckSpec {
        id: "list_meetings",
        required: false,
        depends_on: &["workspace_reachable"],
    },
    CheckSpec {
        id: "sample_transcript",
        required: false,
        depends_on: &["list_meetings"],
    },
];

/// The content field that turns reading huddles as meetings on or off.
const READ_MEETINGS_FIELD: &str = "readMeetings";

/// How far back the huddle check looks, as far as a new connector's first listing reaches.
const SAMPLE_DAYS: i64 = 14;
/// How many huddles the huddle check lists.
const SAMPLE_SIZE: u32 = 5;

pub const DEFINITION: Definition = Definition {
    kind: ConnectorKind::Slack,
    scope: Scope::Personal,
    account: AccountKind::Browser,
    provides: &[Provides::Messages, Provides::Meetings],
    start_url: Some(START_URL),
    content: &[
        ContentField {
            id: "conversations",
            field: ContentFieldType::Selection,
            required: true,
        },
        ContentField {
            id: READ_MEETINGS_FIELD,
            field: ContentFieldType::Toggle {
                on_by_default: true,
            },
            required: false,
        },
        ContentField {
            id: WAIT_FOR_APPROVAL_FIELD,
            field: ContentFieldType::Toggle {
                on_by_default: false,
            },
            required: false,
        },
    ],
    checks: CHECKS,
    replies: Replies::Connector {
        places: &[ReplyPlace::Thread, ReplyPlace::Conversation],
    },
};

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

pub async fn prepare_checks(state: &AppState, source: Source<'_>) -> Result<SlackChecks, ApiError> {
    let Source::Stored(stored) = source else {
        return Err(starts_with_its_browser());
    };
    let content = slack::parse_content(&stored.content)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let browser = browser_of(state, stored).await?;
    Ok(SlackChecks {
        reader: Slack::new(state.connectors.ingest.devtools().clone()),
        huddles: state.meeting_sources.slack().clone(),
        browsers: state.browsers.clone(),
        sign_in: SlackSignIn {
            browser,
            content: content.clone(),
        },
        content,
        counted: None,
        meetings: Vec::new(),
    })
}

/// Slack's checks against one browser.
pub struct SlackChecks {
    reader: Slack,
    huddles: SlackMeetings,
    browsers: Browsers,
    /// The connector's browser and content, which its huddles are read with.
    sign_in: SlackSignIn,
    content: Content,
    /// The workspaces Slack answered, read once and shared by every check that needs them.
    counted: Option<Result<Vec<(Team, Counts)>, Outcome>>,
    /// The huddles the listing check found, the newest of which the transcript check reads.
    meetings: Vec<ListedMeeting>,
}

impl std::fmt::Debug for SlackChecks {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SlackChecks")
            .field("browser", &self.sign_in.browser.id)
            .finish_non_exhaustive()
    }
}

impl Checks for SlackChecks {
    async fn check(&mut self, id: &'static str) -> Outcome {
        match id {
            "browser_running" => {
                let status = self.browsers.status(self.sign_in.browser.id).await;
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
            "list_meetings" => self.list_meetings().await,
            "sample_transcript" => self.sample_transcript().await,
            unknown => Outcome::failed(format!("Slack has no check {unknown}")),
        }
    }
}

impl SlackChecks {
    /// The workspaces the browser is signed in to, with their counts, read once.
    async fn counted(&mut self) -> Result<Vec<(Team, Counts)>, Outcome> {
        if self.counted.is_none() {
            let counted = match self.reader.ask(&self.sign_in.browser, true, &[]).await {
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
        match self.reader.ask(&self.sign_in.browser, false, &[call]).await {
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

    /// Lists the huddles of the last two weeks that it reads as meetings.
    async fn list_meetings(&mut self) -> Outcome {
        if !self.content.read_meetings {
            return Outcome::skipped("meetings_off");
        }
        let since = Utc::now() - chrono::Duration::days(SAMPLE_DAYS);
        match self
            .huddles
            .list(&self.sign_in, Some(since), SAMPLE_SIZE)
            .await
        {
            Ok(meetings) => {
                self.meetings = meetings;
                Outcome::counted(self.meetings.len())
            }
            Err(error) => Outcome::failed(error.to_string()),
        }
    }

    /// Reads the newest listed huddle's transcript.
    async fn sample_transcript(&mut self) -> Outcome {
        let Some(meeting) = self.meetings.first() else {
            return Outcome::skipped("no_meetings");
        };
        match self
            .huddles
            .fetch(&self.sign_in, &meeting.external_id)
            .await
        {
            Ok(fetched) => Outcome {
                message: Some(meeting.title.clone()),
                ..Outcome::counted(fetched.transcript.len())
            },
            Err(SourceError::NotFound) => Outcome::skipped("meeting_unavailable"),
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
        return Err(starts_with_its_browser());
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

/// Connects Slack for `current`, reading `content`, with a browser of its own to sign in on.
///
/// # Errors
/// [`ApiError::BadRequest`] for content of the wrong shape, and as `super::browser::create`.
pub async fn create(
    state: &AppState,
    current: &CurrentUser,
    content: Value,
) -> Result<Connector, ApiError> {
    content_of(content.clone())?;
    super::browser::create(state, current, ConnectorKind::Slack, content, None).await
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

/// Where a reply to `record` goes, `place` in its conversation. See
/// `crate::connectors::slack::send`.
///
/// # Errors
/// [`ApiError::BadRequest`] for a record that does not say where its message was said.
pub fn reply_target(record: &TimelineRecord, place: ReplyPlace) -> Result<Value, ApiError> {
    let target = slack::send::reply_target(record, place).map_err(ApiError::BadRequest)?;
    serde_json::to_value(target).map_err(|error| ApiError::Internal(error.into()))
}

/// Posts `outgoing` as the person `connector`'s browser is signed in to Slack as.
///
/// # Errors
/// [`ConnectorError::Refused`] naming why it was not posted, including a connector still
/// waiting for its browser, and [`ConnectorError`] when the browser or the page fails.
pub async fn send(
    state: &AppState,
    connector: &Connector,
    outgoing: Outgoing<'_>,
) -> Result<Sent, ConnectorError> {
    let target: slack::send::Target = serde_json::from_value(outgoing.target.clone())
        .map_err(|error| ConnectorError::Unexpected(format!("a Slack draft's target: {error}")))?;
    let browser = browser_of(state, connector)
        .await
        .map_err(|error| ConnectorError::Refused(error.to_string()))?;
    Slack::new(state.connectors.ingest.devtools().clone())
        .post(&browser, &target, outgoing.body)
        .await
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
                conversations: Selection::All,
                read_meetings: true,
                ..
            })
        ));
        let quiet = content_of(json!({
            "conversations": "*",
            READ_MEETINGS_FIELD: false,
            WAIT_FOR_APPROVAL_FIELD: true,
        }))
        .expect("accepted");
        assert!(!quiet.read_meetings);
        content_of(json!({ "conversations": ["T01", "T01/dms"] })).expect("accepted");
        content_of(json!({ "conversations": [] })).expect_err("refused");
        content_of(json!({ "conversations": ["T01/x y"] })).expect_err("refused");
        content_of(json!({ "conversations": "*", "other": 1 })).expect_err("refused");
        content_of(json!({})).expect_err("refused");
    }
}
