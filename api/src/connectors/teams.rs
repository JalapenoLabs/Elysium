// Copyright © 2026 Jalapeno Labs

//! Microsoft Teams read from its own local store, in a browser its person signed in to.
//!
//! New Teams keeps the chats and channels it has loaded in `IndexedDB`. `teams.js` runs as the
//! Teams page, over `DevTools`, reads those records without touching the page, and answers them
//! close to how they are stored. This module decides what they mean: which conversations the
//! connector reads, which messages are new, and which want the person's attention. Nothing here
//! opens a chat, so reading leaves unread messages unread.
//!
//! # What wants attention
//!
//! A message wants attention when it is unread (it arrived after its conversation's read
//! horizon), someone else sent it, and it is in a chat (one-on-one or group), mentions the
//! person, or replies in a channel thread the person wrote in. Channel chatter that names
//! nobody is recorded on the timeline without raising anything.
//!
//! # Why the local store and not the page
//!
//! The store's records carry each message's id, sender, arrival time, and mentions, and each
//! conversation's read horizon, which the rendered page shows only partly and in markup that
//! changes with every redesign. The store is the web client's own, not a published API, so the
//! script reads it defensively and says what it found when it finds nothing. See
//! `docs/connectors.md`.

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use super::{BrowserReader, ConnectorError, ConnectorKind, Read, ReadInput, Reading, SignInState};
use crate::browser::devtools::{Devtools, DevtoolsError};
use crate::models::browser_session::BrowserSession;

/// Where Teams on the web runs: a tab at one of these is read.
pub const TEAMS_ORIGINS: &[&str] = &[
    "https://teams.cloud.microsoft/",
    "https://teams.microsoft.com/",
    "https://teams.live.com/",
];

/// Where Microsoft asks its user to sign in, for work and for personal accounts.
pub const SIGN_IN_ORIGINS: &[&str] = &[
    "https://login.microsoftonline.com/",
    "https://login.live.com/",
];

/// The page a browser opens to read Teams.
pub const START_URL: &str = "https://teams.cloud.microsoft/";

const SCRIPT: &str = include_str!("teams.js");

/// The most messages one read answers, newest first. A minute's new messages fit with room to
/// spare; a larger backlog is read over the next passes as the watermark moves.
const MAX_MESSAGES: usize = 200;

/// How far before the watermark a read looks again, so a message stored a little after it
/// arrived, or a clock that drifted, is not missed. Records are deduplicated by id, so what is
/// read twice is stored once.
const WATERMARK_OVERLAP_MINUTES: i64 = 10;

/// The longest message text kept on a record; the message itself stays in Teams.
const TEXT_MAX_CHARACTERS: usize = 1000;
/// Matches `timeline_records`' checks on the columns these fill.
const ACTOR_MAX_CHARACTERS: usize = 512;
const TITLE_MAX_CHARACTERS: usize = 2048;

/// The script to evaluate in the Teams tab, reading what arrived after `since`.
pub fn script(since: Option<DateTime<Utc>>) -> String {
    let since = since.map_or(0, |since| {
        (since - Duration::minutes(WATERMARK_OVERLAP_MINUTES)).timestamp_millis()
    });
    let arguments = json!({ "since": since, "maxMessages": MAX_MESSAGES });
    format!("({SCRIPT})({arguments})")
}

/// Which conversations a Teams connector reads: its content, `{ "conversations": "*" }` or a
/// list of conversation ids. Content that names none reads everything, as `"*"` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Every chat and channel the person sees, including ones joined later.
    All,
    Only(HashSet<String>),
}

impl Selection {
    /// The selection a connector's content holds.
    ///
    /// # Errors
    /// When `conversations` is neither `"*"` nor a list of ids, naming what was wrong.
    pub fn from_content(content: &Value) -> Result<Self, String> {
        match &content["conversations"] {
            Value::String(everything) if everything == "*" => Ok(Self::All),
            Value::Array(ids) => {
                let ids: Option<HashSet<String>> = ids
                    .iter()
                    .map(|id| id.as_str().map(str::to_owned))
                    .collect();
                let ids = ids.ok_or("every conversation is named by its id, as text")?;
                if ids.is_empty() {
                    return Err("pick at least one conversation, or \"*\" for all".to_owned());
                }
                Ok(Self::Only(ids))
            }
            Value::Null => Ok(Self::All),
            _ => Err("conversations is \"*\" or a list of ids".to_owned()),
        }
    }

    fn includes(&self, conversation_id: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(ids) => ids.contains(conversation_id),
        }
    }
}

/// A chat or channel, as the connector names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub name: String,
    pub is_chat: bool,
}

/// What one read of Teams found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamsReading {
    pub sign_in: SignInState,
    /// Whether Teams' store was found. False while signed in means the page holds no store the
    /// script recognizes, and `databases` lists what it does hold.
    pub found: bool,
    /// Every conversation the store holds, selected or not, for picking from.
    pub conversations: Vec<Conversation>,
    /// New messages in the selected conversations, newest first.
    pub items: Vec<Read>,
    /// The newest arrival read, selected or not, which the next read starts from.
    pub newest: Option<DateTime<Utc>>,
    pub databases: Vec<String>,
}

impl TeamsReading {
    fn nothing(sign_in: SignInState) -> Self {
        Self {
            sign_in,
            found: false,
            conversations: Vec::new(),
            items: Vec::new(),
            newest: None,
            databases: Vec::new(),
        }
    }
}

/// Reads Teams through a browser its person signed in to.
#[derive(Debug, Clone)]
pub struct Teams {
    devtools: Devtools,
}

impl Teams {
    pub const fn new(devtools: Devtools) -> Self {
        Self { devtools }
    }

    /// Reads what arrived after `since` in the conversations `selection` names, as the browser's
    /// Teams tab has stored it.
    ///
    /// A browser signed in for Outlook is signed in to Microsoft, so Teams opens in it without
    /// asking again. When no tab shows Teams and none is asking to sign in, Teams is opened in a
    /// new tab and this read answers `Unknown`; the next one reads it once it has loaded.
    ///
    /// # Errors
    /// [`ConnectorError`] when the browser does not answer or the script's answer is not its
    /// shape.
    pub async fn read_teams(
        &self,
        browser: &BrowserSession,
        since: Option<DateTime<Utc>>,
        selection: &Selection,
    ) -> Result<TeamsReading, ConnectorError> {
        let port = u16::try_from(browser.port).map_err(|_overflow| {
            ConnectorError::Unexpected("a browser port out of range".to_owned())
        })?;
        let tabs = self.devtools.tabs(port).await?;
        let on_teams = tabs
            .iter()
            .any(|tab| starts_with_any(&tab.url, TEAMS_ORIGINS));
        if !on_teams {
            let asking_to_sign_in = tabs
                .iter()
                .any(|tab| starts_with_any(&tab.url, SIGN_IN_ORIGINS));
            if asking_to_sign_in {
                return Ok(TeamsReading::nothing(SignInState::SignedOut));
            }
            let start_url = Url::parse(START_URL).expect("the Teams start page is a URL");
            self.devtools.open_tab(port, &start_url).await?;
            return Ok(TeamsReading::nothing(SignInState::Unknown));
        }

        match self
            .devtools
            .evaluate(port, TEAMS_ORIGINS, &script(since))
            .await
        {
            Ok(value) => parse_page(&value, selection),
            Err(DevtoolsError::NoTab(_origins)) => Ok(TeamsReading::nothing(SignInState::Unknown)),
            Err(error) => Err(error.into()),
        }
    }
}

impl BrowserReader for Teams {
    fn kind(&self) -> ConnectorKind {
        ConnectorKind::Teams
    }

    /// Reads from the watermark kept in the connector's state, `{ "since": <ms> }`, and answers
    /// the newest arrival read as the next one.
    async fn read(
        &self,
        browser: &BrowserSession,
        input: ReadInput<'_>,
    ) -> Result<Reading, ConnectorError> {
        let since = input.state["since"]
            .as_i64()
            .and_then(DateTime::from_timestamp_millis);
        let selection =
            Selection::from_content(input.content).map_err(ConnectorError::Unexpected)?;
        let reading = self.read_teams(browser, since, &selection).await?;
        Ok(Reading {
            sign_in: reading.sign_in,
            items: reading.items,
            state: reading
                .newest
                .map(|newest| json!({ "since": newest.timestamp_millis() })),
        })
    }
}

fn starts_with_any(url: &str, origins: &[&str]) -> bool {
    origins.iter().any(|origin| url.starts_with(origin))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    signed_in: bool,
    found: bool,
    me: Option<String>,
    #[serde(default)]
    conversations: Vec<PageConversation>,
    #[serde(default)]
    messages: Vec<PageMessage>,
    #[serde(default)]
    my_thread_ids: Vec<String>,
    #[serde(default)]
    databases: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageConversation {
    id: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    thread_type: String,
    consumption_horizon: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageMessage {
    id: String,
    conversation_id: String,
    parent_id: Option<String>,
    created_at_ms: i64,
    from: Option<String>,
    #[serde(default)]
    from_name: String,
    #[serde(default)]
    message_type: String,
    #[serde(default)]
    text: String,
    /// Stored either as a JSON array or as text holding one.
    #[serde(default)]
    mentions: Value,
}

/// What the Teams script answered, as a reading of the conversations `selection` names.
///
/// # Errors
/// [`ConnectorError::Unexpected`] when the answer is not the script's shape.
pub fn parse_page(value: &Value, selection: &Selection) -> Result<TeamsReading, ConnectorError> {
    let page: Page = serde_json::from_value(value.clone())
        .map_err(|error| ConnectorError::Unexpected(error.to_string()))?;
    if !page.signed_in {
        return Ok(TeamsReading {
            sign_in: SignInState::SignedOut,
            found: false,
            conversations: Vec::new(),
            items: Vec::new(),
            newest: None,
            databases: Vec::new(),
        });
    }

    let me = page.me.as_deref();
    let my_threads: HashSet<&str> = page.my_thread_ids.iter().map(String::as_str).collect();
    let conversations: Vec<Conversation> = page
        .conversations
        .iter()
        .map(|conversation| Conversation {
            id: conversation.id.clone(),
            name: conversation_name(conversation, &page.messages, me),
            is_chat: is_chat(conversation),
        })
        .collect();

    let mut newest: Option<DateTime<Utc>> = None;
    let mut items = Vec::new();
    for message in &page.messages {
        let Some(created_at) = DateTime::from_timestamp_millis(message.created_at_ms) else {
            continue;
        };
        newest = newest.max(Some(created_at));
        if !is_written(message) || !selection.includes(&message.conversation_id) {
            continue;
        }
        let Some((stored, conversation)) = page
            .conversations
            .iter()
            .zip(&conversations)
            .find(|(stored, _named)| stored.id == message.conversation_id)
        else {
            continue;
        };

        let from_someone_else = message.from.as_deref() != me || me.is_none();
        let unread = stored
            .consumption_horizon
            .as_deref()
            .and_then(read_horizon)
            .is_none_or(|horizon| message.created_at_ms > horizon);
        let mentions_me =
            me.is_some_and(|me| mentions(&message.mentions).iter().any(|mri| mri == me));
        let in_my_thread = message.parent_id.as_deref().is_some_and(|parent| {
            parent != message.id
                && my_threads.contains(format!("{}/{parent}", message.conversation_id).as_str())
        });
        let wants_attention =
            unread && from_someone_else && (conversation.is_chat || mentions_me || in_my_thread);

        items.push(Read {
            external_id: format!("{}/{}", message.conversation_id, message.id),
            occurred_at: Some(created_at),
            actor: truncate(&message.from_name, ACTOR_MAX_CHARACTERS),
            title: truncate(&conversation.name, TITLE_MAX_CHARACTERS),
            body: truncate(&message.text, TEXT_MAX_CHARACTERS),
            url: message_link(&message.conversation_id, &message.id),
            wants_attention,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        });
    }

    Ok(TeamsReading {
        sign_in: SignInState::SignedIn,
        found: page.found,
        conversations,
        items,
        newest,
        databases: page.databases,
    })
}

/// A chat (one-on-one, group, or a meeting's chat) rather than a channel. One-on-one chats
/// carry no thread type; their ids end in `@unq.gbl.spaces`. Channels are `topic` or `space`,
/// on `@thread.tacv2` ids.
fn is_chat(conversation: &PageConversation) -> bool {
    match conversation.thread_type.as_str() {
        "chat" | "meeting" => true,
        "topic" | "space" => false,
        _ => {
            conversation.id.ends_with("@unq.gbl.spaces") || conversation.id.ends_with("@thread.v2")
        }
    }
}

/// A message someone wrote, as opposed to a system event (a member added, a call started).
fn is_written(message: &PageMessage) -> bool {
    message.message_type.starts_with("RichText") || message.message_type == "Text"
}

/// The time up to which a conversation has been read: the first field of its
/// `consumptionhorizon`, `"<read until ms>;<read until ms>;<message id>"`.
fn read_horizon(horizon: &str) -> Option<i64> {
    horizon.split(';').next()?.trim().parse().ok()
}

/// The MRIs a message mentions, from its `mentions` property, stored as an array or as text.
fn mentions(value: &Value) -> Vec<String> {
    let parsed;
    let entries = match value {
        Value::String(text) => {
            parsed = serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
            &parsed
        }
        other => other,
    };
    entries
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["mri"].as_str().or_else(|| entry["id"].as_str()))
        .map(str::to_owned)
        .collect()
}

/// A conversation's name: its topic, or for a chat without one, who else wrote in it.
fn conversation_name(
    conversation: &PageConversation,
    messages: &[PageMessage],
    me: Option<&str>,
) -> String {
    if !conversation.topic.trim().is_empty() {
        return conversation.topic.trim().to_owned();
    }
    let mut names: Vec<&str> = Vec::new();
    for message in messages {
        let is_mine = me.is_some() && message.from.as_deref() == me;
        let name = message.from_name.trim();
        if message.conversation_id != conversation.id || is_mine || name.is_empty() {
            continue;
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    if names.is_empty() {
        return "Chat".to_owned();
    }
    names.join(", ")
}

/// Teams' documented link to one message, which opens it in whichever Teams the reader uses.
fn message_link(conversation_id: &str, message_id: &str) -> Option<String> {
    let mut url = Url::parse("https://teams.microsoft.com/l/message/").ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .push(conversation_id)
        .push(message_id);
    Some(url.into())
}

fn truncate(text: &str, max_characters: usize) -> String {
    text.trim().chars().take(max_characters).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `teams.js` answered for `fixtures/teams.html`, read in headless Chromium.
    const FIXTURE: &str = include_str!("fixtures/teams_page.json");

    fn fixture_reading(selection: &Selection) -> TeamsReading {
        let value: Value = serde_json::from_str(FIXTURE).expect("the fixture is JSON");
        parse_page(&value, selection).expect("a reading")
    }

    fn item<'reading>(reading: &'reading TeamsReading, message_id: &str) -> &'reading Read {
        reading
            .items
            .iter()
            .find(|item| item.external_id.ends_with(&format!("/{message_id}")))
            .unwrap_or_else(|| panic!("message {message_id} is read"))
    }

    #[test]
    fn an_unread_direct_message_wants_attention() {
        let reading = fixture_reading(&Selection::All);
        let message = item(&reading, "1730000200000");
        assert!(message.wants_attention);
        assert_eq!(message.actor, "Ada Lovelace");
        assert_eq!(message.title, "Ada Lovelace");
        assert_eq!(message.body, "Are we launching Friday?");
    }

    #[test]
    fn the_persons_own_message_never_wants_attention() {
        let reading = fixture_reading(&Selection::All);
        assert!(!item(&reading, "1730000210000").wants_attention);
    }

    #[test]
    fn a_message_already_read_is_recorded_without_wanting_attention() {
        let reading = fixture_reading(&Selection::All);
        let message = item(&reading, "1730000300000");
        assert!(!message.wants_attention);
        assert_eq!(message.title, "Launch planning");
        assert_eq!(message.body, "Notes are in the doc & ready");
    }

    #[test]
    fn a_channel_mention_and_a_reply_in_the_persons_thread_want_attention() {
        let reading = fixture_reading(&Selection::All);
        assert!(item(&reading, "1730000410000").wants_attention, "mention");
        assert!(
            item(&reading, "1730000400000").wants_attention,
            "reply in my thread"
        );
        assert!(
            !item(&reading, "1730000420000").wants_attention,
            "channel chatter naming nobody"
        );
    }

    #[test]
    fn system_events_are_not_recorded() {
        let reading = fixture_reading(&Selection::All);
        assert!(
            !reading
                .items
                .iter()
                .any(|item| item.external_id.ends_with("/1730000310000"))
        );
        assert_eq!(reading.items.len(), 6);
    }

    #[test]
    fn only_the_selected_conversations_are_recorded_but_the_watermark_sees_everything() {
        let selection = Selection::Only(HashSet::from(["19:group1@thread.v2".to_owned()]));
        let reading = fixture_reading(&selection);
        assert_eq!(reading.items.len(), 1);
        assert_eq!(
            reading.newest.map(|newest| newest.timestamp_millis()),
            Some(1_730_000_420_000)
        );
        assert_eq!(reading.conversations.len(), 3);
    }

    #[test]
    fn conversations_are_named_and_told_apart() {
        let reading = fixture_reading(&Selection::All);
        let named: Vec<(&str, bool)> = reading
            .conversations
            .iter()
            .map(|conversation| (conversation.name.as_str(), conversation.is_chat))
            .collect();
        assert!(named.contains(&("Ada Lovelace", true)));
        assert!(named.contains(&("Launch planning", true)));
        assert!(named.contains(&("General", false)));
    }

    #[test]
    fn each_message_links_to_itself_in_teams() {
        let reading = fixture_reading(&Selection::All);
        assert_eq!(
            item(&reading, "1730000400000").url.as_deref(),
            Some("https://teams.microsoft.com/l/message/19:channel1@thread.tacv2/1730000400000")
        );
    }

    #[test]
    fn a_signed_out_page_reads_nothing() {
        let reading = parse_page(
            &json!({ "signedIn": false, "found": false, "me": null }),
            &Selection::All,
        )
        .expect("a reading");
        assert_eq!(reading.sign_in, SignInState::SignedOut);
        assert!(reading.items.is_empty());
    }

    #[test]
    fn a_conversation_without_a_read_horizon_counts_its_messages_as_unread() {
        let reading = parse_page(
            &json!({
                "signedIn": true, "found": true, "me": "8:orgid:me",
                "conversations": [{ "id": "19:x@unq.gbl.spaces" }],
                "messages": [{
                    "id": "1", "conversationId": "19:x@unq.gbl.spaces", "createdAtMs": 1_730_000_000_000_i64,
                    "from": "8:orgid:them", "fromName": "Them", "messageType": "Text", "text": "hi",
                }],
            }),
            &Selection::All,
        )
        .expect("a reading");
        assert!(reading.items[0].wants_attention);
    }

    #[test]
    fn mentions_are_read_from_an_array_or_from_text_holding_one() {
        let entry = json!([{ "mri": "8:orgid:me" }]);
        assert_eq!(mentions(&entry), ["8:orgid:me"]);
        assert_eq!(mentions(&Value::String(entry.to_string())), ["8:orgid:me"]);
        assert!(mentions(&Value::String("not json".to_owned())).is_empty());
        assert!(mentions(&Value::Null).is_empty());
    }

    #[test]
    fn the_read_horizon_is_its_first_field() {
        assert_eq!(
            read_horizon("1730000100000;1730000100001;55"),
            Some(1_730_000_100_000)
        );
        assert_eq!(read_horizon(""), None);
    }

    #[test]
    fn a_selection_is_everything_or_named_conversations() {
        assert_eq!(
            Selection::from_content(&json!({ "conversations": "*" })),
            Ok(Selection::All)
        );
        assert!(matches!(
            Selection::from_content(&json!({ "conversations": ["19:a@thread.v2"] })),
            Ok(Selection::Only(ids)) if ids.contains("19:a@thread.v2")
        ));
        Selection::from_content(&json!({ "conversations": [] })).expect_err("an empty list");
        Selection::from_content(&json!({ "conversations": [3] }))
            .expect_err("an id that is not text");
        assert_eq!(Selection::from_content(&json!({})), Ok(Selection::All));
    }

    #[test]
    fn a_read_looks_back_past_its_watermark() {
        let since = DateTime::from_timestamp_millis(1_730_000_600_000).expect("a time");
        let script = script(Some(since));
        assert!(script.contains("\"since\":1730000000000"));
        assert!(script.contains("\"maxMessages\":200"));
        assert!(script.contains("conversation-manager"));
    }
}
