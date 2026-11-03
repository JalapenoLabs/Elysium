// Copyright © 2026 Jalapeno Labs

//! Slack, read through Slack's web client in a browser its person signed in to. See
//! `docs/connectors.md`, Slack.
//!
//! # How it reads
//!
//! The page script (`slack.js`) runs in the browser's tab on `app.slack.com` and calls Slack's
//! own web API with the session the web client keeps, which is how the web client itself loads
//! everything. It answers each workspace the tab is signed in to and what each call said; the
//! session token never leaves the page. Only read methods can be called, so nothing is posted
//! and nothing is marked read.
//!
//! A read is up to three evaluations:
//!
//! 1. `client.counts` for every workspace: each conversation's newest message, and whether the
//!    person's threads have unread replies.
//! 2. `conversations.history` for each conversation with something newer than this connector
//!    has read, and the threads view (`subscriptions.thread.getView`) where it has unread
//!    replies.
//! 3. `users.info` and `conversations.info` for names not yet known.
//!
//! # Where it left off
//!
//! The reader's state ([`State`]) keeps the newest message read in each conversation and the
//! names it has looked up. A conversation it has never read is baselined at its newest message,
//! so connecting Slack, or joining a channel, never pulls its history in.
//!
//! # What wants attention
//!
//! A direct or group message, a message that mentions the person, and a reply in a thread they
//! follow, unless they wrote it.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use super::{BrowserReader, ConnectorError, ConnectorKind, Read, ReadInput, Reading, SignInState};
use crate::browser::devtools::{Devtools, DevtoolsError};
use crate::models::browser_session::BrowserSession;
use crate::routes::v1::jira_credentials::Selection;

/// Where the web client runs: a tab here is read.
pub const APP_ORIGINS: &[&str] = &["https://app.slack.com/"];

/// The page a browser for Slack opens at, and the one opened when no Slack tab is open.
pub const START_URL: &str = "https://app.slack.com/client";

const SCRIPT: &str = include_str!("slack.js");
const REQUEST_PLACEHOLDER: &str = "__ELYSIUM_REQUEST__";

/// The most conversations one read fetches. The rest keep their place and are read next time,
/// so a burst across many channels never makes one read slow.
const CONVERSATIONS_PER_READ: usize = 25;
/// The most messages one conversation's fetch returns; Slack caps a page at 1000.
const MESSAGES_PER_CONVERSATION: u32 = 100;
/// The most names one read looks up.
const LOOKUPS_PER_READ: usize = 40;
/// The most names the state remembers. Past it the cache starts again, which only costs lookups.
const NAMES_REMEMBERED: usize = 2000;
/// The longest message kept on a record; the message itself stays in Slack.
const BODY_MAX_CHARACTERS: usize = 4000;
/// Matches `timeline_records`' checks on the columns these fill.
const ACTOR_MAX_CHARACTERS: usize = 512;
const TITLE_MAX_CHARACTERS: usize = 2048;

/// Slack's errors for a session that is no longer signed in.
const SIGNED_OUT_ERRORS: &[&str] = &[
    "invalid_auth",
    "not_authed",
    "token_revoked",
    "token_expired",
    "account_inactive",
];

/// Message subtypes that are someone's words; the rest (joins, topic changes) are not recorded.
const RECORDED_SUBTYPES: &[&str] = &[
    "thread_broadcast",
    "file_share",
    "bot_message",
    "me_message",
];

/// What a Slack connector reads: workspaces (`T1`), one conversation (`T1/C1`), or a
/// workspace's direct and group messages (`T1/dms`); `"*"` for everything.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Content {
    pub conversations: Selection<String>,
}

impl Content {
    pub const EVERYTHING: Self = Self {
        conversations: Selection::All,
    };

    /// Whether a conversation of `team`'s is read.
    pub fn reads(&self, team: &str, conversation: &str, kind: ConversationKind) -> bool {
        match &self.conversations {
            Selection::All => true,
            Selection::Only(picked) => picked.iter().any(|entry| {
                entry == team
                    || *entry == format!("{team}/{conversation}")
                    || (kind != ConversationKind::Channel && *entry == format!("{team}/dms"))
            }),
        }
    }

    /// Whether anything of `team`'s is read, which decides whether its threads are.
    pub fn reads_team(&self, team: &str) -> bool {
        match &self.conversations {
            Selection::All => true,
            Selection::Only(picked) => picked.iter().any(|entry| {
                entry == team
                    || entry
                        .split_once('/')
                        .is_some_and(|(owner, _)| owner == team)
            }),
        }
    }
}

/// Where a read left off. Stored as `Reading::state`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// The newest message read in each conversation, by `team/conversation`.
    #[serde(default)]
    pub conversations: BTreeMap<String, String>,
    /// Display names looked up, by `team/user` or `team/conversation`.
    #[serde(default)]
    pub names: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationKind {
    Channel,
    /// A group direct message (`mpim`).
    Group,
    Direct,
}

/// One conversation as `client.counts` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub kind: ConversationKind,
    /// The newest message's `ts`, when it has one.
    pub latest: Option<String>,
}

/// One workspace as `client.counts` answered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counts {
    pub conversations: Vec<Conversation>,
    pub threads_unread: bool,
}

/// A workspace the tab is signed in to, as the page script answers it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub user_id: String,
    /// What `client.counts` answered, when it was asked.
    #[serde(default)]
    pub counts: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    signed_in: bool,
    teams: Vec<Team>,
    #[serde(default)]
    results: Vec<Value>,
}

/// One web API call the page script makes for a workspace.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Call {
    team: String,
    method: &'static str,
    params: Value,
}

#[derive(Serialize)]
struct Request<'calls> {
    counts: bool,
    calls: &'calls [Call],
}

/// What the tab showed when it was asked.
#[derive(Debug)]
pub enum Tab {
    /// The web client answered.
    Open {
        teams: Vec<Team>,
        results: Vec<Value>,
    },
    /// No tab is on the web client: the session ended, or nothing was open.
    Missing(SignInState),
}

#[derive(Debug, Clone)]
pub struct Slack {
    devtools: Devtools,
}

impl Slack {
    pub const fn new(devtools: Devtools) -> Self {
        Self { devtools }
    }

    /// Asks the web client in `browser`'s tab for every workspace, their counts when `counts`,
    /// and `calls`. With no Slack tab open at all, one is opened for the next read.
    ///
    /// # Errors
    /// [`ConnectorError`] when the browser does not answer or the script fails.
    pub async fn ask(
        &self,
        browser: &BrowserSession,
        counts: bool,
        calls: &[Call],
    ) -> Result<Tab, ConnectorError> {
        let port = u16::try_from(browser.port).map_err(|_overflow| {
            ConnectorError::Unexpected("a browser port out of range".to_owned())
        })?;
        let tabs = self.devtools.tabs(port).await?;
        if !tabs.iter().any(|tab| is_app(&tab.url)) {
            if tabs.iter().any(|tab| is_sign_in(&tab.url)) {
                return Ok(Tab::Missing(SignInState::SignedOut));
            }
            let start = Url::parse(START_URL).expect("Slack's start page parses");
            self.devtools.open_tab(port, &start).await?;
            return Ok(Tab::Missing(SignInState::Unknown));
        }

        let request = serde_json::to_string(&Request { counts, calls })
            .map_err(|error| ConnectorError::Unexpected(error.to_string()))?;
        let script = SCRIPT.replace(REQUEST_PLACEHOLDER, &request);
        let value = match self.devtools.evaluate(port, APP_ORIGINS, &script).await {
            Ok(value) => value,
            Err(DevtoolsError::NoTab(_origins)) => return Ok(Tab::Missing(SignInState::Unknown)),
            Err(error) => return Err(error.into()),
        };
        let answer: Answer = serde_json::from_value(value)
            .map_err(|error| ConnectorError::Unexpected(error.to_string()))?;
        if !answer.signed_in {
            return Ok(Tab::Missing(SignInState::SignedOut));
        }
        Ok(Tab::Open {
            teams: answer.teams,
            results: answer.results,
        })
    }

    async fn read_from(
        &self,
        browser: &BrowserSession,
        content: &Content,
        mut state: State,
    ) -> Result<Reading, ConnectorError> {
        let teams = match self.ask(browser, true, &[]).await? {
            Tab::Open { teams, .. } => teams,
            Tab::Missing(sign_in) => return Ok(Reading::nothing(sign_in)),
        };
        let counted = count_teams(&teams)?;
        if counted.is_empty() {
            return Ok(Reading::nothing(SignInState::SignedOut));
        }

        let plan = plan(&counted, content, &state);
        state.conversations.extend(plan.baselined);
        let fetched = if plan.calls.is_empty() {
            Vec::new()
        } else {
            match self.ask(browser, false, &plan.calls).await? {
                Tab::Open { results, .. } => results,
                Tab::Missing(sign_in) => return Ok(Reading::nothing(sign_in)),
            }
        };
        let messages = collect(&plan.fetches, &fetched, &mut state);

        let lookups = lookups(&messages, &state);
        if !lookups.is_empty()
            && let Tab::Open { results, .. } = self.ask(browser, false, &lookups).await?
        {
            remember_names(&lookups, &results, &mut state);
        }

        let items = reads(&messages, &counted, &state);
        Ok(Reading {
            sign_in: SignInState::SignedIn,
            items,
            state: Some(serde_json::to_value(&state).unwrap_or(Value::Null)),
        })
    }
}

impl BrowserReader for Slack {
    fn kind(&self) -> ConnectorKind {
        ConnectorKind::Slack
    }

    async fn read(
        &self,
        browser: &BrowserSession,
        input: ReadInput<'_>,
    ) -> Result<Reading, ConnectorError> {
        let content = parse_content(input.content)?;
        let state: State = serde_json::from_value(input.state.clone()).unwrap_or_default();
        self.read_from(browser, &content, state).await
    }
}

/// What a Slack connector reads, from its stored `content`. Content with no selection reads
/// everything, as a connector made before it chose any does.
///
/// # Errors
/// [`ConnectorError::Unexpected`] for content of the wrong shape.
pub fn parse_content(content: &Value) -> Result<Content, ConnectorError> {
    if content.is_null() || content.as_object().is_some_and(serde_json::Map::is_empty) {
        return Ok(Content::EVERYTHING);
    }
    serde_json::from_value(content.clone())
        .map_err(|error| ConnectorError::Unexpected(format!("Slack content: {error}")))
}

fn is_app(url: &str) -> bool {
    APP_ORIGINS.iter().any(|origin| url.starts_with(origin))
}

/// A Slack page that is not the web client: a sign-in page, or a workspace's own address,
/// which redirects to sign in.
fn is_sign_in(url: &str) -> bool {
    Url::parse(url).is_ok_and(|parsed| {
        parsed.scheme() == "https"
            && parsed
                .host_str()
                .is_some_and(|host| host == "slack.com" || host.ends_with(".slack.com"))
    })
}

/// Each workspace's counts, for the workspaces Slack answered. A workspace whose session ended
/// is left out, so a tab signed out of every workspace answers none.
///
/// # Errors
/// [`ConnectorError::Unexpected`] when every workspace failed for a reason other than signing
/// out, naming the first.
pub fn count_teams(teams: &[Team]) -> Result<Vec<(Team, Counts)>, ConnectorError> {
    let mut counted = Vec::new();
    let mut first_failure = None;
    for team in teams {
        match team.counts.as_ref().map(parse_counts) {
            Some(Ok(counts)) => counted.push((team.clone(), counts)),
            Some(Err(error)) if SIGNED_OUT_ERRORS.contains(&error.as_str()) => {}
            Some(Err(error)) => {
                first_failure.get_or_insert(error);
            }
            None => {
                first_failure.get_or_insert_with(|| "client.counts was not answered".to_owned());
            }
        }
    }
    if counted.is_empty()
        && let Some(error) = first_failure
    {
        return Err(ConnectorError::Unexpected(format!(
            "Slack answered client.counts with {error}"
        )));
    }
    Ok(counted)
}

/// What `client.counts` answered, or Slack's error code.
///
/// # Errors
/// Slack's `error` when it did not answer `ok`.
pub fn parse_counts(answer: &Value) -> Result<Counts, String> {
    if answer["ok"] != true {
        return Err(answer["error"].as_str().unwrap_or("unknown").to_owned());
    }
    let mut conversations = Vec::new();
    for (list, kind) in [
        ("channels", ConversationKind::Channel),
        ("mpims", ConversationKind::Group),
        ("ims", ConversationKind::Direct),
    ] {
        for entry in answer[list].as_array().into_iter().flatten() {
            let Some(id) = entry["id"].as_str() else {
                continue;
            };
            conversations.push(Conversation {
                id: id.to_owned(),
                kind,
                latest: entry["latest"].as_str().map(str::to_owned),
            });
        }
    }
    Ok(Counts {
        conversations,
        threads_unread: answer["threads"]["has_unreads"] == true,
    })
}

/// A fetch the second evaluation makes, in the same order as its call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    History {
        team: String,
        conversation: String,
        kind: ConversationKind,
        /// The conversation's newest message, which the state moves to once it is read.
        latest: String,
    },
    Threads {
        team: String,
    },
}

/// What a read fetches, and the conversations it baselines without fetching.
#[derive(Debug, Default)]
pub struct Plan {
    pub fetches: Vec<Fetch>,
    pub calls: Vec<Call>,
    pub baselined: Vec<(String, String)>,
}

/// The conversations with something newer than `state` read, newest first and at most
/// [`CONVERSATIONS_PER_READ`]; the threads views of workspaces with unread replies; and the
/// conversations never read before, baselined at their newest message.
pub fn plan(counted: &[(Team, Counts)], content: &Content, state: &State) -> Plan {
    let mut planned = Plan::default();
    let mut behind: Vec<(&Team, &Conversation, &str, &str)> = Vec::new();
    for (team, counts) in counted {
        for conversation in &counts.conversations {
            if !content.reads(&team.id, &conversation.id, conversation.kind) {
                continue;
            }
            let Some(latest) = conversation.latest.as_deref() else {
                continue;
            };
            let key = format!("{}/{}", team.id, conversation.id);
            match state.conversations.get(&key) {
                None => planned.baselined.push((key, latest.to_owned())),
                Some(read) if ts_after(latest, read) => {
                    behind.push((team, conversation, latest, read.as_str()));
                }
                Some(_) => {}
            }
        }
        if counts.threads_unread && content.reads_team(&team.id) {
            planned.fetches.push(Fetch::Threads {
                team: team.id.clone(),
            });
            planned.calls.push(Call {
                team: team.id.clone(),
                method: "subscriptions.thread.getView",
                params: json!({ "limit": 25 }),
            });
        }
    }

    behind.sort_by(|left, right| ts_order(right.2, left.2));
    for (team, conversation, latest, read) in behind.into_iter().take(CONVERSATIONS_PER_READ) {
        planned.fetches.push(Fetch::History {
            team: team.id.clone(),
            conversation: conversation.id.clone(),
            kind: conversation.kind,
            latest: latest.to_owned(),
        });
        planned.calls.push(Call {
            team: team.id.clone(),
            method: "conversations.history",
            params: json!({
                "channel": conversation.id,
                "oldest": read,
                "inclusive": false,
                "limit": MESSAGES_PER_CONVERSATION,
            }),
        });
    }
    planned
}

/// A message read, before it becomes a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub team: String,
    pub conversation: String,
    pub kind: ConversationKind,
    pub ts: String,
    /// The sender's user id, or a bot's name when it has no user.
    pub user: Option<String>,
    pub bot: Option<String>,
    pub text: String,
    /// The thread it replies in, for a reply.
    pub thread_ts: Option<String>,
}

/// Every message the fetches answered, and the state moved on past what they read. A fetch that
/// failed keeps its conversation's place, so it is read again next time.
pub fn collect(fetches: &[Fetch], results: &[Value], state: &mut State) -> Vec<Message> {
    let mut messages = Vec::new();
    for (fetch, result) in fetches.iter().zip(results) {
        if result["ok"] != true {
            continue;
        }
        match fetch {
            Fetch::History {
                team,
                conversation,
                kind,
                latest,
            } => {
                for entry in result["messages"].as_array().into_iter().flatten() {
                    if let Some(message) = message_of(team, conversation, *kind, entry) {
                        messages.push(message);
                    }
                }
                // Slack named this the newest message; a page of fewer messages than that
                // (hidden, deleted, or past the page) must not be fetched again every read.
                state
                    .conversations
                    .insert(format!("{team}/{conversation}"), latest.clone());
            }
            Fetch::Threads { team } => {
                for thread in result["threads"].as_array().into_iter().flatten() {
                    let Some(conversation) = thread["root_msg"]["channel"].as_str() else {
                        continue;
                    };
                    let kind = conversation_kind(conversation);
                    for entry in thread["unread_replies"].as_array().into_iter().flatten() {
                        if let Some(message) = message_of(team, conversation, kind, entry)
                            && message.thread_ts.is_some()
                        {
                            messages.push(message);
                        }
                    }
                }
            }
        }
    }
    messages
}

/// A conversation's kind from its id, for one the threads view names: Slack's ids start with
/// `D` for a direct message and `G` for a group or private conversation.
fn conversation_kind(id: &str) -> ConversationKind {
    if id.starts_with('D') {
        ConversationKind::Direct
    } else {
        ConversationKind::Channel
    }
}

fn message_of(
    team: &str,
    conversation: &str,
    kind: ConversationKind,
    entry: &Value,
) -> Option<Message> {
    let ts = entry["ts"].as_str()?;
    if let Some(subtype) = entry["subtype"].as_str()
        && !RECORDED_SUBTYPES.contains(&subtype)
    {
        return None;
    }
    let thread_ts = entry["thread_ts"]
        .as_str()
        .filter(|thread| *thread != ts)
        .map(str::to_owned);
    let bot = entry["username"]
        .as_str()
        .or_else(|| entry["bot_profile"]["name"].as_str())
        .map(str::to_owned);
    Some(Message {
        team: team.to_owned(),
        conversation: conversation.to_owned(),
        kind,
        ts: ts.to_owned(),
        user: entry["user"].as_str().map(str::to_owned),
        bot,
        text: entry["text"].as_str().unwrap_or_default().to_owned(),
        thread_ts,
    })
}

/// The names `messages` need that the state does not know: each sender, and each channel.
pub fn lookups(messages: &[Message], state: &State) -> Vec<Call> {
    let mut wanted: BTreeSet<(String, &'static str, String)> = BTreeSet::new();
    for message in messages {
        if let Some(user) = &message.user
            && !state
                .names
                .contains_key(&format!("{}/{user}", message.team))
        {
            wanted.insert((message.team.clone(), "users.info", user.clone()));
        }
        if message.kind == ConversationKind::Channel
            && !state
                .names
                .contains_key(&format!("{}/{}", message.team, message.conversation))
        {
            wanted.insert((
                message.team.clone(),
                "conversations.info",
                message.conversation.clone(),
            ));
        }
    }
    wanted
        .into_iter()
        .take(LOOKUPS_PER_READ)
        .map(|(team, method, id)| {
            let params = if method == "users.info" {
                json!({ "user": id })
            } else {
                json!({ "channel": id })
            };
            Call {
                team,
                method,
                params,
            }
        })
        .collect()
}

/// Keeps each name a lookup found.
pub fn remember_names(lookups: &[Call], results: &[Value], state: &mut State) {
    if state.names.len() > NAMES_REMEMBERED {
        state.names.clear();
    }
    for (call, result) in lookups.iter().zip(results) {
        if result["ok"] != true {
            continue;
        }
        let (id, name) = if call.method == "users.info" {
            let user = &result["user"];
            let profile = &user["profile"];
            let name = [
                &profile["display_name"],
                &profile["real_name"],
                &user["real_name"],
                &user["name"],
            ]
            .into_iter()
            .filter_map(Value::as_str)
            .find(|name| !name.trim().is_empty());
            (call.params["user"].as_str(), name)
        } else {
            (
                call.params["channel"].as_str(),
                result["channel"]["name"].as_str(),
            )
        };
        if let (Some(id), Some(name)) = (id, name) {
            state
                .names
                .insert(format!("{}/{id}", call.team), name.trim().to_owned());
        }
    }
}

/// The records `messages` become, newest first.
pub fn reads(messages: &[Message], counted: &[(Team, Counts)], state: &State) -> Vec<Read> {
    let teams: BTreeMap<&str, &Team> = counted
        .iter()
        .map(|(team, _counts)| (team.id.as_str(), team))
        .collect();
    let mut items: Vec<(&str, Read)> = messages
        .iter()
        .filter_map(|message| {
            let team = teams.get(message.team.as_str())?;
            Some((message.ts.as_str(), read_of(message, team, state)))
        })
        .collect();
    items.sort_by(|left, right| ts_order(right.0, left.0));
    items.into_iter().map(|(_ts, read)| read).collect()
}

fn read_of(message: &Message, team: &Team, state: &State) -> Read {
    let name_of = |id: &str| state.names.get(&format!("{}/{id}", team.id)).cloned();
    let sender = message
        .user
        .as_deref()
        .and_then(name_of)
        .or_else(|| message.bot.clone())
        .or_else(|| message.user.clone())
        .unwrap_or_else(|| "Slack".to_owned());
    let place = match message.kind {
        ConversationKind::Channel => format!(
            "#{}",
            name_of(&message.conversation).unwrap_or_else(|| message.conversation.clone())
        ),
        ConversationKind::Group => "a group message".to_owned(),
        ConversationKind::Direct => "a direct message".to_owned(),
    };
    let title = match (&message.thread_ts, message.kind) {
        (Some(_), _) => format!("Reply in {place} in {}", team.name),
        (None, ConversationKind::Channel) => format!("{place} in {}", team.name),
        (None, _) => format!("{sender} in {place} in {}", team.name),
    };
    let own = !team.user_id.is_empty() && message.user.as_deref() == Some(team.user_id.as_str());
    let mentioned =
        !team.user_id.is_empty() && message.text.contains(&format!("<@{}>", team.user_id));
    let wants_attention = !own
        && (message.kind != ConversationKind::Channel || mentioned || message.thread_ts.is_some());

    Read {
        external_id: format!("{}/{}/{}", team.id, message.conversation, message.ts),
        occurred_at: ts_time(&message.ts),
        actor: truncate(&sender, ACTOR_MAX_CHARACTERS),
        title: truncate(&title, TITLE_MAX_CHARACTERS),
        body: truncate(
            &plain_text(&message.text, &team.id, state),
            BODY_MAX_CHARACTERS,
        ),
        url: permalink(team, message),
        wants_attention,
    }
}

/// The message's address in Slack, as its "Copy link" gives it.
fn permalink(team: &Team, message: &Message) -> Option<String> {
    let base = Url::parse(&team.url).ok()?;
    let mut url = base
        .join(&format!(
            "archives/{}/p{}",
            message.conversation,
            message.ts.replace('.', "")
        ))
        .ok()?;
    if let Some(thread) = &message.thread_ts {
        url.query_pairs_mut()
            .append_pair("thread_ts", thread)
            .append_pair("cid", &message.conversation);
    }
    Some(url.into())
}

/// Slack's markup as plain text: `<@U1>` as `@name`, `<#C1|general>` as `#general`, a link
/// as its label or address, and its three escaped characters back.
pub fn plain_text(text: &str, team: &str, state: &State) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        plain.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            plain.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let inner = &rest[start + 1..start + end];
        let (target, label) = inner.split_once('|').unwrap_or((inner, ""));
        let shown = if let Some(user) = target.strip_prefix('@') {
            let name = state.names.get(&format!("{team}/{user}"));
            format!("@{}", name.map_or(user, String::as_str))
        } else if let Some(channel) = target.strip_prefix('#') {
            let name = (!label.is_empty()).then_some(label).or_else(|| {
                state
                    .names
                    .get(&format!("{team}/{channel}"))
                    .map(String::as_str)
            });
            format!("#{}", name.unwrap_or(channel))
        } else if let Some(special) = target.strip_prefix('!') {
            format!("@{}", special.split('^').next().unwrap_or(special))
        } else if label.is_empty() {
            target.to_owned()
        } else {
            label.to_owned()
        };
        plain.push_str(&shown);
        rest = &rest[start + end + 1..];
    }
    plain.push_str(rest);
    plain
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// A `ts` (`1730000000.000100`, seconds and microseconds) as a time.
fn ts_time(ts: &str) -> Option<DateTime<Utc>> {
    let (seconds, micros) = ts.split_once('.').unwrap_or((ts, "0"));
    let seconds: i64 = seconds.parse().ok()?;
    let micros: u32 = format!("{micros:0<6}").get(..6)?.parse().ok()?;
    DateTime::from_timestamp(seconds, micros * 1000)
}

/// A fetch of `conversation`'s newest few messages, for a check's sample.
pub fn sample_call(team: &str, conversation: &str) -> Call {
    Call {
        team: team.to_owned(),
        method: "conversations.history",
        params: json!({ "channel": conversation, "limit": 5 }),
    }
}

/// The channels `team`'s person is in, for the options a connector's content picks from.
/// Group and direct messages are picked together, as the workspace's `dms`.
pub fn channel_list_call(team: &str) -> Call {
    Call {
        team: team.to_owned(),
        method: "users.conversations",
        params: json!({
            "types": "public_channel,private_channel",
            "exclude_archived": true,
            "limit": 200,
        }),
    }
}

/// The options a connector's content picks from, `{ options, truncated }`, from each
/// workspace and what [`channel_list_call`] answered for it, in order. An option's words that
/// are Elysium's rather than Slack's are named by key (`labelKey`, `detailKey`) for the web app
/// to translate.
pub fn options_of(teams: &[Team], results: &[Value]) -> Value {
    let mut options = Vec::new();
    let mut truncated = false;
    for (team, result) in teams.iter().zip(results) {
        options.push(json!({ "id": team.id, "label": team.name, "detailKey": "slack_workspace" }));
        options.push(json!({
            "id": format!("{}/dms", team.id),
            "labelKey": "slack_direct_messages",
            "detail": team.name,
        }));
        for channel in result["channels"].as_array().into_iter().flatten() {
            let (Some(id), Some(name)) = (channel["id"].as_str(), channel["name"].as_str()) else {
                continue;
            };
            options.push(json!({
                "id": format!("{}/{id}", team.id),
                "label": format!("#{name}"),
                "detail": team.name,
            }));
        }
        truncated |= result["response_metadata"]["next_cursor"]
            .as_str()
            .is_some_and(|cursor| !cursor.is_empty());
    }
    json!({ "options": options, "truncated": truncated })
}

/// Orders two `ts` values as times. Slack's are fixed width in practice, but a shorter seconds
/// part must still sort earlier.
pub fn ts_order(left: &str, right: &str) -> std::cmp::Ordering {
    let key = |ts: &str| {
        let (seconds, micros) = ts.split_once('.').unwrap_or((ts, ""));
        (seconds.parse::<u64>().unwrap_or(0), format!("{micros:0<6}"))
    };
    key(left).cmp(&key(right))
}

fn ts_after(candidate: &str, read: &str) -> bool {
    ts_order(candidate, read) == std::cmp::Ordering::Greater
}

fn truncate(text: &str, max_characters: usize) -> String {
    text.trim().chars().take(max_characters).collect()
}

#[cfg(test)]
mod tests;
