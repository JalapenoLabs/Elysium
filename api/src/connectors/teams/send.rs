// Copyright © 2026 Jalapeno Labs

//! Posting on Microsoft Teams as the person, through the browser the connector reads with. See
//! `docs/connectors.md`, Sending.
//!
//! `teams_send.js` runs in the browser's Teams tab and posts to the chat service the web client
//! itself posts to (`/api/chatsvc/<region>/v1/users/ME/conversations/<id>/messages`), with the
//! web client's own token for it. The chat service is not a published API, so the script finds
//! its token and address defensively and names what it could not find; [`sent_from`] turns
//! that into a reason the person can act on.
//!
//! # Where a reply goes
//!
//! A record keeps where its message was said (`conversationId`, `messageId`, `parentId` for a
//! message in a channel thread, and `isChat`). A chat has no threads, so a reply to a chat
//! message is a new message in the chat wherever it is placed. In a channel, a reply in the
//! thread posts under the thread's first message (`<channel>;messageid=<first>`), and a reply in
//! the conversation starts a new post.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use super::{SIGN_IN_ORIGINS, START_URL, TEAMS_ORIGINS, Teams, message_link, starts_with_any};
use crate::browser::devtools::DevtoolsError;
use crate::connectors::{ConnectorError, Sent};
use crate::models::browser_session::BrowserSession;
use crate::models::draft::ReplyPlace;
use crate::models::timeline_record::TimelineRecord;

const SCRIPT: &str = include_str!("../teams_send.js");
const REQUEST_PLACEHOLDER: &str = "__ELYSIUM_REQUEST__";

/// What a person sees when the browser is not signed in to Teams.
const SIGNED_OUT: &str =
    "Teams is signed out in this connector's browser; sign in again on the connector's page";

/// Where a Teams message goes: a chat or channel, and for a channel reply, its thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub conversation_id: String,
    /// The thread's first message, for a reply in a channel thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_message_id: Option<String>,
    /// Whether the conversation is a chat rather than a channel.
    pub is_chat: bool,
}

/// Where a reply to `record` goes, `place` in its conversation.
///
/// # Errors
/// Why the record cannot be answered: it does not say where its message was said.
pub fn reply_target(record: &TimelineRecord, place: ReplyPlace) -> Result<Target, String> {
    let text = |field: &str| record.metadata[field].as_str().map(str::to_owned);
    let (Some(conversation_id), Some(message_id), Some(is_chat)) = (
        text("conversationId"),
        text("messageId"),
        record.metadata["isChat"].as_bool(),
    ) else {
        return Err("this Teams record does not say where its message was said".to_owned());
    };
    let root_message_id = match place {
        ReplyPlace::Thread if !is_chat => Some(text("parentId").unwrap_or(message_id)),
        ReplyPlace::Thread | ReplyPlace::Conversation => None,
    };
    Ok(Target {
        conversation_id,
        root_message_id,
        is_chat,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Request<'target> {
    conversation_id: &'target str,
    root_message_id: Option<&'target str>,
    text: &'target str,
    sender_name: &'target str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    signed_in: bool,
    ok: bool,
    error: Option<String>,
    status: Option<u16>,
    message: Option<String>,
    /// The chat service names a posted message by its arrival, in milliseconds, which is also
    /// the id its store keeps it under.
    arrival_time: Option<i64>,
}

impl Teams {
    /// Posts `body` as the person signed in to `browser`'s Teams, to `target`, with
    /// `sender_name` beside it as Teams shows its sender.
    ///
    /// # Errors
    /// [`ConnectorError::Refused`] naming why it was not posted, such as a session that ended
    /// or a token the page does not hold, and [`ConnectorError`] when the browser does not
    /// answer.
    pub async fn post(
        &self,
        browser: &BrowserSession,
        target: &Target,
        body: &str,
        sender_name: &str,
    ) -> Result<Sent, ConnectorError> {
        let port = u16::try_from(browser.port).map_err(|_overflow| {
            ConnectorError::Unexpected("a browser port out of range".to_owned())
        })?;
        let tabs = self.devtools.tabs(port).await?;
        if !tabs
            .iter()
            .any(|tab| starts_with_any(&tab.url, TEAMS_ORIGINS))
        {
            if tabs
                .iter()
                .any(|tab| starts_with_any(&tab.url, SIGN_IN_ORIGINS))
            {
                return Err(ConnectorError::Refused(SIGNED_OUT.to_owned()));
            }
            let start = Url::parse(START_URL).expect("the Teams start page is a URL");
            self.devtools.open_tab(port, &start).await?;
            return Err(ConnectorError::Refused(
                "Teams was not open in this connector's browser; it is opening now, so send again \
                 in a moment"
                    .to_owned(),
            ));
        }

        let request = Request {
            conversation_id: &target.conversation_id,
            root_message_id: target.root_message_id.as_deref(),
            text: body,
            sender_name,
        };
        let request = serde_json::to_string(&request)
            .map_err(|error| ConnectorError::Unexpected(error.to_string()))?;
        let script = SCRIPT.replace(REQUEST_PLACEHOLDER, &request);
        let value = match self.devtools.evaluate(port, TEAMS_ORIGINS, &script).await {
            Ok(value) => value,
            Err(DevtoolsError::NoTab(_origins)) => {
                return Err(ConnectorError::Refused(
                    "Teams' tab closed before the message could be sent; send it again".to_owned(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        sent_from(target, &value, Utc::now())
    }
}

/// What the send script answered, as the message it posted, `now` standing in for when it
/// arrived if the chat service did not say.
///
/// # Errors
/// [`ConnectorError::Refused`] naming why it was not posted, and
/// [`ConnectorError::Unexpected`] for an answer that is not the script's shape.
pub fn sent_from(
    target: &Target,
    value: &Value,
    now: DateTime<Utc>,
) -> Result<Sent, ConnectorError> {
    let answer: Answer = serde_json::from_value(value.clone())
        .map_err(|error| ConnectorError::Unexpected(error.to_string()))?;
    if !answer.signed_in {
        return Err(ConnectorError::Refused(SIGNED_OUT.to_owned()));
    }
    if !answer.ok {
        let reason = match answer.error.as_deref().unwrap_or("unknown") {
            "token_expired" => "Teams' session in this connector's browser has expired; open \
                                Teams on the connector's page so it signs in again, then send"
                .to_owned(),
            "token_not_found" => "Teams in this connector's browser holds no token Elysium can \
                                  send with; open Teams on the connector's page, then send again"
                .to_owned(),
            "region_unknown" => "Teams has not loaded its chats in this connector's browser yet; \
                                 send again in a moment"
                .to_owned(),
            "http_error" => format!(
                "Teams did not post the message: it answered {}",
                answer
                    .status
                    .map_or_else(|| "an error".to_owned(), |status| status.to_string())
            ),
            "fetch_failed" => format!(
                "Teams could not be reached: {}",
                answer.message.as_deref().unwrap_or("the request failed")
            ),
            other => format!("Teams did not post the message: {other}"),
        };
        return Err(ConnectorError::Refused(reason));
    }

    let arrived_at = answer
        .arrival_time
        .and_then(DateTime::from_timestamp_millis);
    let message_id = answer.arrival_time.map_or_else(
        || now.timestamp_millis().to_string(),
        |arrival| arrival.to_string(),
    );
    let mut metadata = json!({
        "conversationId": target.conversation_id,
        "messageId": message_id,
        "isChat": target.is_chat,
    });
    if let Some(root) = &target.root_message_id {
        metadata["parentId"] = json!(root);
    }
    Ok(Sent {
        external_id: format!("{}/{message_id}", target.conversation_id),
        occurred_at: arrived_at.unwrap_or(now),
        url: message_link(&target.conversation_id, &message_id),
        metadata,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const CHANNEL: &str = "19:launch@thread.tacv2";
    const CHAT: &str = "19:ada_me@unq.gbl.spaces";

    fn record(metadata: Value) -> TimelineRecord {
        TimelineRecord {
            id: uuid::Uuid::now_v7(),
            source_kind: "teams".to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: Utc::now(),
            actor: "Ada Lovelace".to_owned(),
            title: "Launch".to_owned(),
            body: "Ship it?".to_owned(),
            url: None,
            media: None,
            metadata,
            action_item_id: None,
            created_by: uuid::Uuid::now_v7(),
            created_at: Utc::now(),
            excerpt_mark_id: None,
            deleted_at: None,
        }
    }

    #[test]
    fn a_thread_reply_in_a_channel_posts_under_the_threads_first_message() {
        let reply = record(json!({
            "conversationId": CHANNEL,
            "messageId": "1730000300000",
            "parentId": "1730000100000",
            "isChat": false,
        }));
        let target = reply_target(&reply, ReplyPlace::Thread).expect("answerable");
        assert_eq!(target.root_message_id.as_deref(), Some("1730000100000"));

        let first = record(json!({
            "conversationId": CHANNEL,
            "messageId": "1730000100000",
            "isChat": false,
        }));
        let target = reply_target(&first, ReplyPlace::Thread).expect("answerable");
        assert_eq!(target.root_message_id.as_deref(), Some("1730000100000"));
    }

    #[test]
    fn a_reply_in_a_channels_conversation_starts_a_new_post() {
        let reply = record(json!({
            "conversationId": CHANNEL,
            "messageId": "1730000300000",
            "parentId": "1730000100000",
            "isChat": false,
        }));
        let target = reply_target(&reply, ReplyPlace::Conversation).expect("answerable");
        assert_eq!(target.root_message_id, None);
    }

    #[test]
    fn a_chat_has_no_threads() {
        let message = record(json!({
            "conversationId": CHAT,
            "messageId": "1730000200000",
            "isChat": true,
        }));
        let target = reply_target(&message, ReplyPlace::Thread).expect("answerable");
        assert_eq!(
            target,
            Target {
                conversation_id: CHAT.to_owned(),
                root_message_id: None,
                is_chat: true,
            }
        );
    }

    #[test]
    fn a_record_that_does_not_say_where_it_was_said_cannot_be_answered() {
        reply_target(&record(json!({})), ReplyPlace::Thread).expect_err("nowhere to reply");
    }

    #[test]
    fn a_posted_message_is_named_as_the_reader_names_it() {
        let target = Target {
            conversation_id: CHANNEL.to_owned(),
            root_message_id: Some("1730000100000".to_owned()),
            is_chat: false,
        };
        let answer = json!({ "signedIn": true, "ok": true, "arrivalTime": 1_730_000_400_000_i64 });
        let sent = sent_from(&target, &answer, Utc::now()).expect("posted");
        assert_eq!(sent.external_id, format!("{CHANNEL}/1730000400000"));
        assert_eq!(sent.occurred_at.timestamp_millis(), 1_730_000_400_000);
        assert_eq!(
            sent.metadata,
            json!({
                "conversationId": CHANNEL,
                "messageId": "1730000400000",
                "parentId": "1730000100000",
                "isChat": false,
            })
        );
        assert!(
            sent.url
                .is_some_and(|url| url.starts_with("https://teams.microsoft.com/l/message/"))
        );
    }

    #[test]
    fn each_missing_piece_is_named_in_words_the_person_can_act_on() {
        let target = Target {
            conversation_id: CHAT.to_owned(),
            root_message_id: None,
            is_chat: true,
        };
        for (error, expected) in [
            ("token_expired", "has expired"),
            ("token_not_found", "holds no token"),
            ("region_unknown", "has not loaded its chats"),
        ] {
            let answer = json!({ "signedIn": true, "ok": false, "error": error });
            let refused = sent_from(&target, &answer, Utc::now()).expect_err("refused");
            assert!(refused.to_string().contains(expected), "{error}: {refused}");
        }
        let answer = json!({ "signedIn": true, "ok": false, "error": "http_error", "status": 403 });
        let refused = sent_from(&target, &answer, Utc::now()).expect_err("refused");
        assert_eq!(
            refused.to_string(),
            "Teams did not post the message: it answered 403"
        );

        let answer =
            json!({ "signedIn": false, "ok": false, "error": "http_error", "status": 401 });
        let refused = sent_from(&target, &answer, Utc::now()).expect_err("refused");
        assert!(refused.to_string().contains("sign in again"), "{refused}");
    }
}
