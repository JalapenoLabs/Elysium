// Copyright © 2026 Jalapeno Labs

//! Relays Neko's signaling between a viewer's browser and the browser it is watching.
//!
//! The viewer holds the API's WebSocket; the API holds a member session's socket to Neko.
//! Every message is one JSON object, `{"event": ..., "payload": ...}`. From the viewer, only
//! the events in [`VIEWER_EVENTS`] pass: what negotiating media, holding control, sending
//! input, and pasting need. Neko already refuses a member the administrator's events, so
//! the list is the second wall: an event it misses is a bug, not an escalation. From Neko,
//! everything passes but its logs. Media and input never come here: they flow over WebRTC
//! between the viewer and the host's media port.

use axum::extract::ws::{Message as ViewerMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message as NekoMessage;
use tracing::{Level, event};

use super::neko::Upstream;

/// What a viewer may send Neko, by event name (`server/pkg/types/event` in Neko 3.1.6).
const VIEWER_EVENTS: [&str; 25] = [
    "client/heartbeat",
    "signal/request",
    "signal/restart",
    "signal/offer",
    "signal/answer",
    "signal/candidate",
    "signal/video",
    "signal/audio",
    "control/request",
    "control/release",
    "control/move",
    "control/scroll",
    "control/buttonpress",
    "control/buttondown",
    "control/buttonup",
    "control/keypress",
    "control/keydown",
    "control/keyup",
    "control/cut",
    "control/copy",
    "control/paste",
    "control/select_all",
    "clipboard/set",
    "keyboard/map",
    "keyboard/modifiers",
];

/// What Neko sends that a viewer never sees: its server's log lines.
const WITHHELD_EVENTS: [&str; 1] = ["system/logs"];

/// The event a message names, when it is a JSON object with a string `event`. Read as a
/// map on purpose: serde fills a struct from an array by position, so a derived struct
/// would also take `["signal/request"]`, which Neko does not.
fn event_name(text: &str) -> Option<String> {
    let Ok(Value::Object(message)) = serde_json::from_str::<Value>(text) else {
        return None;
    };
    message
        .get("event")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Whether a viewer's message may reach Neko.
pub fn viewer_may_send(text: &str) -> bool {
    event_name(text).is_some_and(|name| VIEWER_EVENTS.contains(&name.as_str()))
}

/// Whether Neko's message may reach the viewer.
pub fn viewer_may_receive(text: &str) -> bool {
    event_name(text).is_some_and(|name| !WITHHELD_EVENTS.contains(&name.as_str()))
}

/// Relays until either side closes or `until` finishes, then closes both.
pub async fn run(viewer: WebSocket, upstream: Upstream, until: impl Future<Output = ()>) {
    let (mut viewer_sink, mut viewer_stream) = viewer.split();
    let (mut neko_sink, mut neko_stream) = upstream.split();

    let to_neko = async {
        while let Some(Ok(message)) = viewer_stream.next().await {
            let ViewerMessage::Text(text) = message else {
                if matches!(message, ViewerMessage::Close(_)) {
                    break;
                }
                continue;
            };
            if !viewer_may_send(text.as_str()) {
                event!(
                    name: "browser.relay.dropped",
                    Level::DEBUG,
                    browser.relay.event = event_name(text.as_str()).as_deref().unwrap_or("<not an event>"),
                    "dropped a viewer message the relay does not pass",
                );
                continue;
            }
            if neko_sink
                .send(NekoMessage::text(text.as_str()))
                .await
                .is_err()
            {
                break;
            }
        }
    };

    let to_viewer = async {
        while let Some(Ok(message)) = neko_stream.next().await {
            let NekoMessage::Text(text) = message else {
                if matches!(message, NekoMessage::Close(_)) {
                    break;
                }
                continue;
            };
            if !viewer_may_receive(text.as_str()) {
                continue;
            }
            if viewer_sink
                .send(ViewerMessage::text(text.as_str()))
                .await
                .is_err()
            {
                break;
            }
        }
    };

    tokio::select! {
        () = to_neko => {}
        () = to_viewer => {}
        () = until => {}
    }

    // Either side may already be gone; closing what is left is all that remains to do.
    let _ = viewer_sink.close().await;
    let _ = neko_sink.close().await;
    event!(name: "browser.relay.closed", Level::DEBUG, "a browser stream closed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_viewer_may_negotiate_media_hold_control_and_paste() {
        for message in [
            r#"{"event":"signal/request","payload":{"video":{},"audio":{}}}"#,
            r#"{"event":"signal/answer","payload":{"sdp":"v=0"}}"#,
            r#"{"event":"control/request"}"#,
            r#"{"event":"clipboard/set","payload":{"text":"hunter2"}}"#,
            r#"{"event":"client/heartbeat"}"#,
        ] {
            assert!(viewer_may_send(message), "{message}");
        }
    }

    #[test]
    fn a_viewer_may_not_reach_administration_or_plugins() {
        for message in [
            r#"{"event":"broadcast/start","payload":{"url":"rtmp://elsewhere"}}"#,
            r#"{"event":"system/admin"}"#,
            r#"{"event":"send/broadcast","payload":{}}"#,
            r#"{"event":"chat/message","payload":{"content":"hi"}}"#,
            r#"{"event":"filetransfer/refresh"}"#,
            r#"{"event":"control/take"}"#,
            r#"["signal/request"]"#,
            r"not json",
            r#"{"payload":{}}"#,
        ] {
            assert!(!viewer_may_send(message), "{message}");
        }
    }

    #[test]
    fn neko_logs_never_reach_a_viewer() {
        assert!(viewer_may_receive(
            r#"{"event":"signal/provide","payload":{"sdp":"v=0"}}"#
        ));
        assert!(!viewer_may_receive(
            r#"{"event":"system/logs","payload":[]}"#
        ));
        assert!(!viewer_may_receive("not json"));
    }
}
