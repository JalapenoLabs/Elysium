// Copyright © 2026 Jalapeno Labs

//! Posting on Slack as the person, through the web client the connector reads with. See
//! `docs/connectors.md`, Sending.
//!
//! `slack_send.js` runs in the browser's tab on `app.slack.com` and makes one call, Slack's
//! published `chat.postMessage`, with the session the web client keeps, exactly as the web
//! client posts what its person types. It can call nothing else, and the session token never
//! leaves the page.
//!
//! # Where a reply goes
//!
//! A record keeps where its message was said (`team`, `conversation`, `ts`, and `threadTs`
//! for a reply). A reply in the thread names the thread's first message as `thread_ts`, which
//! is the record's own `ts` unless it was itself a reply; Slack asks for the first message's
//! `ts`, never a reply's. A reply in the conversation names no thread.
//!
//! # Text
//!
//! The person writes plain text, so the three characters Slack's markup reserves are escaped
//! as Slack documents (`&amp;`, `&lt;`, `&gt;`): a typed `<` is shown, never read as a link
//! or a mention.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use super::{
    APP_ORIGINS, SIGNED_OUT_ERRORS, START_URL, Slack, is_app, is_sign_in, permalink, ts_time,
};
use crate::browser::devtools::DevtoolsError;
use crate::connectors::{ConnectorError, Sent};
use crate::models::browser_session::BrowserSession;
use crate::models::draft::ReplyPlace;
use crate::models::timeline_record::TimelineRecord;

const SCRIPT: &str = include_str!("../slack_send.js");
const REQUEST_PLACEHOLDER: &str = "__ELYSIUM_REQUEST__";

/// What a person sees when the browser is not signed in to Slack.
const SIGNED_OUT: &str =
    "Slack is signed out in this connector's browser; sign in again on the connector's page";

/// Where a Slack message goes: a conversation of a workspace, and the thread in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub team: String,
    pub channel: String,
    /// The thread's first message, for a reply in a thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
}

/// Where a reply to `record` goes, `place` in its conversation.
///
/// # Errors
/// Why the record cannot be answered: it does not say where its message was said.
pub fn reply_target(record: &TimelineRecord, place: ReplyPlace) -> Result<Target, String> {
    let text = |field: &str| record.metadata[field].as_str().map(str::to_owned);
    let (Some(team), Some(channel), Some(ts)) = (text("team"), text("conversation"), text("ts"))
    else {
        return Err("this Slack record does not say where its message was said".to_owned());
    };
    let thread_ts = match place {
        ReplyPlace::Thread => Some(text("threadTs").unwrap_or(ts)),
        ReplyPlace::Conversation => None,
    };
    Ok(Target {
        team,
        channel,
        thread_ts,
    })
}

/// Plain text as Slack's markup shows it: its three reserved characters escaped.
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Request<'target> {
    team: &'target str,
    channel: &'target str,
    text: String,
    thread_ts: Option<&'target str>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    signed_in: bool,
    ok: bool,
    error: Option<String>,
    channel: Option<String>,
    ts: Option<String>,
    team_url: Option<String>,
}

impl Slack {
    /// Posts `body` as the person signed in to `browser`'s Slack, to `target`.
    ///
    /// # Errors
    /// [`ConnectorError::Refused`] naming why Slack did not post it, such as a session that
    /// ended or a conversation the person left, and [`ConnectorError`] when the browser does
    /// not answer.
    pub async fn post(
        &self,
        browser: &BrowserSession,
        target: &Target,
        body: &str,
    ) -> Result<Sent, ConnectorError> {
        let port = u16::try_from(browser.port).map_err(|_overflow| {
            ConnectorError::Unexpected("a browser port out of range".to_owned())
        })?;
        let tabs = self.devtools.tabs(port).await?;
        if !tabs.iter().any(|tab| is_app(&tab.url)) {
            if tabs.iter().any(|tab| is_sign_in(&tab.url)) {
                return Err(ConnectorError::Refused(SIGNED_OUT.to_owned()));
            }
            let start = Url::parse(START_URL).expect("Slack's start page parses");
            self.devtools.open_tab(port, &start).await?;
            return Err(ConnectorError::Refused(
                "Slack was not open in this connector's browser; it is opening now, so send again \
                 in a moment"
                    .to_owned(),
            ));
        }

        let request = Request {
            team: &target.team,
            channel: &target.channel,
            text: escape(body),
            thread_ts: target.thread_ts.as_deref(),
        };
        let request = serde_json::to_string(&request)
            .map_err(|error| ConnectorError::Unexpected(error.to_string()))?;
        let script = SCRIPT.replace(REQUEST_PLACEHOLDER, &request);
        let value = match self.devtools.evaluate(port, APP_ORIGINS, &script).await {
            Ok(value) => value,
            Err(DevtoolsError::NoTab(_origins)) => {
                return Err(ConnectorError::Refused(
                    "Slack's tab closed before the message could be sent; send it again".to_owned(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        sent_from(target, &value)
    }
}

/// What the send script answered, as the message it posted.
///
/// # Errors
/// [`ConnectorError::Refused`] naming why Slack did not post it, and
/// [`ConnectorError::Unexpected`] for an answer that is not the script's shape.
pub fn sent_from(target: &Target, value: &Value) -> Result<Sent, ConnectorError> {
    let answer: Answer = serde_json::from_value(value.clone())
        .map_err(|error| ConnectorError::Unexpected(error.to_string()))?;
    let error = answer.error.as_deref().unwrap_or("unknown");
    if !answer.signed_in || (!answer.ok && SIGNED_OUT_ERRORS.contains(&error)) {
        return Err(ConnectorError::Refused(SIGNED_OUT.to_owned()));
    }
    if !answer.ok {
        let reason = match error {
            "team_not_signed_in" => {
                "this connector's browser is not signed in to the workspace this message was in"
                    .to_owned()
            }
            other => format!("Slack did not post the message: {other}"),
        };
        return Err(ConnectorError::Refused(reason));
    }
    let Some(ts) = answer.ts else {
        return Err(ConnectorError::Unexpected(
            "Slack posted the message but did not name it".to_owned(),
        ));
    };
    let channel = answer.channel.unwrap_or_else(|| target.channel.clone());

    let mut metadata = json!({ "team": target.team, "conversation": channel, "ts": ts });
    if let Some(thread) = &target.thread_ts {
        metadata["threadTs"] = json!(thread);
    }
    let url = answer
        .team_url
        .as_deref()
        .and_then(|team_url| permalink(team_url, &channel, &ts, target.thread_ts.as_deref()));
    Ok(Sent {
        external_id: format!("{}/{channel}/{ts}", target.team),
        occurred_at: ts_time(&ts).unwrap_or_else(Utc::now),
        url,
        metadata,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn record(metadata: Value) -> TimelineRecord {
        TimelineRecord {
            id: uuid::Uuid::now_v7(),
            source_kind: "slack".to_owned(),
            connector_id: None,
            external_id: Some("T1/C1/1730000000.000200".to_owned()),
            occurred_at: Utc::now(),
            actor: "Ada".to_owned(),
            title: "#launch in Mooreslab".to_owned(),
            body: "Ship it?".to_owned(),
            url: None,
            media: None,
            metadata,
            action_item_id: None,
            created_by: uuid::Uuid::now_v7(),
            created_at: Utc::now(),
        }
    }

    fn target(thread_ts: Option<&str>) -> Target {
        Target {
            team: "T1".to_owned(),
            channel: "C1".to_owned(),
            thread_ts: thread_ts.map(str::to_owned),
        }
    }

    #[test]
    fn a_thread_reply_to_a_channel_message_starts_its_thread() {
        let message =
            record(json!({ "team": "T1", "conversation": "C1", "ts": "1730000000.000200" }));
        assert_eq!(
            reply_target(&message, ReplyPlace::Thread),
            Ok(target(Some("1730000000.000200")))
        );
    }

    #[test]
    fn a_thread_reply_to_a_reply_goes_to_the_threads_first_message() {
        let reply = record(json!({
            "team": "T1",
            "conversation": "C1",
            "ts": "1730000000.000200",
            "threadTs": "1730000000.000100",
        }));
        assert_eq!(
            reply_target(&reply, ReplyPlace::Thread),
            Ok(target(Some("1730000000.000100")))
        );
    }

    #[test]
    fn a_reply_in_the_conversation_names_no_thread() {
        let reply = record(json!({
            "team": "T1",
            "conversation": "C1",
            "ts": "1730000000.000200",
            "threadTs": "1730000000.000100",
        }));
        assert_eq!(
            reply_target(&reply, ReplyPlace::Conversation),
            Ok(target(None))
        );
    }

    #[test]
    fn a_record_that_does_not_say_where_it_was_said_cannot_be_answered() {
        reply_target(&record(json!({})), ReplyPlace::Thread).expect_err("nowhere to reply");
    }

    #[test]
    fn typed_markup_is_shown_rather_than_read() {
        assert_eq!(
            escape("a < b && <@U1> > c"),
            "a &lt; b &amp;&amp; &lt;@U1&gt; &gt; c"
        );
    }

    #[test]
    fn a_posted_message_is_named_as_the_reader_names_it() {
        let answer = json!({
            "signedIn": true,
            "ok": true,
            "error": null,
            "channel": "C1",
            "ts": "1730000100.000300",
            "teamUrl": "https://mooreslab.slack.com/",
        });
        let sent = sent_from(&target(Some("1730000000.000100")), &answer).expect("posted");
        assert_eq!(sent.external_id, "T1/C1/1730000100.000300");
        assert_eq!(
            sent.metadata,
            json!({
                "team": "T1",
                "conversation": "C1",
                "ts": "1730000100.000300",
                "threadTs": "1730000000.000100",
            })
        );
        assert_eq!(
            sent.url.as_deref(),
            Some(
                "https://mooreslab.slack.com/archives/C1/p1730000100000300\
                 ?thread_ts=1730000000.000100&cid=C1"
            )
        );
        assert_eq!(sent.occurred_at.timestamp(), 1_730_000_100);
    }

    #[test]
    fn a_session_that_ended_says_to_sign_in_again() {
        for answer in [
            json!({ "signedIn": false, "ok": false, "error": "not_authed" }),
            json!({ "signedIn": true, "ok": false, "error": "invalid_auth" }),
        ] {
            let refused = sent_from(&target(None), &answer).expect_err("refused");
            assert!(refused.to_string().contains("sign in again"), "{refused}");
        }
    }

    #[test]
    fn a_refusal_names_slacks_reason() {
        let answer = json!({ "signedIn": true, "ok": false, "error": "not_in_channel" });
        let refused = sent_from(&target(None), &answer).expect_err("refused");
        assert_eq!(
            refused.to_string(),
            "Slack did not post the message: not_in_channel"
        );
    }
}
